//! Explicit desktop-entry/X11 identity rules. Exact, ASCII-case-insensitive
//! comparisons only; no regexes, shell commands or persistent X11 IDs.
use crate::{
    assignments, storage,
    x11::{Result, Window},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}
impl Rule {
    fn validate(&self) -> Result<()> {
        let fields = [&self.class, &self.instance, &self.role];
        if fields.iter().all(|field| field.is_none()) {
            return Err("an identity rule needs class, instance and/or role".into());
        }
        for value in fields.into_iter().flatten() {
            if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
                return Err("identity criteria must be nonempty, bounded strings without control characters".into());
            }
        }
        Ok(())
    }
    fn normalized(&self) -> Self {
        Self {
            class: self.class.as_ref().map(|v| v.to_ascii_lowercase()),
            instance: self.instance.as_ref().map(|v| v.to_ascii_lowercase()),
            role: self.role.as_ref().map(|v| v.to_ascii_lowercase()),
        }
    }
    pub fn matches(&self, window: &Window) -> bool {
        let matches = |rule: &Option<String>, actual: &Option<String>| {
            rule.as_ref().is_none_or(|rule| {
                actual
                    .as_ref()
                    .is_some_and(|actual| rule.eq_ignore_ascii_case(actual))
            })
        };
        matches(&self.class, &window.class)
            && matches(&self.instance, &window.instance)
            && matches(&self.role, &window.role)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub version: u32,
    pub rules: BTreeMap<String, Rule>,
}
impl Default for Overrides {
    fn default() -> Self {
        Self {
            version: 1,
            rules: BTreeMap::new(),
        }
    }
}
impl Overrides {
    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("unsupported matching-rules version".into());
        }
        if self.rules.len() > 1024 {
            return Err("too many identity rules".into());
        }
        let mut seen = BTreeSet::new();
        for (id, rule) in &self.rules {
            if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
                return Err("invalid desktop-entry ID in identity rules".into());
            }
            rule.validate()?;
            if !seen.insert(rule.normalized()) {
                return Err(
                    "identical identity criteria cannot belong to multiple applications".into(),
                );
            }
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("matching rules exceed safe size".into());
        }
        let rules: Self = serde_json::from_slice(&bytes)?;
        rules.validate()?;
        Ok(rules)
    }
    pub fn change(&mut self, path: &Path, id: &str, rule: Option<Rule>) -> Result<()> {
        if Self::load(path)? != *self {
            return Err(
                "identity rules changed on disk; restart the resident to reload them before saving"
                    .into(),
            );
        }
        let mut next = self.clone();
        if let Some(rule) = rule {
            next.rules.insert(id.to_owned(), rule);
        } else {
            next.rules.remove(id);
        }
        next.validate()?;
        storage::write_json(path, &next)?;
        *self = next;
        Ok(())
    }
}
pub fn path() -> Result<PathBuf> {
    Ok(assignments::path()?.with_file_name("matching.json"))
}
pub fn parse(arguments: &[&str]) -> Result<Rule> {
    let mut rule = Rule::default();
    let (pairs, remainder) = arguments.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("identity options need a value".into());
    }
    for [flag, value] in pairs {
        let field = match *flag {
            "--class" => &mut rule.class,
            "--instance" => &mut rule.instance,
            "--role" => &mut rule.role,
            _ => return Err(format!("unknown identity option {flag}").into()),
        };
        if field.is_some() {
            return Err(format!("duplicate identity option {flag}").into());
        }
        *field = Some((*value).to_owned());
    }
    rule.validate()?;
    Ok(rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_conjunctive_matching_and_cli_validation() {
        let rule = parse(&[
            "--class",
            "Editor",
            "--instance",
            "project",
            "--role",
            "main",
        ])
        .unwrap();
        let mut window = Window {
            class: Some("EDITOR".into()),
            instance: Some("Project".into()),
            role: Some("Main".into()),
            ..Window::default()
        };
        assert!(rule.matches(&window));
        window.role = None;
        assert!(!rule.matches(&window));
        for args in [
            vec![],
            vec!["--class"],
            vec!["--class", ""],
            vec!["--class", "bad\nclass"],
            vec!["--class", "a", "--class", "b"],
            vec!["--unknown", "value"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn persistent_private_rules_duplicate_rejection_and_transactional_failures() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::bindings::PrivateDir::new("matching").unwrap();
        let path = dir.0.join("matching.json");
        let mut rules = Overrides::load(&path).unwrap();
        rules
            .change(
                &path,
                "a.desktop",
                Some(parse(&["--class", "Editor"]).unwrap()),
            )
            .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            rules
                .change(
                    &path,
                    "b.desktop",
                    Some(parse(&["--class", "EDITOR"]).unwrap())
                )
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!rules.rules.contains_key("b.desktop"));
        fs::write(dir.0.join("blocker"), "not a directory").unwrap();
        assert!(
            rules
                .change(&dir.0.join("blocker/file"), "a.desktop", None)
                .is_err()
        );
        assert!(rules.rules.contains_key("a.desktop"));
        assert_eq!(Overrides::load(&path).unwrap().rules, rules.rules);
        for invalid in [
            "invalid",
            r#"{"version":2,"rules":{}}"#,
            r#"{"version":1,"rules":{"a.desktop":{}}}"#,
            r#"{"version":1,"rules":{"a.desktop":{"class":"Editor","unknown":true}}}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(Overrides::load(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
        fs::write(&path, bytes).unwrap();
        rules.change(&path, "a.desktop", None).unwrap();
        assert!(Overrides::load(&path).unwrap().rules.is_empty());
    }
}
