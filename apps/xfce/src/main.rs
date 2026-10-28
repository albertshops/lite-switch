mod actions;
#[cfg(feature = "gui")]
mod applications;
#[cfg(feature = "gui")]
mod assignments;
#[cfg(feature = "gui")]
mod bindings;
#[cfg(feature = "gui")]
mod history;
#[cfg(feature = "gui")]
mod launcher;
#[cfg(feature = "gui")]
mod matching;
#[cfg(feature = "gui")]
mod search;
#[cfg(feature = "gui")]
mod settings;
#[cfg(feature = "gui")]
mod storage;
#[cfg(feature = "gui")]
mod switching;
mod x11;
use std::process::ExitCode;
use x11::{Desktop, Result};

const HELP: &str = "Lite Switch for Xfce/X11 (port in progress)

Usage: lite-switch-xfce <command>
  doctor             Verify xfwm4/X11 and report connection details
  windows [query]    List/search managed application windows as JSON
  outputs            List active RandR monitor geometry as JSON
  focus <x11-id>     Activate a currently managed window across workspaces
  --help             Show this help
  --version          Show version

  show / toggle / quit         Control the resident GTK launcher
  cycle <desktop-id>           Cycle app windows or launch if none exist
  actions / action <id>        List/execute a previous-window action
  assign-action <key> <id>     Persist a direct window-action assignment
  assignments / settings      Print local preferences/assignments
  assign-app <key> <id> [--launch-if-needed]
  assign-window <key> <x11-id> Assign a window for this app session
  unassign <key> / activate <key>
  launch-if-needed <key> on|off
  set-launcher <chord> / set-modifier Alt|Super
  set-shortcut-sync on --approve-xfce-shortcuts / set-shortcut-sync off
  matching                    Print explicit app/window identity rules
  match-app <desktop-id> [--class value] [--instance value] [--role value]
  unmatch-app <desktop-id>     Remove an identity override
  bindings                    Print review-only Xfce shortcut proposals
  bindings-check / bindings-status        Read-only priority/ownership checks
  bindings-install --approve-xfce-shortcuts  Back up and install priority keys
  bindings-sync --approve-xfce-shortcuts     Apply a reviewed shortcut update
  bindings-uninstall --approve-xfce-shortcuts Restore displaced bindings

Graphical commands require the default gui feature and X11/session D-Bus.
Shortcut edits require approved lifecycle commands or explicit automatic-sync
opt-in. Automatic sync only updates an existing installation. App autostart is not installed.
";

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Doctor,
    Windows(String),
    Outputs,
    Actions,
    Focus(u32),
    Gui(Vec<String>),
}
fn parse(args: &[String]) -> Result<Command> {
    match args {
        [] => Ok(Command::Help),
        [arg] if ["-h", "--help"].contains(&arg.as_str()) => Ok(Command::Help),
        [arg] if ["-V", "--version"].contains(&arg.as_str()) => Ok(Command::Version),
        [arg]
            if [
                "show",
                "__serve",
                "toggle",
                "quit",
                "assignments",
                "matching",
                "settings",
                "bindings",
                "bindings-check",
                "bindings-status",
                "bindings-install",
                "bindings-sync",
                "bindings-uninstall",
            ]
            .contains(&arg.as_str()) =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, flag]
            if ["bindings-install", "bindings-sync", "bindings-uninstall"]
                .contains(&arg.as_str())
                && flag == "--approve-xfce-shortcuts" =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, _]
            if [
                "cycle",
                "activate",
                "unassign",
                "unmatch-app",
                "set-launcher",
                "set-modifier",
            ]
            .contains(&arg.as_str()) =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, value] if arg == "set-shortcut-sync" && ["on", "off"].contains(&value.as_str()) => {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, value, flag]
            if arg == "set-shortcut-sync"
                && value == "on"
                && flag == "--approve-xfce-shortcuts" =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, _, rest @ ..] if arg == "match-app" && !rest.is_empty() => {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, _, _]
            if ["assign-app", "assign-window", "launch-if-needed"].contains(&arg.as_str()) =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, _, _, flag] if arg == "assign-app" && flag == "--launch-if-needed" => {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg] if arg == "doctor" => Ok(Command::Doctor),
        [arg] if arg == "outputs" => Ok(Command::Outputs),
        [arg] if arg == "actions" => Ok(Command::Actions),
        [arg, name] if arg == "action" => {
            actions::Action::parse(name)?;
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, _, name] if arg == "assign-action" => {
            actions::Action::parse(name)?;
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, query @ ..] if arg == "windows" => Ok(Command::Windows(query.join(" "))),
        [arg, id] if arg == "focus" => {
            let id: u32 = id
                .parse()
                .map_err(|_| "focus requires a positive numeric X11 window ID")?;
            if id == 0 {
                return Err("focus requires a positive numeric X11 window ID".into());
            }
            Ok(Command::Focus(id))
        }
        _ => Err("unsupported arguments; use --help".into()),
    }
}

fn run(command: Command) -> Result<String> {
    match command {
        Command::Help => return Ok(HELP.to_owned()),
        Command::Version => return Ok(format!("lite-switch-xfce {}\n", env!("CARGO_PKG_VERSION"))),
        Command::Actions => {
            return Ok(format!(
                "{}\n",
                serde_json::to_string_pretty(
                    &actions::Action::ALL
                        .map(|action| serde_json::json!({"id":action.id(),"name":action.title()}))
                )?
            ));
        }
        _ => {}
    }
    let mut desktop = Desktop::connect()?;
    let value = match command {
        Command::Doctor => {
            let windows = desktop.windows()?;
            serde_json::json!({"window_manager": "Xfwm4", "display": std::env::var("DISPLAY").ok(),
                "window_count": windows.len(), "active_window": desktop.active_id()?, "outputs": desktop.outputs()?})
        }
        Command::Windows(query) => {
            let words: Vec<_> = query
                .to_lowercase()
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let windows: Vec<_> = desktop
                .windows()?
                .into_iter()
                .filter(|w| {
                    let text = format!(
                        "{} {} {}",
                        w.title,
                        w.class.as_deref().unwrap_or(""),
                        w.instance.as_deref().unwrap_or("")
                    )
                    .to_lowercase();
                    words.iter().all(|word| text.contains(word))
                })
                .collect();
            serde_json::to_value(windows)?
        }
        Command::Outputs => serde_json::to_value(desktop.outputs()?)?,
        Command::Focus(id) => {
            let target = desktop
                .windows()?
                .into_iter()
                .find(|w| w.x11_id == id)
                .ok_or("window is not a managed application or has closed")?;
            desktop.focus(&target)?;
            serde_json::json!({"focused": id})
        }
        Command::Help | Command::Version | Command::Actions | Command::Gui(_) => unreachable!(),
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}
fn main() -> ExitCode {
    let command = parse(&std::env::args().skip(1).collect::<Vec<_>>());
    if let Ok(Command::Gui(arguments)) = &command {
        #[cfg(feature = "gui")]
        return ExitCode::from(launcher::run(arguments).clamp(0, 255) as u8);
        #[cfg(not(feature = "gui"))]
        {
            eprintln!(
                "lite-switch-xfce: {} requires a build with the gui feature",
                arguments[0]
            );
            return ExitCode::FAILURE;
        }
    }
    match command.and_then(run) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lite-switch-xfce: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }
    #[test]
    fn validates_ids_and_arguments_without_needing_a_display() {
        for words in [
            ["focus", "0"],
            ["focus", "-1"],
            ["focus", "1;kill"],
            ["focus", "4294967296"],
            ["outputs", "extra"],
        ] {
            assert!(parse(&args(&words)).is_err());
        }
        assert_eq!(parse(&args(&["focus", "42"])).unwrap(), Command::Focus(42));
        assert_eq!(
            parse(&args(&["windows", "editor", "project"])).unwrap(),
            Command::Windows("editor project".into())
        );
        assert!(run(Command::Help).unwrap().contains("Xfce"));
        assert!(
            run(Command::Version)
                .unwrap()
                .contains(env!("CARGO_PKG_VERSION"))
        );
    }
}
