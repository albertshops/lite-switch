//! Optimistic property transactions. Xfconf has no cross-client CAS/transaction
//! API: guard every observable step and never overwrite detected outside edits.
use super::{
    plan::{self, Grabs, Plan},
    xfconf::{Snapshot, Store, Value},
};
use crate::x11::Result;
use std::collections::{BTreeMap, BTreeSet};

struct Journal {
    original: Option<Value>,
    expected: Option<Value>,
    uncertain_before: Option<Option<Value>>,
}
fn put(snapshot: &mut Snapshot, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        snapshot.insert(key.to_owned(), value.clone());
    } else {
        snapshot.remove(key);
    }
}
fn step(
    store: &mut impl Store,
    expected: &mut Snapshot,
    journal: &mut BTreeMap<String, Journal>,
    key: &str,
    value: Option<&Value>,
) -> Result<()> {
    if store.snapshot()? != *expected {
        return Err("Xfce shortcuts changed concurrently; refusing to overwrite them".into());
    }
    let before = expected.get(key).cloned();
    if before.as_ref() == value {
        return Ok(());
    }
    let entry = journal.entry(key.to_owned()).or_insert_with(|| Journal {
        original: before.clone(),
        expected: before.clone(),
        uncertain_before: None,
    });
    entry.expected = value.cloned();
    entry.uncertain_before = Some(before);
    store.set(key, value)?;
    entry.uncertain_before = None;
    put(expected, key, value);
    if store.snapshot()? != *expected {
        return Err(format!(
            "shortcut state changed or did not apply at {key}; refusing further writes"
        )
        .into());
    }
    Ok(())
}

pub(super) fn preflight(store: &mut impl Store, plan: &Plan) -> Result<BTreeSet<String>> {
    if store.snapshot()? != plan.before {
        return Err("shortcut state changed after planning; no changes applied".into());
    }
    let properties: BTreeSet<_> = plan
        .before
        .keys()
        .chain(plan.after.keys())
        .filter(|p| plan.before.get(*p) != plan.after.get(*p))
        .cloned()
        .chain(plan.release.iter().cloned())
        .collect();
    for property in &properties {
        if store.locked(property)? {
            return Err(format!("shortcut property is policy-locked: {property}").into());
        }
    }
    Ok(properties)
}

pub(super) fn execute(
    store: &mut impl Store,
    plan: &Plan,
    mut verify: impl FnMut(bool, &Grabs) -> Result<()>,
    mut finalize: impl FnMut() -> Result<()>,
) -> Result<()> {
    let properties = preflight(store, plan)?;
    let mut expected = plan.before.clone();
    let mut journal = BTreeMap::new();
    let result = (|| -> Result<()> {
        // Reset, rather than setting an empty command: native providers need a
        // shortcut-removed signal to release their existing passive grabs.
        for property in &plan.release {
            step(store, &mut expected, &mut journal, property, None)?;
        }
        verify(false, &plan.free)?;
        if store.snapshot()? != expected {
            return Err("shortcut state changed while releasing grabs".into());
        }
        // Restore metadata before adding commands, matching native provider order.
        for bindings in [false, true] {
            for property in &properties {
                if plan::binding(property).is_some() != bindings {
                    continue;
                }
                step(
                    store,
                    &mut expected,
                    &mut journal,
                    property,
                    plan.after.get(property),
                )?;
            }
        }
        if expected != plan.after {
            return Err("incomplete shortcut transaction plan".into());
        }
        verify(true, &plan.claimed)?;
        if store.snapshot()? != expected {
            return Err("shortcut state changed during grab verification".into());
        }
        finalize()?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let rollback = rollback(store, &journal, plan, &mut verify);
            match rollback {
                Ok(()) => Err(format!("{error}; prior shortcut properties restored").into()),
                Err(restore_error) => Err(format!(
                    "{error}; rollback could not safely finish: {restore_error}"
                )
                .into()),
            }
        }
    }
}

fn rollback(
    store: &mut impl Store,
    journal: &BTreeMap<String, Journal>,
    plan: &Plan,
    verify: &mut impl FnMut(bool, &Grabs) -> Result<()>,
) -> Result<()> {
    let current = store.snapshot()?;
    let mut safe = Vec::new();
    let mut errors = Vec::new();
    for (property, entry) in journal {
        let value = current.get(property);
        if value == entry.original.as_ref() {
            continue;
        }
        if value == entry.expected.as_ref()
            || entry
                .uncertain_before
                .as_ref()
                .is_some_and(|before| value == before.as_ref())
        {
            safe.push((property, entry, value.cloned()));
        } else {
            errors.push(format!("outside edit preserved at {property}"));
        }
    }
    let mut free = Grabs::new();
    let mut claimed = Grabs::new();
    for (property, entry, _) in &safe {
        if let Some((all, required)) = plan.grabs.get(*property) {
            free.extend(all);
            if entry
                .original
                .as_ref()
                .and_then(Value::as_string)
                .is_some_and(|value| !value.is_empty())
            {
                claimed.extend(required);
            }
        }
    }
    // Best effort: restore all safe properties even if another owned property
    // was externally edited. Never reset an entire channel or provider tree.
    let mut released = BTreeSet::new();
    for (property, _, observed) in &safe {
        if plan::binding(property).is_some()
            && observed
                .as_ref()
                .and_then(Value::as_string)
                .is_some_and(|value| !value.is_empty())
        {
            if store.snapshot()?.get(*property) != observed.as_ref() {
                errors.push(format!("outside edit preserved at {property}"));
                continue;
            }
            match store.set(property, None) {
                Ok(()) => {
                    released.insert(*property);
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
    }
    // Verification failure must not prevent restoring recoverable configuration
    // values (e.g. keyboard/WM changes). It is still reported, not called success.
    if !free.is_empty()
        && let Err(error) = verify(false, &free)
    {
        errors.push(error.to_string());
    }
    for bindings in [false, true] {
        for (property, entry, observed) in &safe {
            if plan::binding(property).is_some() != bindings {
                continue;
            }
            let expected = if released.contains(property) {
                None
            } else {
                observed.as_ref()
            };
            if store.snapshot()?.get(*property) != expected {
                errors.push(format!("outside edit preserved at {property}"));
                continue;
            }
            if expected == entry.original.as_ref() {
                continue;
            }
            if let Err(error) = store.set(property, entry.original.as_ref()) {
                errors.push(error.to_string());
            }
        }
    }
    if !claimed.is_empty()
        && let Err(error) = verify(true, &claimed)
    {
        errors.push(error.to_string());
    }
    let after = store.snapshot()?;
    for (property, entry, _) in safe {
        if after.get(property) != entry.original.as_ref() {
            errors.push(format!("could not restore {property}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; ").into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan::Manifest;
    use super::*;
    struct Fake {
        state: Snapshot,
        count: usize,
        failure: Option<usize>,
        outside: bool,
    }
    impl Store for Fake {
        fn snapshot(&mut self) -> Result<Snapshot> {
            Ok(self.state.clone())
        }
        fn set(&mut self, key: &str, value: Option<&Value>) -> Result<()> {
            self.count += 1;
            if self.failure == Some(self.count) {
                return Err("injected write failure".into());
            }
            put(&mut self.state, key, value);
            if self.outside {
                self.outside = false;
                self.state
                    .insert(key.into(), Value::string("external edit"));
            }
            Ok(())
        }
    }
    fn plan() -> Plan {
        let key = "/commands/custom/<Alt>space".to_owned();
        Plan {
            before: Snapshot::from([(key.clone(), Value::string("old"))]),
            after: Snapshot::from([(key.clone(), Value::string("new"))]),
            manifest: Manifest {
                version: 1,
                entries: vec![],
                shortcuts: vec![],
            },
            displaced: vec![],
            release: vec![key],
            free: Grabs::new(),
            claimed: Grabs::new(),
            grabs: BTreeMap::new(),
        }
    }
    #[test]
    fn partial_write_and_finalize_failures_restore_properties() {
        let plan = plan();
        let mut fake = Fake {
            state: plan.before.clone(),
            count: 0,
            failure: Some(2),
            outside: false,
        };
        assert!(execute(&mut fake, &plan, |_, _| Ok(()), || Ok(())).is_err());
        assert_eq!(fake.state, plan.before);
        fake.failure = None;
        assert!(
            execute(
                &mut fake,
                &plan,
                |_, _| Ok(()),
                || Err("manifest failure".into())
            )
            .is_err()
        );
        assert_eq!(fake.state, plan.before);
    }
    #[test]
    fn grab_verification_failure_rolls_back_and_never_finalizes() {
        let plan = plan();
        let mut fake = Fake {
            state: plan.before.clone(),
            count: 0,
            failure: None,
            outside: false,
        };
        let error = execute(
            &mut fake,
            &plan,
            |claimed, _| {
                if claimed {
                    Err("grab verification failed".into())
                } else {
                    Ok(())
                }
            },
            || panic!("must not finalize"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("prior shortcut properties restored"),
            "{error}"
        );
        assert_eq!(fake.state, plan.before);
    }
    #[test]
    fn policy_locked_properties_fail_before_writing_or_probing() {
        struct Locked(Snapshot);
        impl Store for Locked {
            fn snapshot(&mut self) -> Result<Snapshot> {
                Ok(self.0.clone())
            }
            fn set(&mut self, _: &str, _: Option<&Value>) -> Result<()> {
                panic!("must not write")
            }
            fn locked(&self, _: &str) -> Result<bool> {
                Ok(true)
            }
        }
        let plan = plan();
        let mut store = Locked(plan.before.clone());
        let error = execute(
            &mut store,
            &plan,
            |_, _| panic!("must not probe"),
            || panic!("must not finalize"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("policy-locked"));
        assert_eq!(store.0, plan.before);
    }
    #[test]
    fn detected_outside_edits_are_not_overwritten_by_rollback() {
        let plan = plan();
        let mut fake = Fake {
            state: plan.before.clone(),
            count: 0,
            failure: None,
            outside: true,
        };
        let error = execute(&mut fake, &plan, |_, _| Ok(()), || Ok(()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside edit preserved"), "{error}");
        assert_eq!(
            fake.state.values().next().unwrap(),
            &Value::string("external edit")
        );
    }
}
