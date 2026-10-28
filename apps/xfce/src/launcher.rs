mod assignments_ui;
mod event_driver;
mod results;
mod service;
mod shortcuts_ui;

const APP_ID: &str = "app.alasdair.LiteSwitch.Xfce";

use crate::{
    actions::Action,
    applications::{self, Application},
    assignments::{self, Assignment, Book, Key},
    bindings,
    history::{self, History},
    matching::{self, Overrides},
    settings::{self, Settings},
    switching,
    x11::{Desktop, Result, Window},
};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

#[derive(Clone)]
enum Target {
    Action(Action),
    Window(Window),
    Application(Application),
}

struct Ui {
    window: gtk::ApplicationWindow,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    status: gtk::Label,
    desktop: RefCell<Option<Desktop>>,
    event_source: RefCell<Option<glib::SourceId>>,
    retry_pending: Cell<bool>,
    refresh_pending: Cell<bool>,
    app_monitor: gio::AppInfoMonitor,
    history_path: PathBuf,
    history: RefCell<History>,
    assignment_path: PathBuf,
    settings_path: PathBuf,
    settings: RefCell<Settings>,
    matching_path: PathBuf,
    overrides: RefCell<Overrides>,
    book: RefCell<Book>,
    recording: RefCell<Option<Target>>,
    shortcut_notice: RefCell<Option<String>>,
    applications: RefCell<Vec<Application>>,
    windows: RefCell<Vec<Window>>,
    targets: RefCell<Vec<Target>>,
    previous: RefCell<Option<Window>>,
    own_xid: Cell<Option<u32>>,
    was_active: Cell<bool>,
}

impl Ui {
    fn new(app: &gtk::Application) -> Result<Rc<Self>> {
        let desktop = Desktop::connect()?;
        let history_path = history::path()?;
        let history = History::load(&history_path)?;
        let assignment_path = assignments::path()?;
        let book = Book::load(&assignment_path)?;
        let settings_path = settings::path()?;
        let settings = Settings::load(&settings_path)?;
        let matching_path = matching::path()?;
        let overrides = Overrides::load(&matching_path)?;
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
        search.set_placeholder_text(Some(
            "Search apps/actions · windows: to list individual windows",
        ));
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
            desktop: RefCell::new(Some(desktop)),
            event_source: RefCell::new(None),
            retry_pending: Cell::new(false),
            refresh_pending: Cell::new(false),
            app_monitor: gio::AppInfoMonitor::get(),
            history_path,
            history: RefCell::new(history),
            assignment_path,
            settings_path,
            settings: RefCell::new(settings),
            matching_path,
            overrides: RefCell::new(overrides),
            book: RefCell::new(book),
            recording: RefCell::new(None),
            shortcut_notice: RefCell::new(None),
            applications: RefCell::new(Vec::new()),
            windows: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            previous: RefCell::new(None),
            own_xid: Cell::new(None),
            was_active: Cell::new(false),
        });
        ui.connect();
        if ui.settings.borrow().auto_sync_shortcuts {
            let _ = ui.sync_shortcuts();
        }
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
                } else if window.is_visible() {
                    // Xfce shortcut grabs temporarily remove GTK keyboard focus
                    // without changing the WM's active window. Dismissing here
                    // would make the arriving toggle reopen the panel. Confirm
                    // real outside activation through EWMH instead.
                    ui.schedule_refresh();
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

    fn live_windows(&self) -> Result<Vec<Window>> {
        self.desktop
            .borrow_mut()
            .as_mut()
            .ok_or("waiting for Xfce/X11 reconnection")?
            .windows()
    }

    fn focus_window(&self, window: &Window) -> Result<()> {
        self.desktop
            .borrow_mut()
            .as_mut()
            .ok_or("waiting for Xfce/X11 reconnection")?
            .focus(window)
    }

    fn show(self: &Rc<Self>) -> Result<()> {
        if self.desktop.borrow().is_none() {
            *self.desktop.borrow_mut() = Some(Desktop::connect()?);
            self.start_events();
        }
        let windows: Vec<_> = self
            .live_windows()?
            .into_iter()
            .filter(|w| Some(w.x11_id) != self.own_xid.get())
            .collect();
        self.prune_assignments(&windows);
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
        let Ok(xid) = u32::try_from(surface.xid()) else {
            return;
        };
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
            let output = ui.previous.borrow().as_ref().and_then(|w| w.output.clone());
            let result = ui
                .desktop
                .borrow_mut()
                .as_mut()
                .ok_or_else(|| -> Box<dyn std::error::Error> { "waiting for Xfce".into() })
                .and_then(|desktop| desktop.prepare_panel(xid, 680, 560, output.as_deref()));
            if matches!(result, Ok(true)) {
                // Once xfwm4 has managed/positioned the panel, losing focus must
                // dismiss even if GTK coalesced the initial FocusIn/FocusOut.
                ui.was_active.set(true);
                let active = ui
                    .desktop
                    .borrow()
                    .as_ref()
                    .and_then(|desktop| desktop.active_id().ok())
                    .flatten();
                if active != Some(xid) {
                    ui.dismiss(false);
                }
                return glib::ControlFlow::Break;
            }
            if attempts >= 10 {
                ui.status
                    .set_text("Could not position launcher through xfwm4.");
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
        let apps = self.applications.borrow();
        let rules = self.overrides.borrow();
        let windows_only = results::window_query(&query).is_some();
        for window in self
            .windows
            .borrow()
            .iter()
            .filter(|w| {
                let app = switching::matched_app(w, &apps, &rules);
                results::show_window(
                    &query,
                    &w.title,
                    w.class.as_deref().unwrap_or(""),
                    app.map(|app| app.name.as_str()),
                    self.book.borrow().key_for(&Assignment::window(w)).is_some(),
                )
            })
            .take(30)
        {
            let description = format!(
                "Window · {} · workspace {}{}",
                window.class.as_deref().unwrap_or("Unknown app"),
                window.workspace_name.as_deref().unwrap_or("?"),
                if window.minimized {
                    " · minimized"
                } else {
                    ""
                }
            );
            self.append(
                &window.title,
                &description,
                switching::matched_app(window, &apps, &rules).and_then(|app| app.info.icon()),
                Target::Window(window.clone()),
            );
            targets.push(Target::Window(window.clone()));
        }
        let history = self.history.borrow();
        for app in applications::ranked(&apps, &query, &history, history::now())
            .into_iter()
            .filter(|_| !windows_only)
            .take(40)
        {
            let running = self
                .windows
                .borrow()
                .iter()
                .filter(|window| {
                    switching::matched_app(window, &apps, &self.overrides.borrow())
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
        for action in Action::ALL {
            if !windows_only
                && (crate::search::match_name(&query, action.title()).is_some()
                    || crate::search::match_name(&query, action.id()).is_some())
            {
                self.append(
                    action.title(),
                    "Window action · targets the window active before the panel",
                    None,
                    Target::Action(action),
                );
                targets.push(Target::Action(action));
            }
        }
        let notice = self.shortcut_notice.borrow();
        self.status.set_text(if self.recording.borrow().is_some() {
            "Press a letter/number/arrow · Backspace removes · Esc cancels"
        } else if let Some(notice) = notice.as_deref() {
            notice
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
            .unwrap_or_else(|| {
                gtk::Image::from_icon_name(if matches!(&target, Target::Action(_)) {
                    "view-grid-symbolic"
                } else {
                    "application-x-executable"
                })
            });
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
        button.set_tooltip_text(Some("Assign a key; Backspace removes it. With approved automatic sync, global shortcuts update immediately (conflicts are backed up and replaced)."));
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
            Target::Action(action) => self.run_action(action).map(|_| ()),
            Target::Window(target) => {
                self.window.set_visible(false);
                self.focus_window(&target)
            }
            Target::Application(app) => self.activate_application(&app, true),
        };
        if let Err(e) = result {
            self.window.present();
            self.status.set_text(&format!("Activation failed: {e}"));
        }
    }

    fn run_action(&self, action: Action) -> Result<crate::x11::ActionOutcome> {
        let target = if self.window.is_visible() {
            self.previous.borrow().clone()
        } else {
            self.live_windows()?
                .into_iter()
                .find(|w| w.focused && Some(w.x11_id) != self.own_xid.get())
        }
        .ok_or("no previous/active application window for this action")?;
        self.recording.borrow_mut().take();
        self.window.set_visible(false);
        self.desktop
            .borrow_mut()
            .as_mut()
            .ok_or("waiting for Xfce/X11 reconnection")?
            .perform_action(&target, action)
    }

    fn activate_application(&self, app: &Application, allow_launch: bool) -> Result<()> {
        let windows = self.live_windows()?;
        self.prune_assignments(&windows);
        let apps = self.applications.borrow();
        let matching: Vec<_> = windows
            .iter()
            .filter(|window| {
                Some(window.x11_id) != self.own_xid.get()
                    && switching::matched_app(window, &apps, &self.overrides.borrow())
                        .is_some_and(|matched| matched.id == app.id)
            })
            .collect();
        let running = !matching.is_empty();
        if !running
            && windows.iter().any(|window| {
                switching::ambiguous_for(window, &apps, &self.overrides.borrow(), &app.id)
            })
        {
            return Err("application/window identity is ambiguous; add an explicit match-app override before cycling or launching".into());
        }
        let candidates = matching
            .into_iter()
            .filter(|window| !self.book.borrow().excludes(window))
            .collect();
        let current = if self.window.is_visible() {
            self.previous.borrow().as_ref().map(|window| window.x11_id)
        } else {
            windows
                .iter()
                .find(|window| window.focused)
                .map(|window| window.x11_id)
        };
        if let Some(window) = switching::next_window(candidates, current) {
            self.window.set_visible(false);
            self.focus_window(window)?;
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
        if restore
            && let Some(previous) = self.previous.borrow().as_ref()
            && let Err(error) = self.focus_window(previous)
        {
            eprintln!("lite-switch-xfce: previous window not restored: {error}");
        }
    }
}

impl Target {
    fn assignment(&self) -> Assignment {
        match self {
            Self::Action(action) => Assignment::Action { action: *action },
            Self::Window(window) => Assignment::window(window),
            Self::Application(app) => Assignment::Application {
                desktop_id: app.id.clone(),
                launch_if_needed: false,
            },
        }
    }

    fn key(&self) -> String {
        match self {
            Self::Action(action) => format!("action:{}", action.id()),
            Self::Window(window) => format!("window:{}:{}", window.identity, window.x11_id),
            Self::Application(app) => format!("app:{}", app.id),
        }
    }
}

pub fn run(commands: &[String]) -> i32 {
    match service::ensure(commands) {
        Ok(true) => return 0,
        Ok(false) => {}
        Err(error) => {
            eprintln!("lite-switch-xfce: {error}");
            return 1;
        }
    }
    glib::set_prgname(Some("lite-switch-xfce"));
    glib::set_application_name("Lite Switch");
    let app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::HANDLES_COMMAND_LINE);
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
                    command_line.printerr_literal(&format!("lite-switch-xfce: {e}\n"));
                    app.quit();
                    return 1.into();
                }
            }
        }
        let ui = state.borrow().as_ref().unwrap().clone();
        if action == "__serve" {
            return 0.into();
        }
        if !["show", "toggle", "cycle"].contains(&action) {
            let args: Vec<_> = arguments
                .iter()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            return match ui.assignment_command(&args) {
                Ok(output) => {
                    command_line.print_literal(&output);
                    0.into()
                }
                Err(error) => {
                    command_line.printerr_literal(&format!("lite-switch-xfce: {error}\n"));
                    1.into()
                }
            };
        }
        let result = if action == "cycle" {
            *ui.applications.borrow_mut() = applications::discover();
            let id = arguments.get(2).and_then(|s| s.to_str()).unwrap_or("");
            let app = ui
                .applications
                .borrow()
                .iter()
                .find(|app| app.id == id)
                .cloned();
            app.ok_or_else(|| format!("desktop entry not found: {id}").into())
                .and_then(|app| ui.activate_application(&app, true))
        } else if action == "toggle" && ui.window.is_visible() {
            ui.dismiss(true);
            Ok(())
        } else if !ui.window.is_visible() {
            ui.show()
        } else {
            ui.window.present();
            Ok(())
        };
        match result {
            Ok(()) => 0.into(),
            Err(error) => {
                command_line.printerr_literal(&format!("lite-switch-xfce: {error}\n"));
                1.into()
            }
        }
    });
    let mut arguments = vec!["lite-switch-xfce".to_owned()];
    arguments.extend_from_slice(commands);
    app.run_with_args(&arguments).into()
}
