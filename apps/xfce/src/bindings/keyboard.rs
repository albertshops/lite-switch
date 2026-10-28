//! Passive grab checks only during approved transactions; never synthesize keys.
use super::plan::Grabs;
use crate::x11::{Desktop, Result};
use gtk::{gdk, glib::translate::IntoGlib, prelude::*};
use std::{
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{
        ErrorKind,
        xkb::{ConnectionExt as _, ID},
        xproto::{ConnectionExt as _, GrabMode, ModMask},
    },
    rust_connection::RustConnection,
};

#[derive(PartialEq, Eq)]
struct Layout {
    symbols: Vec<u32>,
    modifiers: Vec<u8>,
    group: u8,
    width: u8,
}

pub(super) struct Keyboard {
    connection: RustConnection,
    root: u32,
    guard: Desktop,
    display: gdk::Display,
    layout: Layout,
    min: u8,
    count: u8,
}
impl Keyboard {
    pub fn connect() -> Result<Self> {
        let guard = Desktop::connect()?;
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        let min = connection.setup().min_keycode;
        let count = (u16::from(connection.setup().max_keycode) - u16::from(min) + 1).try_into()?;
        if !connection.xkb_use_extension(1, 0)?.reply()?.supported {
            return Err("XKB is required to validate Xfce shortcuts".into());
        }
        let display = gdk::Display::default().ok_or("X11 keymap is not available")?;
        let layout = Self::layout(&connection, min, count)?;
        Ok(Self {
            connection,
            root,
            guard,
            display,
            layout,
            min,
            count,
        })
    }
    fn layout(connection: &RustConnection, min: u8, count: u8) -> Result<Layout> {
        let keys = connection.get_keyboard_mapping(min, count)?.reply()?;
        let modifiers = connection.get_modifier_mapping()?.reply()?.keycodes;
        let state = connection.xkb_get_state(ID::USE_CORE_KBD.into())?.reply()?;
        Ok(Layout {
            symbols: keys.keysyms,
            modifiers,
            group: state.group.into(),
            width: keys.keysyms_per_keycode,
        })
    }
    pub fn unchanged(&self) -> Result<()> {
        self.guard.outputs()?;
        if Self::layout(&self.connection, self.min, self.count)? != self.layout {
            return Err(
                "keyboard layout/group changed during shortcut transaction; retry after it settles"
                    .into(),
            );
        }
        Ok(())
    }
    fn modifier(&self, keys: &[gdk::Key]) -> Result<u16> {
        let codes: Vec<_> = keys
            .iter()
            .flat_map(|key| self.display.map_keyval(*key).unwrap_or_default())
            .map(|mapping| mapping.keycode())
            .collect();
        let per = self.layout.modifiers.len() / 8;
        if per == 0 {
            return Err("empty X11 modifier mapping".into());
        }
        let mut mask = 0u16;
        for (index, codes_in_modifier) in self.layout.modifiers.chunks(per).enumerate() {
            if codes_in_modifier
                .iter()
                .any(|code| *code != 0 && codes.contains(&u32::from(*code)))
            {
                mask |= 1 << index;
            }
        }
        if mask.count_ones() != 1 {
            return Err("virtual modifier is unavailable or ambiguously mapped".into());
        }
        Ok(mask)
    }
    pub fn resolve(&self, accelerator: &str) -> Result<Grabs> {
        self.resolve_inner(accelerator, true)
    }
    pub fn required(&self, accelerator: &str) -> Result<Grabs> {
        self.resolve_inner(accelerator, false)
    }
    fn resolve_inner(&self, accelerator: &str, ignored_locks: bool) -> Result<Grabs> {
        let (key, modifiers) = gtk::accelerator_parse(accelerator)
            .ok_or_else(|| format!("invalid Xfce accelerator {accelerator}"))?;
        let raw = modifiers.bits();
        let virtuals = gdk::ModifierType::SUPER_MASK.bits()
            | gdk::ModifierType::META_MASK.bits()
            | gdk::ModifierType::HYPER_MASK.bits();
        if raw & !(0xff | virtuals) != 0 {
            return Err("unsupported Xfce accelerator modifier".into());
        }
        let mut mask = (raw & 0xff) as u16;
        for (flag, keys) in [
            (
                gdk::ModifierType::SUPER_MASK,
                [gdk::Key::Super_L, gdk::Key::Super_R],
            ),
            (
                gdk::ModifierType::META_MASK,
                [gdk::Key::Meta_L, gdk::Key::Meta_R],
            ),
            (
                gdk::ModifierType::HYPER_MASK,
                [gdk::Key::Hyper_L, gdk::Key::Hyper_R],
            ),
        ] {
            if modifiers.contains(flag) {
                mask |= self.modifier(&keys)?;
            }
        }
        let mappings = self.display.map_keyval(key).unwrap_or_default();
        let group0_only = mappings.iter().all(|mapping| mapping.group() == 0);
        let numlock = self.modifier(&[gdk::Key::Num_Lock]).unwrap_or(0);
        let mut result = Grabs::new();
        for mapping in mappings {
            if !group0_only && mapping.group() != i32::from(self.layout.group) {
                continue;
            }
            let code: u8 = mapping.keycode().try_into()?;
            let offset = usize::from(
                code.checked_sub(self.min)
                    .ok_or("keycode outside X11 range")?,
            ) * usize::from(self.layout.width);
            let symbols = self
                .layout
                .symbols
                .get(offset..offset + usize::from(self.layout.width))
                .ok_or("keycode outside keymap")?;
            if !symbols.contains(&key.into_glib()) {
                return Err("GDK/X11 keymap is changing; retry after it settles".into());
            }
            if !ignored_locks {
                result.insert((code, mask));
                continue;
            }
            // xfsettingsd ignores CapsLock, NumLock, Mod2 and Mod5. xfwm4 has
            // its own grab implementation; only base masks are required there.
            for lock in [0, 2] {
                for num in [0, numlock] {
                    for mod2 in [0, 16] {
                        for mod5 in [0, 128] {
                            result.insert((code, mask | lock | num | mod2 | mod5));
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    fn busy(&self, code: u8, mask: u16) -> Result<bool> {
        match self
            .connection
            .grab_key(
                false,
                self.root,
                ModMask::from(mask),
                code,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check()
        {
            Ok(()) => {
                self.connection
                    .ungrab_key(code, self.root, ModMask::from(mask))?
                    .check()?;
                // If a user pressed during the short probe, end only this
                // connection's active grab. Never affects another client's lock.
                self.connection
                    .ungrab_keyboard(x11rb::CURRENT_TIME)?
                    .check()?;
                Ok(false)
            }
            Err(ReplyError::X11Error(error)) if error.error_kind == ErrorKind::Access => Ok(true),
            Err(error) => Err(error.into()),
        }
    }
    pub fn wait(&self, grabs: &Grabs, claimed: bool) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        // Property replies precede native providers handling their signals. Do
        // not race their first XGrabKey with our temporary availability probe.
        if claimed {
            thread::sleep(Duration::from_millis(120));
        }
        loop {
            self.unchanged()?;
            let mut matched = true;
            for (code, mask) in grabs {
                if self.busy(*code, *mask)? != claimed {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(if claimed {
                    "expected configured shortcut grabs were not observed"
                } else {
                    "shortcut is still grabbed outside the released Xfce properties"
                }
                .into());
            }
            thread::sleep(Duration::from_millis(if claimed { 120 } else { 20 }));
        }
    }
}
