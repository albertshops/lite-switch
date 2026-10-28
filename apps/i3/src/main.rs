#[cfg(feature = "gui")]
mod applications;
#[cfg(feature = "gui")]
mod assignments;
#[cfg(feature = "gui")]
mod bindings;
#[cfg(feature = "gui")]
mod events;
#[cfg(feature = "gui")]
mod history;
mod ipc;
#[cfg(feature = "gui")]
mod launcher;
#[cfg(feature = "gui")]
mod search;
#[cfg(feature = "gui")]
mod settings;
#[cfg(feature = "gui")]
mod storage;
#[cfg(feature = "gui")]
mod switching;
mod windows;

use ipc::Result;
use std::{env, process::ExitCode};

const HELP: &str = "\
Lite Switch for i3

Usage: lite-switch <command>

  toggle               Toggle the graphical launcher (resident, single instance)
  show                 Show the graphical launcher
  quit                 Stop the resident graphical launcher
  cycle <desktop-id>   Cycle existing app windows or launch if none exist
  assignments          Print persistent and current-session assignments as JSON
  assign-app <key> <id> [--launch-if-needed]  Assign an installed application
  assign-window <key> <container-id>         Assign a window for this session
  unassign <key>        Remove an assignment
  activate <key>        Activate an assigned target (for i3 bindings)
  launch-if-needed <key> on|off              Set an app shortcut's launch policy
  bindings              Print a proposed i3 include; does not install it
  bindings-check        Validate the proposal against your config without edits
  settings              Print saved shortcut settings
  set-launcher <chord>   Save launcher shortcut (e.g. Super+space); no i3 edits
  set-modifier Alt|Super Save direct-assignment modifier; no i3 edits
  bindings-status       Show managed installation status/pending changes
  bindings-install --approve-i3-config    Back up, validate, install and reload
  bindings-sync --approve-i3-config       Apply a reviewed binding update
  bindings-uninstall --approve-i3-config  Remove only the managed integration
  windows [query ...]   List/search X11 windows as JSON (including scratchpad)
  outputs              List i3 outputs as JSON
  focus <container-id> Focus an existing window, revealing scratchpad if needed
  doctor               Check the i3 connection and show diagnostic information
  -h, --help           Show this help
  -V, --version        Show the version

Set I3SOCK explicitly or run inside an i3 session for socket discovery.
Graphical commands require the default gui feature and an X11 session.
Shortcut installation is not implemented yet.
";

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Windows(String),
    Outputs,
    Focus(u64),
    Doctor,
    Gui(Vec<String>),
}

fn parse(args: &[String]) -> Result<Command> {
    match args {
        [] => Ok(Command::Help),
        [arg] if arg == "-h" || arg == "--help" => Ok(Command::Help),
        [arg] if arg == "-V" || arg == "--version" => Ok(Command::Version),
        [arg] if ["toggle", "show", "quit"].contains(&arg.as_str()) => {
            Ok(Command::Gui(vec![arg.clone()]))
        }
        [arg, id] if arg == "cycle" && !id.is_empty() && !id.starts_with('-') => {
            Ok(Command::Gui(vec![arg.clone(), id.clone()]))
        }
        [arg]
            if [
                "assignments",
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
            Ok(Command::Gui(vec![arg.clone()]))
        }
        [arg, _]
            if ["activate", "unassign", "set-launcher", "set-modifier"].contains(&arg.as_str()) =>
        {
            Ok(Command::Gui(args.to_vec()))
        }
        [arg, flag]
            if ["bindings-install", "bindings-sync", "bindings-uninstall"]
                .contains(&arg.as_str())
                && flag == "--approve-i3-config" =>
        {
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
        [arg] if arg == "outputs" => Ok(Command::Outputs),
        [arg] if arg == "doctor" => Ok(Command::Doctor),
        [arg, query @ ..] if arg == "windows" => Ok(Command::Windows(query.join(" "))),
        [arg, id] if arg == "focus" => {
            let id: u64 = id
                .parse()
                .map_err(|_| "focus requires a positive numeric container ID")?;
            if id == 0 {
                return Err("focus requires a positive numeric container ID".into());
            }
            Ok(Command::Focus(id))
        }
        _ => Err("unsupported arguments; use --help".into()),
    }
}

fn run(command: Command) -> Result<String> {
    match command {
        Command::Help => return Ok(HELP.to_owned()),
        Command::Version => return Ok(format!("lite-switch {}\n", env!("CARGO_PKG_VERSION"))),
        _ => {}
    }
    let path = ipc::socket_path()?;
    let value = match command {
        Command::Windows(query) => {
            let tree: windows::Node = ipc::request(&path, ipc::GET_TREE, "")?;
            serde_json::to_value(windows::filter_windows(windows::windows(&tree), &query))?
        }
        Command::Outputs => ipc::request(&path, ipc::GET_OUTPUTS, "")?,
        Command::Doctor => {
            let version: serde_json::Value = ipc::request(&path, ipc::GET_VERSION, "")?;
            let tree: windows::Node = ipc::request(&path, ipc::GET_TREE, "")?;
            let windows = windows::windows(&tree);
            serde_json::json!({
                "socket": path,
                "i3": version,
                "window_count": windows.len(),
                "focused_window": windows.iter().find(|window| window.focused),
                "display": env::var("DISPLAY").ok(),
            })
        }
        Command::Focus(id) => {
            windows::focus(&path, id)?;
            serde_json::json!({"focused": id})
        }
        Command::Help | Command::Version | Command::Gui(_) => unreachable!(),
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    #[cfg(feature = "gui")]
    if let [action, base, expression] = arguments.as_slice()
        && action == "__expand-include"
    {
        return match bindings::source::expansion_helper(std::path::Path::new(base), expression) {
            Ok(output) => {
                println!("{output}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let command = parse(&arguments);
    if let Ok(Command::Gui(action)) = &command {
        #[cfg(feature = "gui")]
        return ExitCode::from(launcher::run(action).clamp(0, 255) as u8);
        #[cfg(not(feature = "gui"))]
        {
            eprintln!(
                "lite-switch: {} requires a build with the gui feature",
                action[0]
            );
            return ExitCode::FAILURE;
        }
    }
    let result = command.and_then(run);
    match result {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("lite-switch: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn help_and_version_do_not_require_a_desktop() {
        assert_eq!(run(parse(&[]).unwrap()).unwrap(), HELP);
        for flag in ["-h", "--help"] {
            assert_eq!(parse(&args(&[flag])).unwrap(), Command::Help);
        }
        for flag in ["-V", "--version"] {
            assert_eq!(parse(&args(&[flag])).unwrap(), Command::Version);
        }
    }

    #[test]
    fn parses_window_query_and_commands() {
        assert_eq!(
            parse(&args(&["windows"])).unwrap(),
            Command::Windows(String::new())
        );
        assert_eq!(
            parse(&args(&["windows", "editor", "project"])).unwrap(),
            Command::Windows("editor project".into())
        );
        assert_eq!(
            parse(&args(&["focus", "123"])).unwrap(),
            Command::Focus(123)
        );
        assert_eq!(parse(&args(&["doctor"])).unwrap(), Command::Doctor);
        assert_eq!(parse(&args(&["outputs"])).unwrap(), Command::Outputs);
    }

    #[test]
    fn rejects_invalid_or_injectable_focus_ids_and_extra_arguments() {
        for values in [
            vec!["focus"],
            vec!["focus", "0"],
            vec!["focus", "-1"],
            vec!["focus", "1] kill"],
            vec!["focus", "18446744073709551616"],
            vec!["outputs", "extra"],
            vec!["--help", "extra"],
            vec!["unknown-command"],
        ] {
            assert!(parse(&args(&values)).is_err(), "{values:?}");
        }
    }
}
