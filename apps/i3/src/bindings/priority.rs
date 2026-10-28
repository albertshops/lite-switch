//! New Lite Switch chords take precedence; displaced source is reversible.
use super::{
    source::{self, Projection},
    transaction::{Change, Image},
    validate_config,
};
use crate::ipc::Result;
use gtk::{gdk, prelude::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Displaced {
    pub path: PathBuf,
    pub line: usize,
    pub chord: String,
    pub original: String,
    pub replacement: String,
}

pub(super) fn restore(
    contents: &mut BTreeMap<PathBuf, String>,
    displaced: &[Displaced],
) -> Result<()> {
    for entry in displaced {
        let text = contents
            .get_mut(&entry.path)
            .ok_or("displaced source missing from plan")?;
        if !entry.replacement.starts_with("# LITE SWITCH DISPLACED v1 ")
            || text.matches(&entry.replacement).count() != 1
        {
            return Err(format!(
                "displaced binding block was edited at {}; refusing to overwrite it",
                entry.path.display()
            )
            .into());
        }
        *text = text.replacen(&entry.replacement, &entry.original, 1);
    }
    Ok(())
}

fn replacement(path: &Path, line: usize, original: &str) -> String {
    // Stable, non-security identifier; exact block/content checks protect edits.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path
        .to_string_lossy()
        .bytes()
        .chain(line.to_le_bytes())
        .chain(original.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    let mut text = format!("# LITE SWITCH DISPLACED v1 {hash:016x}\n");
    for physical in original.split_inclusive('\n') {
        text.push_str("# ");
        text.push_str(physical);
        if !physical.ends_with('\n') {
            text.push('\n');
        }
    }
    text.push_str(&format!("# END LITE SWITCH DISPLACED {hash:016x}\n"));
    text
}

struct Chord {
    mask: u32,
    symbol: String,
    code: Option<u32>,
}
fn chord(token: &str, code: bool) -> Option<Chord> {
    let token = token.trim_matches('"');
    let mut parts: Vec<_> = token.split('+').collect();
    let symbol = parts.pop()?.to_owned();
    let mut mask = 0;
    for modifier in parts {
        mask |= match modifier.to_ascii_lowercase().as_str() {
            "shift" => 1,
            "control" | "ctrl" => 4,
            "mod1" => 8,
            // NumLock/explicit layout groups must not beat our default chord.
            "mod2" | "group1" | "group2" | "group3" | "group4" | "mode_switch" => 0,
            "mod3" => 32,
            "mod4" => 64,
            "mod5" => 128,
            _ => return None,
        };
    }
    Some(Chord {
        mask,
        code: if code {
            Some(symbol.parse().ok()?)
        } else {
            None
        },
        symbol,
    })
}

fn structural(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            }
        } else if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character == '#' {
            return line[..index].trim();
        }
    }
    line.trim()
}

fn binding(line: &str) -> Option<(Chord, String)> {
    let mut words = line.split_whitespace();
    let directive = words.next()?;
    if !["bindsym", "bindcode"].contains(&directive.to_ascii_lowercase().as_str()) {
        return None;
    }
    let token = words.find(|token| !token.starts_with("--"))?;
    Some((
        chord(token, directive.eq_ignore_ascii_case("bindcode"))?,
        token.to_owned(),
    ))
}

fn same_symbol(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
        || gdk::Key::from_name(a)
            .zip(gdk::Key::from_name(b))
            .is_some_and(|(a, b)| a == b)
}

fn codes(target: &Chord) -> Result<BTreeSet<u32>> {
    let display = gdk::Display::default()
        .ok_or("X11 display is required to compare physical binding codes")?;
    let key = gdk::Key::from_name(&target.symbol).ok_or("cannot resolve shortcut keysym")?;
    let mut codes = BTreeSet::new();
    for mapping in display.map_keyval(key).unwrap_or_default() {
        codes.insert(mapping.keycode());
    }
    Ok(codes)
}

fn overlaps(candidate: &Chord, targets: &[Chord]) -> Result<bool> {
    for target in targets {
        if candidate.mask != target.mask {
            continue;
        }
        if let Some(code) = candidate.code {
            if codes(target)?.contains(&code) {
                return Ok(true);
            }
        } else if same_symbol(&candidate.symbol, &target.symbol) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) struct Plan {
    pub changes: Vec<Change>,
    pub displaced: Vec<Displaced>,
    pub root_after: String,
    pub root_before: Image,
}

impl Plan {
    pub fn report(&self) -> String {
        report(&self.displaced)
    }
}

pub(super) fn report(displaced: &[Displaced]) -> String {
    let mut text = format!(
        "Lite Switch takes priority; {} existing binding(s) displaced.\n",
        displaced.len()
    );
    for entry in displaced {
        text.push_str(&format!(
            "  {}:{} [{}] {}\n",
            entry.path.display(),
            entry.line,
            entry.chord,
            entry.original.trim().replace('\n', " ")
        ));
    }
    text
}

pub(super) fn build(
    root_config: &Path,
    clean_root: &str,
    previous: &[Displaced],
    proposed: &str,
    owned_include: &Path,
) -> Result<Plan> {
    // First ask i3 to discover/expand the current include tree. Raw sources are
    // retained for exact restoration, while i3—not us—expands binding variables.
    let root = fs::canonicalize(root_config)?;
    let mut files = source::trace(root_config)?;
    let mut contents = files
        .iter()
        .map(|(p, f)| Ok((p.clone(), String::from_utf8(f.before.bytes.clone())?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut snapshots: BTreeMap<_, _> = files
        .iter()
        .map(|(path, source)| (path.clone(), source.before.clone()))
        .collect();
    for entry in previous {
        if !contents.contains_key(&entry.path) {
            let image = Image::read(&entry.path)?.ok_or("displaced source disappeared")?;
            contents.insert(entry.path.clone(), String::from_utf8(image.bytes.clone())?);
            snapshots.insert(entry.path.clone(), image);
        }
    }
    contents.insert(root.clone(), clean_root.to_owned());
    restore(&mut contents, previous)?;
    let root_before = files
        .get(&root)
        .ok_or("i3 trace omitted root")?
        .before
        .clone();
    // Do not trace/rewrite our own generated bindings as user source.
    let owned = fs::canonicalize(owned_include).unwrap_or_else(|_| owned_include.to_owned());
    files.remove(&owned);
    let projection = Projection::new(&files)?;
    let baseline = projection.write(&contents, &root, "")?;
    let expanded = source::trace(&baseline)?;
    let targets: Vec<_> = proposed
        .lines()
        .filter_map(binding)
        .map(|(chord, _)| chord)
        .collect();
    let mut displaced = Vec::new();
    for (original_path, shadow) in &projection.paths {
        let Some(parsed) = expanded.get(&fs::canonicalize(shadow)?) else {
            continue;
        };
        let original = contents.get(original_path).unwrap().clone();
        let spans = source::lines(&original);
        if spans.len() != parsed.expanded.len() {
            return Err(
                "projection/source line mismatch; refusing to guess binding locations".into(),
            );
        }
        let mut blocks = Vec::new();
        let mut rewritten = String::new();
        for ((line, raw, _), resolved) in spans.into_iter().zip(&parsed.expanded) {
            let trimmed = structural(resolved);
            if trimmed == "}" {
                blocks.pop();
            }
            let block_type = ["mode", "bar", "colors"].into_iter().find(|kind| {
                trimmed.strip_prefix(kind).is_some_and(|rest| {
                    rest.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '{')
                })
            });
            if let Some(kind) = block_type
                && trimmed.ends_with('{')
            {
                blocks.push(kind.to_owned());
            }
            if ["bindsym", "bindcode"].iter().any(|directive| {
                trimmed
                    .strip_prefix(directive)
                    .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(char::is_whitespace))
            }) {
                return Err(
                    "unsupported compact binding syntax; no existing config was changed".into(),
                );
            }
            let collision = if blocks.is_empty() {
                binding(resolved)
                    .map(|(candidate, token)| {
                        overlaps(&candidate, &targets).map(|matched| (matched, token))
                    })
                    .transpose()?
            } else {
                None
            };
            if let Some((true, token)) = collision {
                let replacement = replacement(original_path, line, &raw);
                rewritten.push_str(&replacement);
                displaced.push(Displaced {
                    path: original_path.clone(),
                    line,
                    chord: token,
                    original: raw,
                    replacement,
                });
            } else {
                rewritten.push_str(&raw);
            }
        }
        contents.insert(original_path.clone(), rewritten);
    }
    let effective = projection.write(&contents, &root, proposed)?;
    validate_config(&effective)?;
    let mut changes = Vec::new();
    let root_after = contents.get(&root).unwrap().clone();
    for (path, after) in contents {
        if path == root || path == owned {
            continue;
        }
        let before = snapshots
            .remove(&path)
            .ok_or("source snapshot disappeared")?;
        // Even unchanged sources are optimistic transaction guards: a concurrent
        // edit must not invalidate the configuration we just validated.
        changes.push(Change {
            path,
            after: Some(Image::text(&after, before.mode)),
            before: Some(before),
        });
    }
    Ok(Plan {
        changes,
        displaced,
        root_after,
        root_before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_headers_ignore_only_unquoted_trailing_comments() {
        assert_eq!(
            structural("mode \"resize #1\" { # comment"),
            "mode \"resize #1\" {"
        );
        assert_eq!(structural("} # end mode"), "}");
    }

    #[test]
    fn symbols_modifiers_and_reversible_comment_blocks() {
        let old = chord("Shift+Mod1+a", false).unwrap();
        let new = chord("Mod1+Shift+a", false).unwrap();
        assert!(overlaps(&old, &[new]).unwrap());
        assert!(
            !overlaps(
                &chord("Mod4+a", false).unwrap(),
                &[chord("Mod1+a", false).unwrap()]
            )
            .unwrap()
        );
        let path = PathBuf::from("/tmp/example");
        let raw = "bindsym $mod+space \\\n exec original";
        let entry = Displaced {
            path: path.clone(),
            line: 1,
            chord: "Mod1+space".into(),
            original: raw.into(),
            replacement: replacement(&path, 1, raw),
        };
        let mut contents = BTreeMap::from([(path.clone(), entry.replacement.clone())]);
        restore(&mut contents, std::slice::from_ref(&entry)).unwrap();
        assert_eq!(contents[&path], raw);
        assert!(restore(&mut contents, &[entry]).is_err());
    }
}
