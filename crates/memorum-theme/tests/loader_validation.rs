use memorum_theme::{Loader, LoaderError};

#[test]
fn loader_resolves_default_and_rejects_unknown_preset() {
    assert_eq!(Loader::resolve(Some("default-warm-dark"), None).expect("default loads").name, "default-warm-dark");
    assert!(
        matches!(Loader::resolve(Some("nonexistent"), None), Err(LoaderError::UnknownPreset(name)) if name == "nonexistent")
    );
}

#[test]
fn loader_reports_missing_and_unknown_tokens() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing.toml");
    std::fs::write(&missing, "name = 'broken'\n[colors]\nbg = 'oklch(0.1 0.1 0)'\n").expect("write missing");
    assert!(
        matches!(Loader::resolve(None, Some(&missing)), Err(LoaderError::MissingToken(token)) if token == "surface")
    );

    let unknown = temp.path().join("unknown.toml");
    let mut body = include_str!("../src/presets/default_warm_dark.toml").to_string();
    body.push_str("extra = 'bad'\n");
    std::fs::write(&unknown, body).expect("write unknown");
    assert!(matches!(Loader::resolve(None, Some(&unknown)), Err(LoaderError::ParseFailed(_))));
}
