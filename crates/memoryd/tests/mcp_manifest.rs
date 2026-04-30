use memoryd::mcp::{manifest, ToolName};

#[test]
fn mcp_manifest_declares_exact_agent_facing_tools_in_order() {
    let manifest = manifest();
    let names: Vec<_> = manifest.tools.iter().map(|tool| tool.name.as_str()).collect();

    assert_eq!(
        names,
        [
            "memory_search",
            "memory_get",
            "memory_write",
            "memory_supersede",
            "memory_forget",
            "memory_reveal",
            "memory_startup",
            "memory_note",
        ]
    );
    assert_eq!(manifest.tools.len(), 8);
}

#[test]
fn mcp_manifest_excludes_admin_tools_and_provides_descriptors() {
    let manifest = manifest();

    for admin_tool in [
        "memory_rollback",
        "memory_pin",
        "memory_unpin",
        "memory_policy",
        "memory_doctor",
        "memory_privacy_status",
        "memory_privacy_scan",
        "memory_privacy_filter_install",
        "memory_privacy_filter_enable",
        "memory_privacy_filter_disable",
        "memory_device_onboard",
        "memory_device_rotate_keys",
        "memory_device_revoke",
    ] {
        assert!(
            manifest.tools.iter().all(|tool| tool.name != admin_tool),
            "admin-only tool leaked into MCP manifest: {admin_tool}"
        );
    }

    for tool in &manifest.tools {
        assert!(!tool.description.trim().is_empty(), "{} needs a description", tool.name);
        assert!(tool.input_schema.is_object(), "{} needs an object input schema", tool.name);
        assert!(tool.output_schema.is_object(), "{} needs an object output schema", tool.name);
    }
}

#[test]
fn tool_name_conversion_accepts_only_manifest_tools() {
    for tool in manifest().tools {
        let parsed = ToolName::try_from(tool.name.as_str()).expect("manifest tool name parses");
        assert_eq!(parsed.as_str(), tool.name);
    }

    for unknown in [
        "memory_doctor",
        "memory_status",
        "search",
        "memory_delete",
        "memory_privacy_status",
        "memory_privacy_scan",
        "memory_privacy_filter_enable",
        "memory_device_onboard",
        "memory_device_rotate_keys",
        "memory_device_revoke",
    ] {
        assert!(ToolName::try_from(unknown).is_err(), "unexpected tool accepted: {unknown}");
    }
}

#[test]
fn mcp_manifest_memory_startup_requires_binding_context() {
    let manifest = manifest();
    let startup = manifest.tools.iter().find(|tool| tool.name == "memory_startup").expect("startup tool");

    assert_eq!(startup.input_schema["required"], serde_json::json!(["cwd", "session_id", "harness"]));
    for field in ["cwd", "session_id", "harness", "budget_tokens"] {
        assert!(startup.input_schema["properties"].get(field).is_some(), "missing startup field {field}");
    }
    assert!(startup.output_schema["properties"].get("recall_block").is_some());
}
