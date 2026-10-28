mod assignments_ui;
mod event_driver;

use crate::{
    applications::{self, Application},
    assignments::{self, Assignment, Book, Key},
    bindings, events,
    history::{self, History},
    ipc::{self, Result},
    settings::{self, Settings},
    switching,
    windows::{self, Node, Window},
};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    os::fd::AsRawFd,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

#[derive(Clone)]
enum Target {
    Window(Window),
    Application(Application),
}

struct Ui {
    window: gtk::ApplicationWindow,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    status: gtk::Label,
    socket: RefCell<PathBuf>,
    event_source: RefCell<Option<glib::SourceId>>,
    retry_pending: Cell<bool>,
    refresh_pending: Cell<bool>,
    app_monitor: gio::AppInfoMonitor,
    history_path: PathBuf,
    history: RefCell<History>,
    assignment_path: PathBuf,
    settings_path: PathBuf,
    settings: RefCell<Settings>,
    book: RefCell<Book>,
    recording: RefCell<Option<Target>>,
    applications: RefCell<Vec<Application>>,
    windows: RefCell<Vec<Window>>,
    targets: RefCell<Vec<Target>>,
    previous: RefCell<Option<Window>>,
    own_xid: Cell<Option<u64>>,
    was_active: Cell<bool>,
}

impl Ui {
    fn new(app: &gtk::Application) -> Result<Rc<Self>> {
        let socket = ipc::socket_path()?;
        let history_path = history::path()?;
        let history = History::load(&history_path)?;
        let assignment_path = assignments::path()?;
        let book = Book::load(&assignment_path)?;
        let settings_path = settings::path()?;
        let settings = Settings::load(&settings_path)?;
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Lite Switch")
            .default_width(680)
            .default_height(560)
            .resizable(false)
            .decorated(false)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(18);
        content.set_margin_bottom(14);
        content.set_margin_start(18);
        content.set_margin_end(18);
        let heading = gtk::Label::new(Some("Lite Switch"));
        heading.set_xalign(0.0);
        heading.add_css_class("title-2");
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search applications and windows…"));
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(false);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let status = gtk::Label::new(Some("Enter to activate · Esc to cancel · ↑↓ to select"));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.add_css_class("dim-label");
        content.append(&heading);
        content.append(&search);
        content.append(&scroll);
        content.append(&status);
        window.set_child(Some(&content));
        let ui = Rc::new(Self {
            window,
            search,
            list,
            status,
            socket: RefCell::new(socket),
            event_source: RefCell::new(None),
            retry_pending: Cell::new(false),
            refresh_pending: Cell::new(false),
            app_monitor: gio::AppInfoMonitor::get(),
            history_path,
            history: RefCell::new(history),
            assignment_path,
            settings_path,
            settings: RefCell::new(settings),
            book: RefCell::new(book),
            recording: RefCell::new(None),
            applications: RefCell::new(Vec::new()),
            windows: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            previous: RefCell::new(None),
            own_xid: Cell::new(None),
            was_active: Cell::new(false),
        });
        ui.connect();
        Ok(ui)
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.search.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.list.unselect_all();
                ui.render();
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(ui) = weak.upgrade() {
                ui.activate(row.index() as usize);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if ui.recording.borrow().is_some() {
                ui.capture_key(key);
                return glib::Propagation::Stop;
            }
            if modifiers.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK,
            ) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Escape => ui.dismiss(true),
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Some(row) = ui.list.selected_row() {
                        ui.activate(row.index() as usize);
                    }
                }
                gdk::Key::Down | gdk::Key::Up => {
                    let count = ui.targets.borrow().len() as i32;
                    if count > 0 {
                        let current = ui.list.selected_row().map_or(0, |r| r.index());
                        let next = (current + if key == gdk::Key::Down { 1 } else { -1 })
                            .rem_euclid(count);
                        if let Some(row) = ui.list.row_at_index(next) {
                            ui.list.select_row(Some(&row));
                            // Focus the row so GTK scrolls it into view, then keep
                            // typing routed to the search field.
                            row.grab_focus();
                            ui.search.grab_focus();
                        }
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.dismiss(true);
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            if let Some(ui) = weak.upgrade() {
                if window.is_active() {
                    ui.was_active.set(true);
                } else if window.is_visible() && ui.was_active.replace(false) {
                    // Outside clicks must not refocus the old window over the
                    // window the user has just clicked.
                    ui.dismiss(false);
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_map(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.position();
            }
        });
        let weak = Rc::downgrade(self);
        self.app_monitor.connect_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                *ui.applications.borrow_mut() = applications::discover();
                if ui.window.is_visible() {
                    ui.render();
                }
            }
        });
        self.start_events();
    }

    fn show(self: &Rc<Self>) -> Result<()> {
        *self.socket.borrow_mut() = ipc::socket_path()?;
        let root: Node = ipc::request(&self.socket.borrow(), ipc::GET_TREE, "")?;
        let windows: Vec<_> = windows::windows(&root)
            .into_iter()
            .filter(|w| Some(w.x11_id) != self.own_xid.get())
            .collect();
        self.book.borrow_mut().prune(&windows);
        self.recording.borrow_mut().take();
        *self.previous.borrow_mut() = windows.iter().find(|w| w.focused).cloned();
        *self.windows.borrow_mut() = windows;
        *self.applications.borrow_mut() = applications::discover();
        self.was_active.set(false);
        self.search.set_text("");
        self.render();
        self.window.present();
        self.search.grab_focus();
        Ok(())
    }

    fn position(self: &Rc<Self>) {
        let Some(surface) = self
            .window
            .surface()
            .and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok())
        else {
            self.status.set_text("Lite Switch requires an X11 session.");
            return;
        };
        let xid = surface.xid();
        self.own_xid.set(Some(xid));
        let weak = Rc::downgrade(self);
        let mut attempts = 0;
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !ui.window.is_visible() {
                return glib::ControlFlow::Break;
            }
            attempts += 1;
            let command = format!(
                "[id={xid}] floating enable, border pixel 0, resize set 680 px 560 px, move position center"
            );
            let result: Result<serde_json::Value> =
                ipc::request(&ui.socket.borrow(), ipc::COMMAND, &command);
            // A successful command can match nothing until i3 has managed the X window.
            let tree: Result<Node> = ipc::request(&ui.socket.borrow(), ipc::GET_TREE, "");
            let found =
                tree.is_ok_and(|root| windows::windows(&root).iter().any(|w| w.x11_id == xid));
            let accepted = result.is_ok_and(|reply| {
                reply.as_array().is_some_and(|replies| {
                    !replies.is_empty() && replies.iter().all(|r| r["success"] == true)
                })
            });
            if accepted && found {
                return glib::ControlFlow::Break;
            }
            if attempts >= 10 {
                ui.status
                    .set_text("Could not position launcher through i3.");
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    fn render(self: &Rc<Self>) {
        let selected = self.list.selected_row().and_then(|row| {
            self.targets
                .borrow()
                .get(row.index() as usize)
                .map(Target::key)
        });
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let query = self.search.text().to_string();
        let mut targets = Vec::new();
        for window in self
            .windows
            .borrow()
            .iter()
            .filter(|w| {
                crate::search::match_name(&query, &w.title).is_some()
                    || crate::search::match_name(&query, w.class.as_deref().unwrap_or("")).is_some()
            })
            .take(30)
        {
            let description = format!(
                "Window · {} · workspace {}{}",
                window.class.as_deref().unwrap_or("Unknown app"),
                window.workspace.as_deref().unwrap_or("?"),
                if window.scratchpad {
                    " · scratchpad"
                } else {
                    ""
                }
            );
            self.append(
                &window.title,
                &description,
                None,
                Target::Window(window.clone()),
            );
            targets.push(Target::Window(window.clone()));
        }
        let apps = self.applications.borrow();
        let history = self.history.borrow();
        for app in applications::ranked(&apps, &query, &history, history::now())
            .into_iter()
            .take(40)
        {
            let running = self
                .windows
                .borrow()
                .iter()
                .filter(|window| {
                    switching::matching_app(window, &apps)
                        .is_some_and(|matched| matched.id == app.id)
                })
                .count();
            let description = if running == 0 {
                format!("Application · {}", app.id)
            } else {
                format!("Running application · {running} window(s) · {}", app.id)
            };
            self.append(
                &app.name,
                &description,
                app.info.icon(),
                Target::Application(app.clone()),
            );
            targets.push(Target::Application(app.clone()));
        }
        self.status.set_text(if self.recording.borrow().is_some() {
            "Press a letter/number/arrow · Backspace removes · Esc cancels"
        } else if targets.is_empty() {
            "No results"
        } else {
            "Enter to activate · Esc to cancel · ↑↓ to select"
        });
        let index = selected
            .and_then(|key| targets.iter().position(|target| target.key() == key))
            .unwrap_or(0);
        *self.targets.borrow_mut() = targets;
        self.list
            .select_row(self.list.row_at_index(index as i32).as_ref());
    }

    fn append(
        self: &Rc<Self>,
        title: &str,
        description: &str,
        icon: Option<gio::Icon>,
        target: Target,
    ) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_margin_top(8);
        row.set_margin_bottom(8);
        row.set_margin_start(10);
        row.set_margin_end(10);
        let image = icon
            .as_ref()
            .map(gtk::Image::from_gicon)
            .unwrap_or_else(|| gtk::Image::from_icon_name("application-x-executable"));
        image.set_pixel_size(28);
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 3);
        labels.set_hexpand(true);
        for (text, secondary) in [(title, false), (description, true)] {
            let label = gtk::Label::new(Some(text));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if secondary {
                label.add_css_class("dim-label");
                label.add_css_class("caption");
            }
            labels.append(&label);
        }
        row.append(&image);
        row.append(&labels);
        let assignment = target.assignment();
        let assigned = self.book.borrow().key_for(&assignment).cloned();
        if let (Target::Application(app), Some(key)) = (&target, &assigned) {
            let checked = matches!(
                self.book.borrow().targets.get(key),
                Some(Assignment::Application {
                    launch_if_needed: true,
                    ..
                })
            );
            let checkbox = gtk::CheckButton::with_label("Launch");
            checkbox.set_tooltip_text(Some("Launch when no app windows are running"));
            checkbox.set_active(checked);
            let weak = Rc::downgrade(self);
            let id = app.id.clone();
            checkbox.connect_toggled(move |checkbox| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let result =
                    ui.book
                        .borrow_mut()
                        .set_launch(&ui.assignment_path, &id, checkbox.is_active());
                if let Err(e) = result {
                    ui.render();
                    ui.status
                        .set_text(&format!("Cannot save launch preference: {e}"));
                }
            });
            row.append(&checkbox);
        }
        let button = gtk::Button::with_label(&assigned.map_or_else(
            || "+ key".to_owned(),
            |key| key.label(self.settings.borrow().assignment_modifier.label()),
        ));
        button.set_tooltip_text(Some("Assign a key; Backspace removes the assignment. Global binding installation is separate."));
        let weak = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                *ui.recording.borrow_mut() = Some(target.clone());
                ui.status.set_text("Press a letter/number/arrow · Backspace removes · Esc cancels (bindings exported separately)");
                ui.search.grab_focus();
            }
        });
        row.append(&button);
        self.list.append(&row);
    }

    fn activate(&self, index: usize) {
        let Some(target) = self.targets.borrow().get(index).cloned() else {
            return;
        };
        let result = match target {
            Target::Window(target) => {
                self.window.set_visible(false);
                windows::focus_window(&self.socket.borrow(), &target)
            }
            Target::Application(app) => self.activate_application(&app, true),
        };
        if let Err(e) = result {
            self.window.present();
            self.status.set_text(&format!("Activation failed: {e}"));
        }
    }

    fn activate_application(&self, app: &Application, allow_launch: bool) -> Result<()> {
        let root: Node = ipc::request(&self.socket.borrow(), ipc::GET_TREE, "")?;
        let windows = windows::windows(&root);
        self.book.borrow_mut().prune(&windows);
        let apps = self.applications.borrow();
        let matching: Vec<_> = windows
            .iter()
            .filter(|window| {
                Some(window.x11_id) != self.own_xid.get()
                    && switching::matching_app(window, &apps)
                        .is_some_and(|matched| matched.id == app.id)
            })
            .collect();
        let running = !matching.is_empty();
        let candidates = matching
            .into_iter()
            .filter(|window| !self.book.borrow().excludes(window))
            .collect();
        let current = if self.window.is_visible() {
            self.previous.borrow().as_ref().map(|window| window.id)
        } else {
            windows
                .iter()
                .find(|window| window.focused)
                .map(|window| window.id)
        };
        if let Some(window) = switching::next_window(candidates, current) {
            self.window.set_visible(false);
            windows::focus_window(&self.socket.borrow(), window)?;
        } else if running {
            return Err(
                "all app windows are individually assigned; no eligible windows to cycle".into(),
            );
        } else if !allow_launch {
            return Err("application is not running and launch-if-needed is disabled".into());
        } else {
            app.info.launch(&[], None::<&gio::AppLaunchContext>)?;
            self.window.set_visible(false);
        }
        if let Err(e) = self
            .history
            .borrow_mut()
            .record(&app.id, &self.history_path)
        {
            eprintln!("lite-switch: activated app but could not save usage history: {e}");
        }
        Ok(())
    }

    fn dismiss(&self, restore: bool) {
        self.recording.borrow_mut().take();
        self.window.set_visible(false);
        if !restore {
            return;
        }
        let previous = self.previous.borrow();
        let Some(previous) = previous.as_ref() else {
            return;
        };
        let tree: Result<Node> = ipc::request(&self.socket.borrow(), ipc::GET_TREE, "");
        if !tree.is_ok_and(|root| {
            windows::windows(&root)
                .iter()
                .any(|w| w.id == previous.id && w.x11_id == previous.x11_id)
        }) {
            return;
        }
        if let Err(e) = windows::focus_window(&self.socket.borrow(), previous) {
            eprintln!("lite-switch: {e}");
        }
    }
}

impl Target {
    fn assignment(&self) -> Assignment {
        match self {
            Self::Window(window) => Assignment::window(window),
            Self::Application(app) => Assignment::Application {
                desktop_id: app.id.clone(),
                launch_if_needed: false,
            },
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Window(window) => format!("window:{}:{}", window.id, window.x11_id),
            Self::Application(app) => format!("app:{}", app.id),
        }
    }
}

pub fn run(commands: &[String]) -> i32 {
    glib::set_prgname(Some("lite-switch"));
    glib::set_application_name("Lite Switch");
    let app = gtk::Application::new(
        Some("app.alasdair.LiteSwitch.I3"),
        gio::ApplicationFlags::HANDLES_COMMAND_LINE,
    );
    let state: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    app.connect_command_line(move |app, command_line| {
        let arguments = command_line.arguments();
        let action = arguments
            .get(1)
            .and_then(|s| s.to_str())
            .unwrap_or("toggle");
        if action == "quit" {
            if let Some(ui) = state.borrow().as_ref() {
                ui.dismiss(true);
            }
            app.quit();
            return 0.into();
        }
        if state.borrow().is_none() {
            match Ui::new(app) {
                Ok(ui) => *state.borrow_mut() = Some(ui),
                Err(e) => {
                    eprintln!("lite-switch: {e}");
                    app.quit();
                    return 1.into();
                }
            }
        }
        let ui = state.borrow().as_ref().unwrap().clone();
        if !["show", "toggle", "cycle"].contains(&action) {
            let args: Vec<_> = arguments
                .iter()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            match ui.assignment_command(&args) {
                Ok(output) => command_line.print_literal(&output),
                Err(e) => {
                    command_line.printerr_literal(&format!("lite-switch: {e}\n"));
                    return 1.into();
                }
            }
            return 0.into();
        }
        if action == "cycle" {
            *ui.applications.borrow_mut() = applications::discover();
            let id = arguments.get(2).and_then(|s| s.to_str()).unwrap_or("");
            let target = ui
                .applications
                .borrow()
                .iter()
                .find(|app| app.id == id)
                .cloned();
            let result = target
                .ok_or_else(|| format!("desktop entry not found: {id}").into())
                .and_then(|app| ui.activate_application(&app, true));
            if let Err(e) = result {
                eprintln!("lite-switch: {e}");
                return 1.into();
            }
            return 0.into();
        }
        if action == "toggle" && ui.window.is_visible() {
            ui.dismiss(true);
        } else if !ui.window.is_visible() {
            if let Err(e) = ui.show() {
                eprintln!("lite-switch: {e}");
                app.quit();
                return 1.into();
            }
        } else {
            ui.window.present();
        }
        0.into()
    });
    let mut arguments = vec!["lite-switch".to_owned()];
    arguments.extend_from_slice(commands);
    app.run_with_args(&arguments).into()
}
