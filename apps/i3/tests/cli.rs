//! Exercise the compiled CLI over real Unix sockets without a desktop session.
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Server {
    directory: PathBuf,
    socket: PathBuf,
}

impl Server {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "lite-switch-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        Self {
            socket: directory.join("ipc"),
            directory,
        }
    }

    fn run(&self, args: &[&str], exchanges: Vec<(u32, String, u32, String)>) -> Output {
        let listener = UnixListener::bind(&self.socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            for (kind, payload, reply_kind, reply) in exchanges {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "client did not connect"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("accept failed: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut header = [0; 14];
                stream.read_exact(&mut header).unwrap();
                assert_eq!(&header[..6], b"i3-ipc");
                assert_eq!(u32::from_le_bytes(header[10..14].try_into().unwrap()), kind);
                let length = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
                assert!(length < 1024);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                assert_eq!(String::from_utf8(body).unwrap(), payload);
                stream.write_all(b"i3-ipc").unwrap();
                stream
                    .write_all(&(reply.len() as u32).to_le_bytes())
                    .unwrap();
                stream.write_all(&reply_kind.to_le_bytes()).unwrap();
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_lite-switch"))
            .args(args)
            .env("I3SOCK", &self.socket)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        server.join().unwrap();
        output
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn exchange(kind: u32, payload: &str, reply: &str) -> (u32, String, u32, String) {
    (kind, payload.into(), kind, reply.into())
}

fn tree(focused: bool, scratchpad: bool) -> String {
    serde_json::json!({"id": 1, "type": "workspace",
        "name": if scratchpad { "__i3_scratch" } else { "2" },
        "nodes": [{"id": 42, "window": 99, "name": "Editor", "focused": focused}]
    })
    .to_string()
}

#[test]
fn searches_windows_through_ipc() {
    let server = Server::new();
    let result = server.run(
        &["windows", "editor"],
        vec![exchange(4, "", &tree(true, false))],
    );
    assert!(result.status.success(), "{:?}", result);
    let windows: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(windows[0]["id"], 42);
}

#[test]
fn focuses_then_verifies_the_target() {
    let result = Server::new().run(
        &["focus", "42"],
        vec![
            exchange(4, "", &tree(false, false)),
            exchange(0, "[con_id=42 id=99] focus", r#"[{"success":true}]"#),
            exchange(4, "", &tree(true, false)),
        ],
    );
    assert!(result.status.success(), "{:?}", result);
}

#[test]
fn scratchpad_is_revealed_before_focus() {
    let result = Server::new().run(
        &["focus", "42"],
        vec![
            exchange(4, "", &tree(false, true)),
            exchange(
                0,
                "[con_id=42 id=99] scratchpad show; [con_id=42 id=99] focus",
                r#"[{"success":true},{"success":true}]"#,
            ),
            exchange(4, "", &tree(true, false)),
        ],
    );
    assert!(result.status.success(), "{:?}", result);
}

#[test]
fn stale_window_is_rejected_without_sending_a_command() {
    let result = Server::new().run(&["focus", "123"], vec![exchange(4, "", &tree(true, false))]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no longer exists"));
}

#[test]
fn reports_command_failure() {
    let result = Server::new().run(
        &["focus", "42"],
        vec![
            exchange(4, "", &tree(false, false)),
            exchange(
                0,
                "[con_id=42 id=99] focus",
                r#"[{"success":false,"error":"test rejection"}]"#,
            ),
        ],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("test rejection"));
}

#[test]
fn successful_command_without_focus_is_not_reported_as_success() {
    let result = Server::new().run(
        &["focus", "42"],
        vec![
            exchange(4, "", &tree(false, false)),
            exchange(0, "[con_id=42 id=99] focus", r#"[{"success":true}]"#),
            exchange(4, "", &tree(false, false)),
        ],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("did not focus"));
}

#[test]
fn rejects_wrong_response_types_and_invalid_json() {
    for (reply_kind, reply) in [(3, "{}"), (4, "not JSON")] {
        let result = Server::new().run(
            &["windows"],
            vec![(4, String::new(), reply_kind, reply.into())],
        );
        assert!(!result.status.success());
    }
}

#[cfg(feature = "gui")]
#[test]
fn private_include_helper_handles_quoted_globs_without_executing_commands() {
    let server = Server::new();
    fs::write(server.directory.join("quoted file.conf"), b"").unwrap();
    let invoke = |expression: &str| {
        Command::new(env!("CARGO_BIN_EXE_lite-switch"))
            .arg("__expand-include")
            .arg(&server.directory)
            .arg(expression)
            .env_remove("DISPLAY")
            .output()
            .unwrap()
    };
    let output = invoke("*.conf");
    assert!(output.status.success());
    let paths: Vec<PathBuf> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(paths, vec![server.directory.join("quoted file.conf")]);
    assert!(invoke("'quoted file.conf'").status.success());
    let marker = server.directory.join("must-not-run");
    let output = invoke(&format!("$(touch {})", marker.display()));
    assert!(!output.status.success());
    assert!(!marker.exists());
}

#[test]
fn unavailable_socket_is_an_actionable_error() {
    let server = Server::new();
    let result = Command::new(env!("CARGO_BIN_EXE_lite-switch"))
        .arg("doctor")
        .env("I3SOCK", &server.socket)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot connect to i3"));
}
