use std::path::Path;

use memoryd::mcp::{
    forward_payload_to_daemon, manifest, request_from_args, stdio_manifest, ObserveKindRequest, ToolName, ToolRequest,
};
use memoryd::protocol::{ObserveResponse, ObserveTarget, PeerHeartbeat, RequestPayload, ResponseResult};

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
            "memory_observe",
            "memory_capture_source",
        ]
    );
    assert_eq!(manifest.tools.len(), 10);
}

#[test]
fn mcp_stdio_manifest_hides_memory_reveal_unless_explicitly_allowed() {
    let default_stdio = stdio_manifest(false);
    assert!(
        default_stdio.tools.iter().all(|tool| tool.name != "memory_reveal"),
        "stdio MCP bridge must not expose memory_reveal by default"
    );
    assert_eq!(default_stdio.tools.len(), 9);

    let reveal_stdio = stdio_manifest(true);
    assert!(
        reveal_stdio.tools.iter().any(|tool| tool.name == "memory_reveal"),
        "--allow-reveal must restore the v1 reveal tool for explicit sessions"
    );
    assert_eq!(reveal_stdio.tools.len(), 10);
}

#[test]
fn mcp_get_output_schema_advertises_optional_provenance() {
    let manifest = manifest();
    let get = manifest.tools.iter().find(|tool| tool.name == "memory_get").expect("get tool");

    assert_eq!(get.output_schema["properties"]["provenance"]["type"], "object");
    assert_eq!(get.input_schema["properties"]["include_provenance"]["type"], "boolean");
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
        "memory_dream_now",
        "memory_dream_status",
        "memory_dream_enable",
        "memory_dream_disable",
        "memory_web_enable",
        "memory_web_disable",
        "memory_web_status",
        "memory_reality_check_run",
        "memory_reality_check_skip",
        "memory_reality_check_snooze",
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

#[tokio::test]
async fn mcp_forward_rejects_admin_web_payloads_before_socket_io() {
    for payload in [
        RequestPayload::WebEnable { port: 7137, socket_path: "/tmp/memoryd.sock".to_owned() },
        RequestPayload::WebDisable,
        RequestPayload::WebStatus,
    ] {
        let response =
            forward_payload_to_daemon(Path::new("/tmp/memoryd-definitely-missing.sock"), "mcp-test", payload)
                .await
                .expect("admin web payload rejection is local");

        match response.result {
            ResponseResult::Error(error) => assert_eq!(error.code, "method_not_allowed_on_mcp"),
            other => panic!("expected MCP method-not-allowed error, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn mcp_forward_rejects_peer_heartbeat_before_socket_io() {
    let payload = RequestPayload::PeerHeartbeat(PeerHeartbeat {
        session_id: "sess_mcp_forbidden".to_owned(),
        device_id: None,
        harness: "codex".to_owned(),
        project_binding: None,
        namespace: "project:agent-memory".to_owned(),
        salient_entities: Vec::new(),
        salient_paths: Vec::new(),
        capabilities: Vec::new(),
        started_at: None,
        claim_locks_held: Vec::new(),
    });
    let response = forward_payload_to_daemon(Path::new("/tmp/memoryd-definitely-missing.sock"), "mcp-test", payload)
        .await
        .expect("peer heartbeat rejection is local");

    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "method_not_allowed_on_mcp"),
        other => panic!("expected MCP method-not-allowed error, got {other:?}"),
    }
}

#[test]
fn mcp_manifest_memory_observe_schema_declares_stream_f_shape() {
    let manifest = manifest();
    let observe = manifest.tools.iter().find(|tool| tool.name == "memory_observe").expect("observe tool");

    assert_eq!(observe.input_schema["required"], serde_json::json!(["text", "kind"]));
    assert_eq!(observe.input_schema["additionalProperties"], serde_json::json!(false));
    assert_eq!(observe.input_schema["properties"]["text"]["type"], "string");
    assert_eq!(
        observe.input_schema["properties"]["kind"]["enum"],
        serde_json::json!(["observation", "pattern", "signal"])
    );
    assert_eq!(observe.input_schema["properties"]["entities"]["type"], "array");
    assert_eq!(observe.input_schema["properties"]["entities"]["maxItems"], 32);
    assert_eq!(observe.input_schema["properties"]["entities"]["items"]["type"], "string");
    assert_eq!(observe.input_schema["properties"]["entities"]["items"]["pattern"], "^ent_[A-Za-z0-9_.:-]{1,124}$");
    assert_eq!(observe.input_schema["properties"]["cwd"]["type"], "string");
    assert_eq!(observe.input_schema["properties"]["session_id"]["type"], "string");
    assert_eq!(observe.input_schema["properties"]["harness"]["type"], "string");
    assert_eq!(observe.input_schema["properties"]["harness_version"]["type"], "string");
}

#[test]
fn memory_observe_request_defaults_omitted_entities_to_empty_vec() {
    let request = request_from_args(
        ToolName::try_from("memory_observe").expect("observe tool name parses"),
        serde_json::json!({
            "text": "agent noticed recurring cache churn",
            "kind": "pattern",
            "cwd": "/tmp/project",
            "session_id": "sess_mcp",
            "harness": "codex"
        }),
    )
    .expect("valid observe request parses without entities");

    let ToolRequest::MemoryObserve(observe) = request else {
        panic!("expected MemoryObserve request");
    };
    assert!(observe.entities.is_empty());
}

#[test]
fn memory_observe_request_accepts_spec_shaped_args_without_binding_fields() {
    let request = request_from_args(
        ToolName::try_from("memory_observe").expect("observe tool name parses"),
        serde_json::json!({
            "text": "agent noticed recurring cache churn",
            "kind": "pattern"
        }),
    )
    .expect("spec-shaped observe request parses");

    let ToolRequest::MemoryObserve(observe) = request else {
        panic!("expected MemoryObserve request");
    };
    assert_eq!(observe.text, "agent noticed recurring cache churn");
    assert_eq!(observe.kind, ObserveKindRequest::Pattern);
    assert!(observe.entities.is_empty());
    assert!(!observe.cwd.is_empty());
    assert_eq!(observe.session_id, "synthetic-memory-observe");
    assert_eq!(observe.harness, "unknown");
    assert_eq!(observe.harness_version, None);
}

#[test]
fn mcp_manifest_memory_observe_output_schema_matches_observe_response() {
    let manifest = manifest();
    let observe = manifest.tools.iter().find(|tool| tool.name == "memory_observe").expect("observe tool");
    let required = observe.output_schema["required"].as_array().expect("required array");
    let response = serde_json::to_value(ObserveResponse {
        fragment_id: "sub_01HWPRZK1SPRAWM6EVQ6Y0XS8R".to_owned(),
        target: ObserveTarget::PlaintextSubstrate,
    })
    .expect("observe response serializes");

    assert_eq!(observe.output_schema["required"], serde_json::json!(["fragment_id", "target"]));
    for key in required {
        let key = key.as_str().expect("required key is string");
        assert!(response.get(key).is_some(), "schema requires {key}, but ObserveResponse omits it");
    }
    assert_eq!(
        observe.output_schema["properties"]["target"]["enum"],
        serde_json::json!(["plaintext_substrate", "encrypted_substrate"])
    );
}

#[test]
fn tool_name_conversion_accepts_memory_observe() {
    let parsed = ToolName::try_from("memory_observe").expect("memory_observe parses");

    assert_eq!(parsed.as_str(), "memory_observe");
}

#[test]
fn mcp_manifest_memory_capture_source_schema_has_modes() {
    let manifest = manifest();
    let tool = manifest.tools.iter().find(|tool| tool.name == "memory_capture_source").expect("capture tool");

    assert_eq!(tool.input_schema["required"], serde_json::json!(["source", "excerpts"]));
    assert_eq!(tool.input_schema["additionalProperties"], serde_json::json!(false));
    assert_eq!(
        tool.input_schema["properties"]["mode"]["enum"],
        serde_json::json!([
            "http_static",
            "local_artifact",
            "pdf_text",
            "browser_rendered",
            "screenshot",
            "authenticated"
        ])
    );
    assert!(tool.input_schema["properties"].get("local_path").is_some());
    assert!(tool.input_schema["properties"].get("allow_private_network").is_none());
    assert!(tool.input_schema["properties"].get("key_path").is_none());
    assert!(tool.input_schema["properties"].get("raw_key").is_none());
    assert!(tool.input_schema["properties"].get("auth").is_none());

    let request = request_from_args(
        ToolName::try_from("memory_capture_source").expect("capture tool parses"),
        serde_json::json!({
            "source": "local:evidence.md",
            "mode": "local_artifact",
            "local_path": "/tmp/evidence.md",
            "excerpts": ["exact quote"],
            "note": "safe note"
        }),
    )
    .expect("capture request parses");

    let ToolRequest::MemoryCaptureSource(args) = request else {
        panic!("expected MemoryCaptureSource request");
    };
    assert_eq!(args.source, "local:evidence.md");
    assert_eq!(args.mode, memoryd::protocol::CaptureSourceMode::LocalArtifact);
    assert_eq!(args.local_path.as_deref(), Some(Path::new("/tmp/evidence.md")));
    assert_eq!(args.excerpts, vec!["exact quote"]);
}

#[test]
fn mcp_capture_source_rejects_external_key_material_and_bypass_flags() {
    for field in ["key_path", "raw_key", "key_material", "allow_private_network", "privacy_bypass"] {
        let mut args = serde_json::json!({
            "source": "https://example.com/report",
            "excerpts": ["exact quote"]
        });
        args.as_object_mut().expect("object args").insert(field.to_string(), serde_json::json!(true));
        let error = request_from_args(ToolName::CaptureSource, args).expect_err("unsafe field must be rejected");
        assert!(error.to_string().contains(field), "unexpected error for {field}: {error}");
    }
}

#[test]
fn memory_note_rejects_kind_instead_of_becoming_observe() {
    let error = request_from_args(
        ToolName::Note,
        serde_json::json!({
            "text": "this is still a canonical note",
            "kind": "Observation"
        }),
    )
    .expect_err("memory_note must not accept observe-only kind");

    assert!(error.to_string().contains("unknown field `kind`"), "unexpected error for extra kind: {error}");
}

#[test]
fn mcp_tool_args_reject_unknown_fields_for_every_tool() {
    for (tool, args) in [
        (ToolName::Search, serde_json::json!({ "query": "cache", "extra": true })),
        (ToolName::Get, serde_json::json!({ "id": "mem_20260525_abcdef1234567890_000001", "extra": true })),
        (ToolName::Write, serde_json::json!({ "body": "alpha dogfood note", "extra": true })),
        (
            ToolName::Supersede,
            serde_json::json!({
                "old_id": "mem_20260525_abcdef1234567890_000001",
                "new_body": "replacement",
                "reason": "fix stale text",
                "extra": true
            }),
        ),
        (
            ToolName::Forget,
            serde_json::json!({
                "id": "mem_20260525_abcdef1234567890_000001",
                "reason": "user requested removal",
                "extra": true
            }),
        ),
        (
            ToolName::Reveal,
            serde_json::json!({
                "id": "mem_20260525_abcdef1234567890_000001",
                "reason": "user asked to reveal encrypted note",
                "extra": true
            }),
        ),
        (
            ToolName::Startup,
            serde_json::json!({ "cwd": "/tmp", "session_id": "sess", "harness": "codex", "extra": true }),
        ),
        (ToolName::Note, serde_json::json!({ "text": "dogfood note", "extra": true })),
        (ToolName::Observe, serde_json::json!({ "text": "signal", "kind": "signal", "extra": true })),
        (
            ToolName::CaptureSource,
            serde_json::json!({ "url": "https://example.com", "excerpts": ["quote"], "extra": true }),
        ),
    ] {
        let error = request_from_args(tool, args).expect_err("unknown fields must be rejected");
        assert!(
            error.to_string().contains("unknown field `extra`"),
            "{tool} accepted or misreported an extra field: {error}"
        );
    }
}

#[test]
fn mcp_tool_args_report_all_missing_required_fields_for_every_tool() {
    let manifest = manifest();
    for tool in ToolName::all() {
        let descriptor =
            manifest.tools.iter().find(|descriptor| descriptor.name == tool.as_str()).expect("tool exists");
        let required = descriptor.input_schema["required"].as_array().expect("required fields are declared");
        if required.is_empty() {
            continue;
        }

        let error = request_from_args(tool, serde_json::json!({})).expect_err("missing required fields reject");
        let message = error.to_string();
        assert!(message.contains(tool.as_str()), "error must name {tool}: {message}");
        assert!(message.contains("required shape"), "error must describe required shape for {tool}: {message}");
        for field in required {
            let field = field.as_str().expect("required field name");
            assert!(message.contains(field), "error for {tool} must name missing field {field}: {message}");
        }
    }
}

#[test]
fn memory_observe_request_validates_entities_shape() {
    let request = request_from_args(
        ToolName::try_from("memory_observe").expect("observe tool name parses"),
        serde_json::json!({
            "text": "agent noticed recurring cache churn",
            "kind": "pattern",
            "entities": [
                "ent_cache",
                "ent_agent_memory"
            ],
            "cwd": "/tmp/project",
            "session_id": "sess_mcp",
            "harness": "codex",
            "harness_version": "0.0.0"
        }),
    )
    .expect("valid observe request parses");

    let ToolRequest::MemoryObserve(observe) = request else {
        panic!("expected MemoryObserve request");
    };
    assert_eq!(observe.text, "agent noticed recurring cache churn");
    assert_eq!(observe.kind, ObserveKindRequest::Pattern);
    assert_eq!(observe.entities.len(), 2);
    assert_eq!(observe.entities[0], "ent_cache");
    assert_eq!(observe.cwd, "/tmp/project");
    assert_eq!(observe.session_id, "sess_mcp");
    assert_eq!(observe.harness, "codex");
    assert_eq!(observe.harness_version.as_deref(), Some("0.0.0"));

    let error = request_from_args(
        ToolName::try_from("memory_observe").expect("observe tool name parses"),
        serde_json::json!({
            "text": "entity entries must be ids",
            "kind": "signal",
            "entities": [{ "id": "ent_cache" }],
            "cwd": "/tmp/project",
            "session_id": "sess_mcp",
            "harness": "codex"
        }),
    )
    .expect_err("entities must be strings");
    assert!(error.to_string().contains("invalid type"), "unexpected entity error: {error}");
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
