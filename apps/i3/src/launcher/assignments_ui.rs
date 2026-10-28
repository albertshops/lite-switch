use super::*;

impl Ui {
    pub(super) fn capture_key(self: &Rc<Self>, key: gdk::Key) {
        let Some(target) = self.recording.borrow().clone() else {
            return;
        };
        if key == gdk::Key::Escape {
            self.recording.borrow_mut().take();
            self.render();
            return;
        }
        let mut assignment = target.assignment();
        let previous_key = self.book.borrow().key_for(&assignment).cloned();
        if let Some(previous) = previous_key
            .as_ref()
            .and_then(|key| self.book.borrow().targets.get(key).cloned())
        {
            assignment = previous;
        }
        let result = if key == gdk::Key::BackSpace || key == gdk::Key::Delete {
            previous_key.as_ref().map_or(Ok(()), |key| {
                self.book
                    .borrow_mut()
                    .remove_key(&self.assignment_path, key)
            })
        } else {
            let value = match key {
                gdk::Key::Left => "Left".to_owned(),
                gdk::Key::Right => "Right".to_owned(),
                gdk::Key::Up => "Up".to_owned(),
                gdk::Key::Down => "Down".to_owned(),
                _ => key.to_unicode().map(|c| c.to_string()).unwrap_or_default(),
            };
            let Ok(key) = Key::try_from(value) else {
                return;
            };
            self.book
                .borrow_mut()
                .assign(&self.assignment_path, key, assignment)
        };
        self.recording.borrow_mut().take();
        self.render();
        self.status.set_text(&match result {
            Ok(()) => {
                "Assignment saved · global keys require reviewing/installing the exported bindings"
                    .to_owned()
            }
            Err(e) => format!("Cannot save assignment: {e}"),
        });
    }

    fn activate_assignment(&self, key: &Key) -> Result<()> {
        let root: Node = ipc::request(&self.socket.borrow(), ipc::GET_TREE, "")?;
        let live = windows::windows(&root);
        self.book.borrow_mut().prune(&live);
        let target = self
            .book
            .borrow()
            .targets
            .get(key)
            .cloned()
            .ok_or("key is unassigned or its session target has closed")?;
        match target {
            Assignment::Window {
                container_id,
                x11_id,
            } => {
                let window = live
                    .iter()
                    .find(|w| w.id == container_id && w.x11_id == x11_id)
                    .ok_or("assigned window has closed")?;
                self.window.set_visible(false);
                windows::focus_window(&self.socket.borrow(), window)
            }
            Assignment::Application {
                desktop_id,
                launch_if_needed,
            } => {
                *self.applications.borrow_mut() = applications::discover();
                let app = self
                    .applications
                    .borrow()
                    .iter()
                    .find(|app| app.id == desktop_id)
                    .cloned()
                    .ok_or("assigned desktop entry is no longer installed")?;
                self.activate_application(&app, launch_if_needed)
            }
        }
    }

    pub(super) fn assignment_command(self: &Rc<Self>, arguments: &[String]) -> Result<String> {
        let args: Vec<_> = arguments.iter().map(String::as_str).collect();
        let key = |value: &str| {
            Key::try_from(value.to_owned()).map_err(|e| -> Box<dyn std::error::Error> { e.into() })
        };
        match args.as_slice() {
            ["assignments"] => {
                return Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&self.book.borrow().targets)?
                ));
            }
            ["settings"] => {
                return Ok(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&*self.settings.borrow())?
                ));
            }
            ["set-launcher", value] | ["set-modifier", value] => {
                let mut settings = self.settings.borrow().clone();
                if args[0] == "set-launcher" {
                    settings.launcher_shortcut = settings::Shortcut::try_from((*value).to_owned())?;
                } else {
                    settings.assignment_modifier = settings::Modifier::parse(value)?;
                }
                bindings::render(&self.book.borrow(), &settings, &bindings::launcher_path()?)?;
                settings.save(&self.settings_path)?;
                *self.settings.borrow_mut() = settings;
            }
            ["bindings-check"] => {
                return bindings::managed::check(
                    &self.book.borrow(),
                    &self.settings.borrow(),
                    &bindings::launcher_path()?,
                    &self.socket.borrow(),
                );
            }
            ["bindings-status"] => {
                return bindings::managed::status(
                    &self.book.borrow(),
                    &self.settings.borrow(),
                    &bindings::launcher_path()?,
                    &self.socket.borrow(),
                );
            }
            [action] | [action, "--approve-i3-config"]
                if ["bindings-install", "bindings-sync"].contains(action) =>
            {
                return bindings::managed::install(
                    &self.book.borrow(),
                    &self.settings.borrow(),
                    &bindings::launcher_path()?,
                    &self.socket.borrow(),
                    args.len() == 2,
                );
            }
            ["bindings-uninstall"] | ["bindings-uninstall", "--approve-i3-config"] => {
                return bindings::managed::uninstall(&self.socket.borrow(), args.len() == 2);
            }
            ["bindings"] => {
                return bindings::render(
                    &self.book.borrow(),
                    &self.settings.borrow(),
                    &bindings::launcher_path()?,
                );
            }
            ["unassign", value] => self
                .book
                .borrow_mut()
                .remove_key(&self.assignment_path, &key(value)?)?,
            ["activate", value] => {
                self.activate_assignment(&key(value)?)?;
                return Ok(String::new());
            }
            ["launch-if-needed", value, enabled] if ["on", "off"].contains(enabled) => {
                let assignment = self.book.borrow().targets.get(&key(value)?).cloned();
                let Some(Assignment::Application { desktop_id, .. }) = assignment else {
                    return Err("key must be assigned to an application".into());
                };
                self.book.borrow_mut().set_launch(
                    &self.assignment_path,
                    &desktop_id,
                    *enabled == "on",
                )?;
            }
            ["assign-app", value, id] | ["assign-app", value, id, "--launch-if-needed"] => {
                let key = key(value)?;
                *self.applications.borrow_mut() = applications::discover();
                if !self.applications.borrow().iter().any(|app| app.id == *id) {
                    return Err(format!("desktop entry not found: {id}").into());
                }
                let target = Assignment::Application {
                    desktop_id: (*id).to_owned(),
                    launch_if_needed: false,
                };
                let existing = self
                    .book
                    .borrow()
                    .key_for(&target)
                    .and_then(|key| self.book.borrow().targets.get(key).cloned());
                let enabled = args.len() == 4
                    || matches!(
                        existing,
                        Some(Assignment::Application {
                            launch_if_needed: true,
                            ..
                        })
                    );
                self.book.borrow_mut().assign(
                    &self.assignment_path,
                    key,
                    Assignment::Application {
                        desktop_id: (*id).to_owned(),
                        launch_if_needed: enabled,
                    },
                )?;
            }
            ["assign-window", value, id] => {
                let key = key(value)?;
                let id: u64 = id
                    .parse()
                    .map_err(|_| "window container ID must be numeric")?;
                let root: Node = ipc::request(&self.socket.borrow(), ipc::GET_TREE, "")?;
                let target = windows::windows(&root)
                    .into_iter()
                    .find(|w| w.id == id && Some(w.x11_id) != self.own_xid.get())
                    .ok_or("window does not exist or is the launcher")?;
                self.book.borrow_mut().assign(
                    &self.assignment_path,
                    key,
                    Assignment::window(&target),
                )?;
            }
            _ => return Err("invalid assignment command; use --help".into()),
        }
        if self.window.is_visible() {
            self.render();
        }
        Ok("Saved. Global keys require reviewing/installing the exported bindings.\n".to_owned())
    }
}
