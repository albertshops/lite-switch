//! Pure before/after planning. Existing custom providers are preserved outside
//! the explicitly owned/displaced shortcut properties.
use super::{
    Shortcut,
    xfconf::{Snapshot, Value},
};
use crate::x11::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Grabs = BTreeSet<(u8, u16)>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub property: String,
    pub original: Option<Value>,
    pub installed: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub version: u32,
    pub entries: Vec<Entry>,
    pub shortcuts: Vec<Shortcut>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct Displaced {
    pub property: String,
    pub shortcut: String,
    pub command: String,
}
pub(super) struct Plan {
    pub before: Snapshot,
    pub after: Snapshot,
    pub manifest: Manifest,
    pub displaced: Vec<Displaced>,
    pub release: Vec<String>,
    pub free: Grabs,
    pub claimed: Grabs,
    pub grabs: BTreeMap<String, (Grabs, Grabs)>,
}

pub(super) fn binding(property: &str) -> Option<(&str, &str)> {
    let (provider, shortcut) = property.strip_prefix('/')?.split_once("/custom/")?;
    if provider.is_empty()
        || provider.contains('/')
        || shortcut.is_empty()
        || shortcut.contains('/')
        || shortcut == "override"
    {
        return None;
    }
    Some((provider, shortcut))
}
fn safe_property(property: &str) -> bool {
    binding(property.strip_suffix("/startup-notify").unwrap_or(property)).is_some_and(
        |(provider, chord)| {
            provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                && chord.len() <= 256
                && !chord.contains(['\0', '\n', '\r'])
        },
    )
}
fn put(snapshot: &mut Snapshot, property: &str, value: Option<&Value>) {
    if let Some(value) = value {
        snapshot.insert(property.to_owned(), value.clone());
    } else {
        snapshot.remove(property);
    }
}

pub(super) fn restored(current: &Snapshot, previous: Option<&Manifest>) -> Result<Snapshot> {
    let mut baseline = current.clone();
    if let Some(previous) = previous {
        if previous.version != 1 {
            return Err("unsupported Xfce binding manifest version".into());
        }
        let mut properties = BTreeSet::new();
        for entry in &previous.entries {
            if !safe_property(&entry.property) || !properties.insert(&entry.property) {
                return Err("invalid/duplicate owned shortcut property in manifest".into());
            }
            if current.get(&entry.property) != entry.installed.as_ref() {
                return Err(format!(
                    "owned shortcut property was edited: {}; refusing to overwrite it",
                    entry.property
                )
                .into());
            }
            for value in entry.original.iter().chain(entry.installed.iter()) {
                value.variant()?;
            }
            put(&mut baseline, &entry.property, entry.original.as_ref());
        }
    }
    Ok(baseline)
}

pub(super) fn build(
    current: Snapshot,
    previous: Option<&Manifest>,
    shortcuts: Vec<Shortcut>,
    resolve: impl Fn(&str) -> Result<Grabs>,
    required: impl Fn(&str) -> Result<Grabs>,
) -> Result<Plan> {
    let baseline = restored(&current, previous)?;
    let value = baseline
        .get("/providers")
        .ok_or("Xfce shortcut providers are not initialized")?
        .variant()?;
    // Xfconf arrays commonly arrive as av (GValue arrays), not homogeneous as.
    let providers = value
        .get::<Vec<String>>()
        .or_else(|| {
            value
                .get::<Vec<gtk::glib::Variant>>()?
                .into_iter()
                .map(|v| v.get::<String>())
                .collect()
        })
        .ok_or("invalid Xfce shortcut provider list")?;
    if !providers.iter().any(|p| p == "commands") || !providers.iter().any(|p| p == "xfwm4") {
        return Err("start xfwm4 and xfsettingsd before managing shortcuts".into());
    }
    for provider in &providers {
        if provider.is_empty()
            || !provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("invalid Xfce provider name".into());
        }
        if baseline.get(&format!("/{provider}/custom/override")) != Some(&Value::boolean(true)) {
            return Err(format!(
                "Xfce provider {provider} has not initialized custom shortcuts; no changes made"
            )
            .into());
        }
    }
    if let Some(previous) = previous {
        for entry in &previous.entries {
            let parent = entry
                .property
                .strip_suffix("/startup-notify")
                .unwrap_or(&entry.property);
            let (provider, chord) = binding(parent).ok_or("invalid owned shortcut property")?;
            if !providers.iter().any(|p| p == provider) {
                return Err("owned shortcut provider is no longer registered; inspect the recovery snapshot".into());
            }
            resolve(chord)?;
        }
    }
    let mut desired = Vec::new();
    let mut seen = Grabs::new();
    for shortcut in &shortcuts {
        if shortcut.shortcut.contains('/') || shortcut.shortcut.is_empty() {
            return Err("unsafe shortcut property name".into());
        }
        let grabs = resolve(&shortcut.shortcut)?;
        if grabs.is_empty() || !grabs.is_disjoint(&seen) {
            return Err("proposed shortcuts are unavailable or overlap physically".into());
        }
        seen.extend(&grabs);
        desired.push((shortcut, grabs));
    }
    let mut after = baseline.clone();
    let mut properties = BTreeSet::new();
    let mut displaced = Vec::new();
    for (property, value) in &baseline {
        let Some((provider, chord)) = binding(property) else {
            continue;
        };
        if !providers.iter().any(|p| p == provider) {
            continue;
        }
        let Some(command) = value.as_string().filter(|value| !value.is_empty()) else {
            continue;
        };
        let grabs =
            resolve(chord).map_err(|e| format!("cannot analyze Xfce shortcut {property}: {e}"))?;
        if desired
            .iter()
            .any(|(_, target)| !target.is_disjoint(&grabs))
        {
            after.remove(property);
            properties.insert(property.clone());
            if !shortcuts.iter().any(|s| {
                property == &format!("/commands/custom/{}", s.shortcut) && command == s.command
            }) {
                displaced.push(Displaced {
                    property: property.clone(),
                    shortcut: chord.to_owned(),
                    command,
                });
            }
        }
    }
    for shortcut in &shortcuts {
        let property = format!("/commands/custom/{}", shortcut.shortcut);
        let notify = format!("{property}/startup-notify");
        after.insert(property.clone(), Value::string(&shortcut.command));
        after.insert(notify.clone(), Value::boolean(false));
        properties.extend([property, notify]);
    }
    let entries = properties
        .iter()
        .map(|property| Entry {
            property: property.clone(),
            original: baseline.get(property).cloned(),
            installed: after.get(property).cloned(),
        })
        .collect();
    let manifest = Manifest {
        version: 1,
        entries,
        shortcuts,
    };
    if let Some(previous) = previous {
        properties.extend(previous.entries.iter().map(|entry| entry.property.clone()));
    }
    let mut release = Vec::new();
    let mut free = Grabs::new();
    let mut claimed = Grabs::new();
    let mut by_property = BTreeMap::new();
    for property in properties {
        if let Some((provider, chord)) = binding(&property) {
            let original = current
                .get(&property)
                .and_then(Value::as_string)
                .is_some_and(|value| !value.is_empty());
            let installed = after
                .get(&property)
                .and_then(Value::as_string)
                .is_some_and(|value| !value.is_empty());
            if original {
                release.push(property.clone());
            }
            if original || installed {
                let all = resolve(chord)?;
                let expected = if provider == "commands" {
                    all.clone()
                } else {
                    required(chord)?
                };
                free.extend(&all);
                if installed {
                    claimed.extend(&expected);
                }
                by_property.insert(property, (all, expected));
            }
        }
    }
    Ok(Plan {
        before: current,
        after,
        manifest,
        displaced,
        release,
        free,
        claimed,
        grabs: by_property,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::*;
    fn initial() -> Snapshot {
        Snapshot::from([
            (
                "/providers".into(),
                Value::from_variant(
                    &vec!["commands".to_variant(), "xfwm4".to_variant()].to_variant(),
                ),
            ),
            ("/commands/custom/override".into(), Value::boolean(true)),
            ("/xfwm4/custom/override".into(), Value::boolean(true)),
            (
                "/xfwm4/custom/<Alt>space".into(),
                Value::string("popup_menu_key"),
            ),
            (
                "/commands/custom/<Control><Alt>b".into(),
                Value::string("old command"),
            ),
            (
                "/commands/custom/<Control><Alt>b/startup-notify".into(),
                Value::boolean(true),
            ),
            (
                "/xfwm4/custom/<Alt>F4".into(),
                Value::string("close_window_key"),
            ),
        ])
    }
    fn resolve(chord: &str) -> Result<Grabs> {
        Ok(match chord {
            "<Alt>space" => [(65, 8)].into(),
            "<Primary><Alt>b" | "<Control><Alt>b" => [(56, 12)].into(),
            "<Alt>F4" => [(70, 8)].into(),
            _ => return Err("unknown test chord".into()),
        })
    }
    #[test]
    fn priority_restoration_aliases_metadata_and_unrelated_keys() {
        let initial = initial();
        let shortcuts = vec![
            Shortcut {
                shortcut: "<Alt>space".into(),
                command: "launcher toggle".into(),
            },
            Shortcut {
                shortcut: "<Primary><Alt>b".into(),
                command: "launcher activate b".into(),
            },
        ];
        let plan = build(initial.clone(), None, shortcuts.clone(), resolve, resolve).unwrap();
        assert_eq!(plan.displaced.len(), 2);
        assert!(!plan.after.contains_key("/xfwm4/custom/<Alt>space"));
        assert_eq!(
            plan.after["/commands/custom/<Control><Alt>b/startup-notify"],
            Value::boolean(true)
        );
        assert_eq!(
            plan.after["/xfwm4/custom/<Alt>F4"],
            initial["/xfwm4/custom/<Alt>F4"]
        );
        assert_eq!(
            restored(&plan.after, Some(&plan.manifest)).unwrap(),
            initial
        );
        let again = build(
            plan.after.clone(),
            Some(&plan.manifest),
            shortcuts,
            resolve,
            resolve,
        )
        .unwrap();
        assert_eq!(again.after, plan.after);
        assert_eq!(again.manifest, plan.manifest);
        let removed = build(
            plan.after.clone(),
            Some(&plan.manifest),
            vec![],
            resolve,
            resolve,
        )
        .unwrap();
        assert_eq!(removed.after, initial);
        let mut edited = plan.after;
        edited.insert(
            "/commands/custom/<Alt>space".into(),
            Value::string("user edit"),
        );
        assert!(restored(&edited, Some(&plan.manifest)).is_err());
    }
}
