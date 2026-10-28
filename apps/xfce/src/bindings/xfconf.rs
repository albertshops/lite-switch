//! Typed Xfconf access. All writes target the same unique service owner.
use crate::x11::Result;
use gtk::{gio, glib, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const CHANNEL: &str = "xfce4-keyboard-shortcuts";
pub(super) type Snapshot = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Value {
    signature: String,
    text: String,
}
impl Value {
    pub fn from_variant(value: &glib::Variant) -> Self {
        Self {
            signature: value.type_().as_str().to_owned(),
            text: value.print(true).to_string(),
        }
    }
    pub fn variant(&self) -> Result<glib::Variant> {
        if self.text.len() > 1024 * 1024 || self.signature.len() > 256 {
            return Err("saved Xfconf value is too large".into());
        }
        let kind = glib::VariantTy::new(&self.signature)?;
        Ok(glib::Variant::parse(Some(kind), &self.text)?)
    }
    pub fn string(value: &str) -> Self {
        Self::from_variant(&value.to_variant())
    }
    pub fn boolean(value: bool) -> Self {
        Self::from_variant(&value.to_variant())
    }
    pub fn as_string(&self) -> Option<String> {
        self.variant().ok()?.get()
    }
}

pub(super) trait Store {
    fn snapshot(&mut self) -> Result<Snapshot>;
    fn set(&mut self, property: &str, value: Option<&Value>) -> Result<()>;
    fn locked(&self, _property: &str) -> Result<bool> {
        Ok(false)
    }
}

pub(super) struct Xfconf {
    bus: gio::DBusConnection,
    owner: String,
}
impl Xfconf {
    pub fn connect() -> Result<Self> {
        let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
        let owner = Self::owner(&bus)?;
        Ok(Self { bus, owner })
    }
    fn owner(bus: &gio::DBusConnection) -> Result<String> {
        let reply = bus
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetNameOwner",
                Some(&("org.xfce.Xfconf",).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                2000,
                None::<&gio::Cancellable>,
            )
            .map_err(|e| format!("Xfconf service is not running: {e}"))?;
        Ok(reply
            .get::<(String,)>()
            .ok_or("invalid Xfconf service-owner reply")?
            .0)
    }
    fn call(&self, method: &str, arguments: &glib::Variant) -> Result<glib::Variant> {
        if Self::owner(&self.bus)? != self.owner {
            return Err(
                "Xfconf service restarted; refusing to write through lost continuity".into(),
            );
        }
        Ok(self.bus.call_sync(
            Some(&self.owner),
            "/org/xfce/Xfconf",
            "org.xfce.Xfconf",
            method,
            Some(arguments),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            2000,
            None::<&gio::Cancellable>,
        )?)
    }
}
impl Store for Xfconf {
    fn snapshot(&mut self) -> Result<Snapshot> {
        let reply = self.call("GetAllProperties", &(CHANNEL, "/").to_variant())?;
        let values = reply
            .get::<(BTreeMap<String, glib::Variant>,)>()
            .ok_or("invalid Xfconf properties reply")?
            .0;
        if values.len() > 4096 {
            return Err("too many Xfconf shortcut properties".into());
        }
        let snapshot: Snapshot = values
            .into_iter()
            .map(|(key, value)| (key, Value::from_variant(&value)))
            .collect();
        if serde_json::to_vec(&snapshot)?.len() > 8 * 1024 * 1024 {
            return Err("Xfconf shortcut snapshot exceeds safe size".into());
        }
        Ok(snapshot)
    }
    fn set(&mut self, property: &str, value: Option<&Value>) -> Result<()> {
        if let Some(value) = value {
            self.call(
                "SetProperty",
                &(CHANNEL, property, value.variant()?).to_variant(),
            )?;
        } else {
            let exists = self
                .call("PropertyExists", &(CHANNEL, property).to_variant())?
                .get::<(bool,)>()
                .ok_or("invalid Xfconf existence reply")?
                .0;
            if !exists {
                return Ok(());
            }
            // Do not remove descendants or reset a provider's default tree.
            self.call("ResetProperty", &(CHANNEL, property, false).to_variant())?;
        }
        Ok(())
    }
    fn locked(&self, property: &str) -> Result<bool> {
        let reply = self.call("IsPropertyLocked", &(CHANNEL, property).to_variant())?;
        Ok(reply
            .get::<(bool,)>()
            .ok_or("invalid Xfconf policy-lock reply")?
            .0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_variants_preserve_types_and_quoted_values() {
        for variant in [
            "a 'quoted' \\ command\n".to_variant(),
            false.to_variant(),
            42u32.to_variant(),
            vec!["a", "b"].to_variant(),
        ] {
            let value = Value::from_variant(&variant);
            let bytes = serde_json::to_vec(&value).unwrap();
            let restored: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(restored.variant().unwrap(), variant);
        }
        let invalid = Value {
            signature: "b".into(),
            text: "'not a bool'".into(),
        };
        assert!(invalid.variant().is_err());
    }
}
