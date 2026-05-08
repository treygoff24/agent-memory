use std::process::Command;

// `--inject-panic` only exists under `#[cfg(debug_assertions)]`. In release
// builds clap rejects the unknown flag with exit code 2 and a stderr that does
// not contain the panic message, so the assertion below would fail for the
// wrong reason. Gate the test to match the production scope of the flag.
#[cfg(debug_assertions)]
#[test]
fn pre_run_inject_panic_flag_invokes_hook_before_default_hook() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd-tui"))
        .arg("--inject-panic")
        .output()
        .expect("spawn memoryd-tui --inject-panic");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("injected memoryd-tui panic"));
}

// `--inject-panic-mid-render` only exists under `#[cfg(debug_assertions)]`. In a
// release-profile test build the binary would reject the flag, the
// `!status.success()` assertion would pass for the wrong reason, and the
// `find("\u{1b}[?1049h").expect(...)` below would panic on `None`. Gate the test
// to match the production scope of the flag.
#[cfg(debug_assertions)]
#[test]
fn mid_render_panic_restores_alternate_screen_after_tui_enters_it() {
    use std::io::Read;

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).expect("open pty");
    let mut reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_memoryd-tui"));
    command.arg("--inject-panic-mid-render");

    let mut child = pair.slave.spawn_command(command).expect("spawn memoryd-tui in pty");
    drop(pair.slave);

    // Drain the PTY master concurrently. A rendered TUI frame is large enough to fill the
    // PTY pipe buffer, and if we don't drain while the child is alive the child blocks on
    // stdout writes and wait() deadlocks.
    let reader_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let status = child.wait().expect("wait for injected panic");
    assert!(!status.success(), "injected panic should fail the process");

    let output = reader_handle.join().expect("pty reader thread");

    let enter = output.find("\u{1b}[?1049h").expect("TUI should enter alternate screen before panic");
    let leave = output.find("\u{1b}[?1049l").expect("panic hook should leave alternate screen");
    assert!(leave > enter, "alternate screen restore should happen after enter sequence");
    assert!(output.contains("injected memoryd-tui mid-render panic"));
}

#[test]
fn inject_panic_flags_are_hidden_from_help_in_debug_builds() {
    let output =
        Command::new(env!("CARGO_BIN_EXE_memoryd-tui")).arg("--help").output().expect("spawn memoryd-tui --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("inject-panic"));
    assert!(!stdout.contains("inject-panic-mid-render"));
}
