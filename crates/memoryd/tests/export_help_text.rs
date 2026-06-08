//! B5 regression: `memoryd export --help` must surface the substrate-open
//! side effects per spec §7 MUST.
//!
//! Spec §7: "Acknowledge the substrate open side effects in the user-facing
//! docs (README / help text), since runtime-dir creation, index repair
//! replay, and event-log mirror rebuild are NOT no-ops even though the
//! export does not write substrate content."
//!
//! Without this test, a future edit could silently drop the side-effects
//! note from the doc comment and the contract would silently regress.

use std::process::Command;

#[test]
fn export_help_mentions_substrate_open_side_effects() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["export", "--help"])
        .output()
        .expect("spawn memoryd export --help");

    assert!(
        output.status.success(),
        "memoryd export --help must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");

    // The doc comment on `Command::Export` must surface in the long help.
    // These are the three concrete side effects spec §7 names; the help
    // text must reference each so an operator reading --help understands
    // what `memoryd export` actually does to the runtime directory.
    for phrase in &["runtime-dir creation", "index-repair replay", "event-log mirror rebuild"] {
        assert!(
            stdout.contains(phrase),
            "memoryd export --help must mention '{phrase}' (B5 spec §7); full help output:\n{stdout}"
        );
    }

    // The help must also tell the operator to stop `memoryd serve` against
    // the same --repo/--runtime pair before exporting (spec §2 non-goal).
    assert!(
        stdout.contains("memoryd serve"),
        "memoryd export --help must mention the serve/export interaction; full help:\n{stdout}"
    );
}
