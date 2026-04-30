use std::fmt;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client;
use crate::protocol::{RequestPayload, ResponseEnvelope};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_body: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetRequest {
    pub id: String,
    #[serde(default)]
    pub include_provenance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct SupersedeRequest {
    pub old_id: String,
    pub new_body: String,
    pub reason: String,
    #[serde(default = "null_value")]
    pub meta: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgetRequest {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealRequest {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupRequest {
    #[serde(default)]
    pub include_recent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRequest {
    pub text: String,
}

pub fn manifest() -> Manifest {
    Manifest { tools: ToolName::all().iter().map(|name| descriptor(*name)).collect() }
}

pub fn request_from_args(name: ToolName, args: Value) -> Result<ToolRequest, serde_json::Error> {
    match name {
        ToolName::Search => serde_json::from_value(args).map(ToolRequest::MemorySearch),
        ToolName::Get => serde_json::from_value(args).map(ToolRequest::MemoryGet),
        ToolName::Write => serde_json::from_value(args).map(ToolRequest::MemoryWrite),
        ToolName::Supersede => serde_json::from_value(args).map(ToolRequest::MemorySupersede),
        ToolName::Forget => serde_json::from_value(args).map(ToolRequest::MemoryForget),
        ToolName::Reveal => serde_json::from_value(args).map(ToolRequest::MemoryReveal),
        ToolName::Startup => serde_json::from_value(args).map(ToolRequest::MemoryStartup),
        ToolName::Note => serde_json::from_value(args).map(ToolRequest::MemoryNote),
    }
}

pub fn args_from_request(request: &ToolRequest) -> Result<Value, serde_json::Error> {
    match request {
        ToolRequest::MemorySearch(args) => serde_json::to_value(args),
        ToolRequest::MemoryGet(args) => serde_json::to_value(args),
        ToolRequest::MemoryWrite(args) => serde_json::to_value(args),
        ToolRequest::MemorySupersede(args) => serde_json::to_value(args),
        ToolRequest::MemoryForget(args) => serde_json::to_value(args),
        ToolRequest::MemoryReveal(args) => serde_json::to_value(args),
        ToolRequest::MemoryStartup(args) => serde_json::to_value(args),
        ToolRequest::MemoryNote(args) => serde_json::to_value(args),
    }
}

/// Forward an MCP `ToolRequest` to the memoryd daemon, or return a structured
/// `NotImplemented` response for tools that are not yet wired up.
///
/// Implemented mappings:
///   `MemorySearch`  → `RequestPayload::Search`
///   `MemoryGet`     → `RequestPayload::Get`
///   `MemoryWrite`   → `RequestPayload::WriteMemory`
///   `MemorySupersede` → `RequestPayload::Supersede`
///   `MemoryForget`  → `RequestPayload::Forget`
///   `MemoryReveal`  → `RequestPayload::Reveal`
///   `MemoryNote`    → `RequestPayload::WriteNote`
///
/// Not-yet-implemented (returns structured error without contacting daemon):
///   `MemoryStartup`
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
            RequestPayload::Get { id: args.id, include_provenance: args.include_provenance }
        }
        ToolRequest::MemoryWrite(args) => {
            RequestPayload::WriteMemory { body: args.body, title: args.title, tags: args.tags, meta: args.meta }
        }
        ToolRequest::MemoryNote(args) => RequestPayload::WriteNote { text: args.text },
        ToolRequest::MemorySupersede(args) => RequestPayload::Supersede {
            old_id: args.old_id,
            content: args.new_body,
            reason: args.reason,
            meta: args.meta,
        },
        ToolRequest::MemoryForget(args) => RequestPayload::Forget { id: args.id, reason: args.reason },
        ToolRequest::MemoryReveal(args) => RequestPayload::Reveal { id: args.id, reason: args.reason },
        ToolRequest::MemoryStartup(_) => {
            return Ok(ResponseEnvelope::error(
                id,
                "not_implemented",
                "memory_startup is not yet implemented; planned for Stream E",
                false,
            ));
        }
    };

    client::request(socket_path, id, payload).await
}

impl ToolName {
    pub const fn all() -> [Self; 8] {
        [Self::Search, Self::Get, Self::Write, Self::Supersede, Self::Forget, Self::Reveal, Self::Startup, Self::Note]
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
                &[("id", "string"), ("body", "string"), ("truncated", "boolean")],
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
            object_schema(&[("include_recent", "boolean")], &[]),
            object_schema(&[("items", "array"), ("guidance", "string")], &["items", "guidance"]),
        ),
        ToolName::Note => (
            "Capture a lightweight note without exposing admin controls.",
            object_schema(&[("text", "string")], &["text"]),
            object_schema(&[("id", "string"), ("summary", "string")], &["id", "summary"]),
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

fn null_value() -> Value {
    Value::Null
}
