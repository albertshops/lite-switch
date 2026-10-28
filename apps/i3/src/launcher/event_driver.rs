use super::*;

impl Ui {
    pub(super) fn start_events(self: &Rc<Self>) {
        if self.event_source.borrow().is_some() {
            return;
        }
        let connected = ipc::socket_path().and_then(|path| {
            let stream = events::subscribe(&path)?;
            *self.socket.borrow_mut() = path;
            Ok(stream)
        });
        let mut stream = match connected {
            Ok(stream) => stream,
            Err(e) => {
                self.status.set_text(&format!("Waiting for i3 events: {e}"));
                self.retry_events();
                return;
            }
        };
        let weak = Rc::downgrade(self);
        let mut decoder = events::Decoder::default();
        let source = glib::source::unix_fd_add_local(
            stream.as_raw_fd(),
            glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
            move |_, condition| {
                let Some(ui) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let result = events::read_available(&mut stream, &mut decoder);
                match result {
                    Ok(events)
                        if !condition
                            .intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) =>
                    {
                        for event in events {
                            if event.kind == 6 {
                                ui.previous.borrow_mut().take();
                                ui.book.borrow_mut().clear_session();
                            }
                            if event.kind == 3
                                && event.data["change"] == "close"
                                && let (Some(id), Some(xid)) = (
                                    event.data["container"]["id"].as_u64(),
                                    event.data["container"]["window"].as_u64(),
                                )
                            {
                                ui.book.borrow_mut().close_window(id, xid);
                            }
                            if [0, 1, 3, 6].contains(&event.kind) {
                                let own_focus = event.kind == 3
                                    && event.data["change"] == "focus"
                                    && event.data["container"]["window"].as_u64()
                                        == ui.own_xid.get();
                                if !own_focus {
                                    ui.schedule_refresh();
                                }
                            }
                        }
                        glib::ControlFlow::Continue
                    }
                    _ => {
                        ui.event_source.borrow_mut().take();
                        ui.book.borrow_mut().clear_session();
                        ui.status
                            .set_text("Lost i3 event connection; reconnecting…");
                        ui.retry_events();
                        glib::ControlFlow::Break
                    }
                }
            },
        );
        *self.event_source.borrow_mut() = Some(source);
        self.schedule_refresh();
    }

    fn retry_events(self: &Rc<Self>) {
        if self.retry_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let Some(ui) = weak.upgrade() {
                ui.retry_pending.set(false);
                ui.start_events();
            }
        });
    }

    fn schedule_refresh(self: &Rc<Self>) {
        if !self.window.is_visible() || self.refresh_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(ui) = weak.upgrade() {
                ui.refresh_pending.set(false);
                if !ui.window.is_visible() {
                    return;
                }
                match ipc::request::<Node>(&ui.socket.borrow(), ipc::GET_TREE, "") {
                    Ok(root) => {
                        let live = windows::windows(&root);
                        ui.book.borrow_mut().prune(&live);
                        *ui.windows.borrow_mut() = live
                            .into_iter()
                            .filter(|window| Some(window.x11_id) != ui.own_xid.get())
                            .collect();
                        ui.render();
                    }
                    Err(e) => ui.status.set_text(&format!("Cannot refresh windows: {e}")),
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
