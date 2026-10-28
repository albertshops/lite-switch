//! Read i3's own variable-expanded parser trace, retaining exact source spans.
use super::{PrivateDir, shell_quote, transaction::Image};
use crate::ipc::Result;
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_int},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone)]
pub(super) struct Source {
    pub path: PathBuf,
    pub origin: PathBuf,
    pub before: Image,
    pub expanded: Vec<String>,
}

pub(super) fn lines(text: &str) -> Vec<(usize, String, String)> {
    let mut result = Vec::new();
    let mut original = String::new();
    let mut logical = String::new();
    let mut start = 1;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if original.is_empty() {
            start = index + 1;
        }
        original.push_str(line);
        let comment = if logical.is_empty() {
            line.trim_start().starts_with('#')
        } else {
            logical.trim_start().starts_with('#')
        };
        if !comment && line.ends_with("\\\n") {
            logical.push_str(&line[..line.len() - 2]);
        } else {
            logical.push_str(line.trim_end_matches('\n'));
            result.push((
                start,
                std::mem::take(&mut original),
                std::mem::take(&mut logical),
            ));
        }
    }
    if !original.is_empty() {
        result.push((start, original, logical));
    }
    result
}

pub(super) fn trace(root: &Path) -> Result<BTreeMap<PathBuf, Source>> {
    let output = Command::new("i3")
        .args(["-C", "-d", "all", "-V", "-c"])
        .arg(root)
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut files: BTreeMap<PathBuf, Source> = BTreeMap::new();
    let mut current = None;
    for line in text.lines() {
        let Some((_, message)) = line.split_once(" - ") else {
            continue;
        };
        if let Some(path) = message
            .strip_prefix("Parsing configfile ")
            .or_else(|| message.strip_prefix("Including config file "))
        {
            let origin = PathBuf::from(path);
            let path = fs::canonicalize(&origin)?;
            let before = Image::read(&path)?.ok_or("config source disappeared")?;
            files.entry(path.clone()).or_insert(Source {
                path: path.clone(),
                origin,
                before,
                expanded: Vec::new(),
            });
            current = Some(path);
        } else if message.contains(":parse_config:")
            && let Some((_, record)) = message.split_once(" - CONFIG(line ")
        {
            let Some((number, contents)) = record.split_once("): ") else {
                return Err("unrecognized i3 parser trace".into());
            };
            let number: usize = number.trim().parse()?;
            let file = files
                .get_mut(current.as_ref().ok_or("i3 trace omitted source path")?)
                .unwrap();
            if number != file.expanded.len() + 1 {
                return Err(
                    "non-contiguous i3 parser trace; refusing to guess source positions".into(),
                );
            }
            file.expanded.push(contents.to_owned());
        }
    }
    if files.is_empty() {
        return Err("i3 did not emit a supported parser trace".into());
    }
    for source in files.values() {
        let raw = std::str::from_utf8(&source.before.bytes)?;
        if lines(raw).len() != source.expanded.len() {
            return Err(format!(
                "i3 trace/source line mismatch at {}; no files changed",
                source.path.display()
            )
            .into());
        }
    }
    Ok(files)
}

// POSIX wordexp_t has these three public fields on supported Linux platforms.
#[repr(C)]
struct Words {
    count: usize,
    words: *mut *mut c_char,
    offset: usize,
}
unsafe extern "C" {
    fn wordexp(input: *const c_char, result: *mut Words, flags: c_int) -> c_int;
    fn wordfree(result: *mut Words);
}

/// Runs only in a fresh helper process; changing cwd cannot affect GTK threads.
pub(crate) fn expansion_helper(base: &Path, expression: &str) -> Result<String> {
    std::env::set_current_dir(base)?;
    let input = CString::new(expression)?;
    let mut words = Words {
        count: 0,
        words: std::ptr::null_mut(),
        offset: 0,
    };
    // WRDE_NOCMD: never execute command substitution in the helper.
    let status = unsafe { wordexp(input.as_ptr(), &mut words, 4) };
    if status != 0 {
        if status == 1 {
            unsafe { wordfree(&mut words) };
        }
        return Err(format!("include expansion rejected (wordexp status {status}); command substitutions are not supported").into());
    }
    let result = (|| -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for index in 0..words.count {
            let value = unsafe { CStr::from_ptr(*words.words.add(index)) }.to_str()?;
            let path = Path::new(value);
            paths.push(if path.is_absolute() {
                path.to_owned()
            } else {
                base.join(path)
            });
        }
        Ok(paths)
    })();
    unsafe { wordfree(&mut words) };
    Ok(serde_json::to_string(&result?)?)
}

fn expand(base: &Path, expression: &str) -> Result<Vec<PathBuf>> {
    let output = Command::new(std::env::current_exe()?)
        .arg("__expand-include")
        .arg(base)
        .arg(expression)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cannot resolve include: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(super) struct Projection {
    _directory: PrivateDir,
    pub paths: BTreeMap<PathBuf, PathBuf>,
    include_lines: BTreeMap<PathBuf, BTreeMap<String, Vec<PathBuf>>>,
}

impl Projection {
    pub fn new(files: &BTreeMap<PathBuf, Source>) -> Result<Self> {
        let directory = PrivateDir::new("binding-projection")?;
        let paths = files
            .keys()
            .enumerate()
            .map(|(index, original)| (original.clone(), directory.0.join(format!("{index}.conf"))))
            .collect();
        let mut include_lines = BTreeMap::new();
        for (path, source) in files {
            let mut includes = BTreeMap::new();
            for ((_, original, _), expanded) in lines(std::str::from_utf8(&source.before.bytes)?)
                .iter()
                .zip(&source.expanded)
            {
                let statement = expanded.trim_start();
                if let Some(expression) = statement
                    .strip_prefix("include")
                    .filter(|rest| rest.starts_with(char::is_whitespace))
                {
                    // i3 strips enclosing double quotes before wordexp.
                    let expression = expression.trim();
                    let expression = expression
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(expression)
                        .replace("\\\"", "\"");
                    includes.insert(
                        original.clone(),
                        expand(
                            source.origin.parent().ok_or("source has no parent")?,
                            &expression,
                        )?,
                    );
                }
            }
            include_lines.insert(path.clone(), includes);
        }
        Ok(Self {
            _directory: directory,
            paths,
            include_lines,
        })
    }

    pub fn write(
        &self,
        contents: &BTreeMap<PathBuf, String>,
        root: &Path,
        extra: &str,
    ) -> Result<PathBuf> {
        for (path, output) in &self.paths {
            let source = contents
                .get(path)
                .ok_or("projection omitted source contents")?;
            let mut result = String::new();
            for (_, original, _) in lines(source) {
                if let Some(includes) = self
                    .include_lines
                    .get(path)
                    .and_then(|lines| lines.get(&original))
                {
                    let targets = includes
                        .iter()
                        .map(|include| {
                            let canonical =
                                fs::canonicalize(include).unwrap_or_else(|_| include.clone());
                            shell_quote(self.paths.get(&canonical).unwrap_or(include))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if targets.is_empty() {
                        result.push_str("# Empty include\n");
                    } else {
                        result.push_str(&format!("include {}\n", targets.join(" ")));
                    }
                } else {
                    result.push_str(&original);
                }
            }
            if path == root && !extra.is_empty() {
                if !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(extra);
            }
            fs::write(output, result)?;
        }
        self.paths
            .get(root)
            .cloned()
            .ok_or_else(|| "projection omitted root config".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spans_keep_continuations_comments_and_missing_final_newline() {
        let spans = lines("bindsym Mod1+a \\\n exec app\n# comment \\\nlast");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].2, "bindsym Mod1+a  exec app");
        assert_eq!(spans[1].0, 3);
        assert_eq!(spans[2].1, "last");
    }
}
