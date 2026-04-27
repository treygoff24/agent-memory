#[test]
fn workspace_exposes_substrate_version() {
    assert_eq!(memory_substrate::STREAM_A_SPEC_VERSION, "1.1");
}
