use super::*;

impl Ui {
    pub(super) fn start_events(self: &Rc<Self>) {
        if self.event_source.borrow().is_some() {
            return;
        }
        let Some(fd) = self.desktop.borrow().as_ref().map(Desktop::event_fd) else {
            self.retry_events();
            return;
        };
        let weak = Rc::downgrade(self);
        let source = glib::source::unix_fd_add_local(
            fd,
            glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
            move |_, condition| {
                let Some(ui) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let result = ui
                    .desktop
                    .borrow_mut()
                    .as_mut()
                    .map(Desktop::process_events)
                    .unwrap_or_else(|| Err("X11 connection lost".into()));
                if result.is_err()
                    || condition.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR)
                {
                    ui.event_source.borrow_mut().take();
                    ui.connection_lost();
                    glib::ControlFlow::Break
                } else {
                    if matches!(result, Ok(true)) {
                        ui.schedule_refresh();
                    }
                    glib::ControlFlow::Continue
                }
            },
        );
        *self.event_source.borrow_mut() = Some(source);
        self.schedule_refresh();
    }

    fn connection_lost(self: &Rc<Self>) {
        self.desktop.borrow_mut().take();
        self.prune_assignments(&[]);
        self.previous.borrow_mut().take();
        self.windows.borrow_mut().clear();
        self.status
            .set_text("Lost Xfce event continuity; session targets cleared. Reconnecting…");
        self.retry_events();
    }

    fn retry_events(self: &Rc<Self>) {
        if self.retry_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.retry_pending.set(false);
            if ui.desktop.borrow().is_none() {
                match Desktop::connect() {
                    Ok(desktop) => *ui.desktop.borrow_mut() = Some(desktop),
                    Err(_) => {
                        ui.retry_events();
                        return;
                    }
                }
            }
            ui.start_events();
        });
    }

    pub(super) fn schedule_refresh(self: &Rc<Self>) {
        if self.refresh_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.refresh_pending.set(false);
            match ui.live_windows() {
                Ok(live) => {
                    let own_focus = live
                        .iter()
                        .any(|w| Some(w.x11_id) == ui.own_xid.get() && w.focused);
                    if ui.window.is_visible() {
                        if own_focus {
                            ui.was_active.set(true);
                        } else if ui.was_active.get() {
                            ui.dismiss(false);
                        }
                    }
                    ui.prune_assignments(&live);
                    let stale = ui.previous.borrow().as_ref().is_some_and(|previous| {
                        !live
                            .iter()
                            .any(|w| w.x11_id == previous.x11_id && w.identity == previous.identity)
                    });
                    if stale {
                        ui.previous.borrow_mut().take();
                    }
                    let filtered: Vec<_> = live
                        .into_iter()
                        .filter(|w| Some(w.x11_id) != ui.own_xid.get())
                        .collect();
                    let changed = *ui.windows.borrow() != filtered;
                    *ui.windows.borrow_mut() = filtered;
                    // Never tear down a pressed GTK button because of our own
                    // focus/user-time/geometry events when targets did not change.
                    if changed && ui.window.is_visible() {
                        ui.render();
                    }
                    // Property/destruction events read during a synchronous reply
                    // are buffered by x11rb and might not leave the FD readable.
                    let pending = ui
                        .desktop
                        .borrow_mut()
                        .as_mut()
                        .is_some_and(Desktop::take_pending_refresh);
                    if pending {
                        ui.schedule_refresh();
                    }
                }
                Err(_) => {
                    if let Some(source) = ui.event_source.borrow_mut().take() {
                        source.remove();
                    }
                    ui.connection_lost();
                }
            }
        });
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        if let Some(source) = self.event_source.get_mut().take() {
            source.remove();
        }
    }
}
