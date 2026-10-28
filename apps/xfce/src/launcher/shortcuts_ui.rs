use super::*;

impl Ui {
    pub(super) fn sync_shortcuts(&self) -> Result<String> {
        let result: Result<String> = (|| {
            if self.settings.borrow().auto_sync_shortcuts {
                bindings::managed::sync_existing(&self.book.borrow(), &self.settings.borrow(), &bindings::launcher_path()?)
                    .map(|report| report.unwrap_or_else(|| "Preferences saved; shortcuts are not installed. Approve bindings-install to enable global keys.\n".into()))
            } else {
                Ok("Preferences saved; automatic shortcut sync is off. Review bindings-check, then approve bindings-sync.\n".into())
            }
        })();
        let message = match &result {
            Ok(report) => report.clone(),
            Err(error) => format!("Preferences saved, but shortcut sync FAILED: {error}"),
        };
        if result.is_err() {
            eprintln!("{message}");
        }
        let summary = match &result {
            Ok(report) if report.starts_with("Managed Xfce") => format!(
                "Global shortcuts synced · {}",
                report
                    .lines()
                    .find(|line| line.contains("binding(s) displaced"))
                    .unwrap_or("up to date")
            ),
            _ => message.lines().next().unwrap_or(&message).to_owned(),
        };
        self.status.set_tooltip_text(Some(&message));
        *self.shortcut_notice.borrow_mut() = Some(summary.clone());
        self.status.set_text(&summary);
        result
            .map_err(|error| format!("Preferences saved, but shortcut sync failed: {error}").into())
    }

    pub(super) fn prune_assignments(&self, live: &[Window]) {
        let before = self.book.borrow().targets.clone();
        self.book.borrow_mut().prune(live);
        let changed = self.book.borrow().targets != before;
        if changed && self.settings.borrow().auto_sync_shortcuts {
            let _ = self.sync_shortcuts();
        }
    }
}
