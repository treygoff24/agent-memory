use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::client;
pub use crate::protocol::ObserveKind as ObserveKindRequest;
use crate::protocol::{
    default_observe_cwd, default_observe_harness, default_observe_session_id, CaptureSourceMode, ObserveKind,
    RequestPayload, ResponseEnvelope, ResponseResult, SourceCapturePayload,
};
pub use crate::recall::StartupRequest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub tools: Vec<ToolDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolName {
    Search,
    Get,
    Write,
    Supersede,
    Forget,
    Reveal,
    Startup,
    Note,
    Observe,
    CaptureSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownToolName {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolRequest {
    MemorySearch(SearchRequest),
    MemoryGet(GetRequest),
    MemoryWrite(WriteRequest),
    MemorySupersede(SupersedeRequest),
    MemoryForget(ForgetRequest),
    MemoryReveal(RevealRequest),
    MemoryStartup(StartupRequest),
    MemoryNote(NoteRequest),
    MemoryObserve(ObserveRequest),
    MemoryCaptureSource(CaptureSourceRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_body: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetRequest {
    pub id: String,
    #[serde(default)]
    pub include_provenance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteRequest {
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default = "null_value")]
    pub meta: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersedeRequest {
    pub old_id: String,
    pub new_body: String,
    pub reason: String,
    #[serde(default = "null_value")]
    pub meta: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgetRequest {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevealRequest {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveRequest {
    pub text: String,
    pub kind: ObserveKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    #[serde(default = "default_observe_cwd")]
    pub cwd: String,
    #[serde(default = "default_observe_session_id")]
    pub session_id: String,
    #[serde(default = "default_observe_harness")]
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSourceRequest {
    #[serde(alias = "url")]
    pub source: String,
    #[serde(default)]
    pub mode: CaptureSourceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
    pub excerpts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub fn manifest() -> Manifest {
    debug_assert_eq!(ToolName::all().len(), 10, "MCP v1 manifest tool count changed without a spec bump");
    manifest_from_tools(ToolName::all())
}

pub fn stdio_manifest(allow_reveal: bool) -> Manifest {
    Manifest {
        tools: ToolName::all()
            .into_iter()
            .filter(|name| allow_reveal || *name != ToolName::Reveal)
            .map(descriptor)
            .collect(),
    }
}

fn manifest_from_tools<const N: usize>(tools: [ToolName; N]) -> Manifest {
    Manifest { tools: tools.into_iter().map(descriptor).collect() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArgumentsError {
    tool: ToolName,
    message: String,
}

impl ToolArgumentsError {
    fn new(tool: ToolName, message: impl Into<String>) -> Self {
        Self { tool, message: message.into() }
    }
}

impl fmt::Display for ToolArgumentsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} arguments {}", self.tool, self.message)
    }
}

impl std::error::Error for ToolArgumentsError {}

pub fn request_from_args(name: ToolName, args: Value) -> Result<ToolRequest, ToolArgumentsError> {
    validate_required_arguments(name, &args)?;
    match name {
        ToolName::Search => deserialize_tool_args(name, args).map(ToolRequest::MemorySearch),
        ToolName::Get => deserialize_tool_args(name, args).map(ToolRequest::MemoryGet),
        ToolName::Write => deserialize_tool_args(name, args).map(ToolRequest::MemoryWrite),
        ToolName::Supersede => deserialize_tool_args(name, args).map(ToolRequest::MemorySupersede),
        ToolName::Forget => deserialize_tool_args(name, args).map(ToolRequest::MemoryForget),
        ToolName::Reveal => deserialize_tool_args(name, args).map(ToolRequest::MemoryReveal),
        ToolName::Startup => deserialize_tool_args(name, args).map(ToolRequest::MemoryStartup),
        ToolName::Note => deserialize_tool_args(name, args).map(ToolRequest::MemoryNote),
        ToolName::Observe => deserialize_tool_args(name, args).map(ToolRequest::MemoryObserve),
        ToolName::CaptureSource => deserialize_tool_args(name, args).map(ToolRequest::MemoryCaptureSource),
    }
}

fn validate_required_arguments(name: ToolName, args: &Value) -> Result<(), ToolArgumentsError> {
    let required_shape = required_argument_shape(name);
    let Some(arguments) = args.as_object() else {
        return Err(ToolArgumentsError::new(name, format!("must be a JSON object; required shape: {required_shape}")));
    };

    let missing_fields = required_argument_fields(name)
        .into_iter()
        .filter(|field| !argument_object_contains_required_field(name, arguments, field))
        .collect::<Vec<_>>();

    if missing_fields.is_empty() {
        return Ok(());
    }

    Err(ToolArgumentsError::new(
        name,
        format!("missing required fields: {}; required shape: {required_shape}", missing_fields.join(", ")),
    ))
}

fn deserialize_tool_args<T>(name: ToolName, args: Value) -> Result<T, ToolArgumentsError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(args).map_err(|error| {
        ToolArgumentsError::new(
            name,
            format!("are invalid: {error}; required shape: {}", required_argument_shape(name)),
        )
    })
}

fn argument_object_contains_required_field(name: ToolName, arguments: &Map<String, Value>, field: &str) -> bool {
    arguments.contains_key(field)
        || matches!((name, field), (ToolName::CaptureSource, "source")) && arguments.contains_key("url")
}

fn required_argument_fields(name: ToolName) -> Vec<String> {
    let descriptor = descriptor(name);
    descriptor
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn required_argument_shape(name: ToolName) -> String {
    let descriptor = descriptor(name);
    let properties = descriptor.input_schema.get("properties").and_then(Value::as_object);
    let fields = required_argument_fields(name)
        .into_iter()
        .map(|field| {
            let field_type = properties
                .and_then(|properties| properties.get(&field))
                .and_then(|property| property.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("value");
            format!("{field}: {field_type}")
        })
        .collect::<Vec<_>>();

    format!("{{ {} }}", fields.join(", "))
}

/// Forward an MCP `ToolRequest` to the memoryd daemon.
///
/// Implemented mappings:
///   `MemorySearch`  → `RequestPayload::Search`
///   `MemoryGet`     → `RequestPayload::Get`
///   `MemoryWrite`   → `RequestPayload::WriteMemory`
///   `MemorySupersede` → `RequestPayload::Supersede`
///   `MemoryForget`  → `RequestPayload::Forget`
///   `MemoryReveal`  → `RequestPayload::Reveal`
///   `MemoryNote`    → `RequestPayload::WriteNote`
///   `MemoryObserve` → `RequestPayload::Observe`
///   `MemoryStartup` → `RequestPayload::Startup`
///
/// Admin/UI daemon payloads are rejected before socket I/O. Stream G adds
/// `RealityCheck` and trust artifact lookup; Stream I peer-state payloads and
/// Stream H test-injection payloads will reuse the same
/// `method_not_allowed_on_mcp` error.
pub async fn forward_to_daemon(
    socket_path: &Path,
    id: impl Into<String>,
    request: ToolRequest,
) -> Result<ResponseEnvelope> {
    let id = id.into();
    let payload = match request {
        ToolRequest::MemorySearch(args) => RequestPayload::Search {
            query: args.query,
            limit: args.limit.map(|n| n as usize),
            include_body: args.include_body,
        },
        ToolRequest::MemoryGet(args) => {
            RequestPayload::Get { id: args.id, include_provenance: args.include_provenance, full_body: false }
        }
        ToolRequest::MemoryWrite(args) => RequestPayload::WriteMemory {
            body: args.body,
            title: args.title,
            tags: args.tags,
            meta: meta_with_current_cwd_if_missing(args.meta)?,
        },
        ToolRequest::MemoryNote(args) => RequestPayload::WriteNote { text: args.text, meta: Value::Null },
        ToolRequest::MemoryObserve(args) => RequestPayload::Observe {
            text: args.text,
            kind: args.kind,
            entities: args.entities,
            cwd: args.cwd,
            session_id: args.session_id,
            harness: args.harness,
            harness_version: args.harness_version,
        },
        ToolRequest::MemorySupersede(args) => RequestPayload::Supersede {
            old_id: args.old_id,
            content: args.new_body,
            reason: args.reason,
            meta: meta_with_current_cwd_if_missing(args.meta)?,
        },
        ToolRequest::MemoryForget(args) => RequestPayload::Forget { id: args.id, reason: args.reason },
        ToolRequest::MemoryReveal(args) => RequestPayload::Reveal { id: args.id, reason: args.reason },
        ToolRequest::MemoryStartup(args) => RequestPayload::Startup(args),
        ToolRequest::MemoryCaptureSource(args) => RequestPayload::CaptureSource(SourceCapturePayload {
            source: args.source,
            mode: args.mode,
            excerpts: args.excerpts,
            note: args.note,
            local_path: args.local_path,
        }),
    };

    forward_payload_to_daemon(socket_path, id, payload).await
}

pub(crate) fn meta_with_current_cwd_if_missing(meta: Value) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    Ok(meta_with_cwd_if_missing(meta, &cwd))
}

fn meta_with_cwd_if_missing(meta: Value, cwd: &Path) -> Value {
    let cwd = Value::String(cwd.to_string_lossy().into_owned());
    match meta {
        Value::Null => json!({ "cwd": cwd }),
        Value::Object(mut fields) => {
            fields.entry("cwd".to_string()).or_insert(cwd);
            Value::Object(fields)
        }
        other => other,
    }
}

pub async fn forward_payload_to_daemon(
    socket_path: &Path,
    id: impl Into<String>,
    payload: RequestPayload,
) -> Result<ResponseEnvelope> {
    let id = id.into();
    match payload {
        RequestPayload::TrustArtifact { .. }
        | RequestPayload::WebEnable { .. }
        | RequestPayload::WebDisable
        | RequestPayload::WebStatus
        | RequestPayload::RealityCheck(_)
        | RequestPayload::InspectEntities { .. }
        | RequestPayload::EventsLogPage { .. }
        | RequestPayload::NamespaceTree { .. }
        | RequestPayload::GovernancePolicyDump
        | RequestPayload::ConflictsList { .. }
        | RequestPayload::QuarantineResolve { .. }
        | RequestPayload::PeerHeartbeat(_)
        | RequestPayload::PeerStatus
        | RequestPayload::PeerActivity { .. }
        | RequestPayload::PeerReleaseLock { .. } => Ok(ResponseEnvelope {
            id,
            result: ResponseResult::Error(crate::protocol::ProtocolError::method_not_allowed_on_mcp()),
        }),
        payload => client::request(socket_path, id, payload).await,
    }
}

impl ToolName {
    pub const fn all() -> [Self; 10] {
        [
            Self::Search,
            Self::Get,
            Self::Write,
            Self::Supersede,
            Self::Forget,
            Self::Reveal,
            Self::Startup,
            Self::Note,
            Self::Observe,
            Self::CaptureSource,
        ]
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Search => "memory_search",
            Self::Get => "memory_get",
            Self::Write => "memory_write",
            Self::Supersede => "memory_supersede",
            Self::Forget => "memory_forget",
            Self::Reveal => "memory_reveal",
            Self::Startup => "memory_startup",
            Self::Note => "memory_note",
            Self::Observe => "memory_observe",
            Self::CaptureSource => "memory_capture_source",
        }
    }
}

impl TryFrom<&str> for ToolName {
    type Error = UnknownToolName;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "memory_search" => Ok(Self::Search),
            "memory_get" => Ok(Self::Get),
            "memory_write" => Ok(Self::Write),
            "memory_supersede" => Ok(Self::Supersede),
            "memory_forget" => Ok(Self::Forget),
            "memory_reveal" => Ok(Self::Reveal),
            "memory_startup" => Ok(Self::Startup),
            "memory_note" => Ok(Self::Note),
            "memory_observe" => Ok(Self::Observe),
            "memory_capture_source" => Ok(Self::CaptureSource),
            name => Err(UnknownToolName { name: name.to_owned() }),
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Display for UnknownToolName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown MCP tool `{}`", self.name)
    }
}

impl std::error::Error for UnknownToolName {}

fn descriptor(name: ToolName) -> ToolDescriptor {
    let (description, input_schema, output_schema) = match name {
        ToolName::Search => (
            "Search agent-facing memory records with bounded snippets.",
            object_schema(&[("query", "string"), ("limit", "integer"), ("include_body", "boolean")], &["query"]),
            object_schema(
                &[("hits", "array"), ("total", "integer"), ("guidance", "string")],
                &["hits", "total", "guidance"],
            ),
        ),
        ToolName::Get => (
            "Read one memory by id with an optional provenance envelope.",
            object_schema(&[("id", "string"), ("include_provenance", "boolean")], &["id"]),
            object_schema(
                &[("id", "string"), ("body", "string"), ("truncated", "boolean"), ("provenance", "object")],
                &["id", "body", "truncated"],
            ),
        ),
        ToolName::Write => (
            "Write a new durable memory from structured agent input.",
            object_schema(&[("body", "string"), ("title", "string"), ("tags", "array"), ("meta", "object")], &["body"]),
            object_schema(
                &[("status", "string"), ("id", "string"), ("reason", "string"), ("next_actions", "array")],
                &["status"],
            ),
        ),
        ToolName::Supersede => (
            "Supersede an existing memory with replacement content and a reason.",
            object_schema(
                &[("old_id", "string"), ("new_body", "string"), ("reason", "string"), ("meta", "object")],
                &["old_id", "new_body", "reason"],
            ),
            object_schema(&[("status", "string"), ("old_id", "string"), ("new_id", "string")], &["status"]),
        ),
        ToolName::Forget => (
            "Forget a memory through the agent-facing tombstone path.",
            object_schema(&[("id", "string"), ("reason", "string")], &["id", "reason"]),
            object_schema(&[("status", "string"), ("id", "string"), ("tombstone_ref", "string")], &["status", "id"]),
        ),
        ToolName::Reveal => (
            "Explicitly reveal encrypted content by id with a user-directed reason.",
            object_schema(&[("id", "string"), ("reason", "string")], &["id", "reason"]),
            object_schema(
                &[("id", "string"), ("summary", "string"), ("body", "string"), ("truncated", "boolean")],
                &["id", "body", "truncated"],
            ),
        ),
        ToolName::Startup => (
            "Return startup memory context for a new agent session.",
            object_schema(
                &[
                    ("cwd", "string"),
                    ("session_id", "string"),
                    ("harness", "string"),
                    ("harness_version", "string"),
                    ("include_recent", "boolean"),
                    ("since_event_id", "string"),
                    ("budget_tokens", "integer"),
                ],
                &["cwd", "session_id", "harness"],
            ),
            object_schema(
                &[
                    ("session_binding", "object"),
                    ("recall_block", "string"),
                    ("budget_used_tokens", "integer"),
                    ("recall_explanation", "object"),
                    ("guidance", "string"),
                ],
                &["session_binding", "recall_block", "budget_used_tokens", "recall_explanation", "guidance"],
            ),
        ),
        ToolName::Note => (
            "Capture a lightweight note without exposing admin controls.",
            object_schema(&[("text", "string")], &["text"]),
            object_schema(&[("id", "string"), ("summary", "string")], &["id", "summary"]),
        ),
        ToolName::Observe => (
            "Capture low-level Stream F substrate telemetry without creating a canonical memory.",
            observe_input_schema(),
            observe_output_schema(),
        ),
        ToolName::CaptureSource => (
            "Capture a public HTTP(S) source as a local verified webcap artifact before writing a grounded memory.",
            capture_source_input_schema(),
            object_schema(
                &[
                    ("artifact_id", "string"),
                    ("source_refs", "array"),
                    ("mode", "string"),
                    ("final_url", "string"),
                    ("captured_at", "string"),
                    ("capture_status", "string"),
                    ("warnings", "array"),
                ],
                &["artifact_id", "source_refs", "mode", "final_url", "captured_at", "capture_status", "warnings"],
            ),
        ),
    };

    ToolDescriptor { name: name.as_str().to_owned(), description: description.to_owned(), input_schema, output_schema }
}

fn object_schema(properties: &[(&str, &str)], required: &[&str]) -> Value {
    let properties = properties
        .iter()
        .map(|(name, property_type)| ((*name).to_owned(), json!({ "type": property_type })))
        .collect::<serde_json::Map<_, _>>();

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn observe_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text": { "type": "string", "minLength": 1, "maxLength": 16384 },
            "kind": { "type": "string", "enum": ["observation", "pattern", "signal"] },
            "entities": {
                "type": "array",
                "maxItems": 32,
                "items": {
                    "type": "string",
                    "minLength": 5,
                    "maxLength": 128,
                    "pattern": "^ent_[A-Za-z0-9_.:-]{1,124}$"
                }
            },
            "cwd": { "type": "string" },
            "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "harness": { "type": "string", "minLength": 1, "maxLength": 128 },
            "harness_version": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["text", "kind"],
        "additionalProperties": false,
    })
}

fn observe_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "fragment_id": { "type": "string" },
            "target": { "type": "string", "enum": ["plaintext_substrate", "encrypted_substrate"] }
        },
        "required": ["fragment_id", "target"],
        "additionalProperties": false,
    })
}

fn capture_source_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source": { "type": "string", "minLength": 1 },
            "mode": {
                "type": "string",
                "enum": [
                    "http_static",
                    "local_artifact",
                    "pdf_text",
                    "browser_rendered",
                    "screenshot",
                    "authenticated"
                ]
            },
            "local_path": { "type": "string", "minLength": 1 },
            "excerpts": {
                "type": "array",
                "minItems": 1,
                "maxItems": 8,
                "items": { "type": "string", "minLength": 1, "maxLength": 2048 }
            },
            "note": { "type": "string", "maxLength": 2048 }
        },
        "required": ["source", "excerpts"],
        "additionalProperties": false,
    })
}

fn null_value() -> Value {
    Value::Null
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::meta_with_cwd_if_missing;

    #[test]
    fn memory_write_meta_without_cwd_gets_bridge_cwd_injected() {
        let meta = meta_with_cwd_if_missing(json!({ "namespace": "project" }), Path::new("/tmp/memorum-project"));

        assert_eq!(meta["namespace"], "project");
        assert_eq!(meta["cwd"], "/tmp/memorum-project");
    }

    #[test]
    fn memory_write_meta_absent_gets_bridge_cwd_injected() {
        let meta = meta_with_cwd_if_missing(serde_json::Value::Null, Path::new("/tmp/memorum-project"));

        assert_eq!(meta, json!({ "cwd": "/tmp/memorum-project" }));
    }

    #[test]
    fn caller_supplied_cwd_is_preserved() {
        let meta = meta_with_cwd_if_missing(
            json!({ "namespace": "project", "cwd": "/caller/supplied" }),
            Path::new("/tmp/memorum-project"),
        );

        assert_eq!(meta["cwd"], "/caller/supplied");
    }
}
