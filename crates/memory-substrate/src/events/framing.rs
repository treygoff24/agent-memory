//! CRC-framed JSONL event encoding per spec §12.1.
//!
//! The CRC32C checksum is a field *inside* the JSON object, not an out-of-band
//! prefix. Any consumer can verify integrity with a standard JSON parser.

use serde_json::Value;

/// Maximum byte length of a single event line (spec §12.3 step 1).
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Encode an event payload as a JSON line with an embedded `crc32c` field.
///
/// The checksum covers the serialized event with `crc32c` set to zero, per
/// spec §12.1.
///
/// Returns `Err` if serialization fails or the resulting line exceeds 64 KiB.
pub fn encode_event_line(value: &Value) -> Result<String, EventFramingError> {
    let mut obj = match value.as_object().cloned() {
        Some(map) => map,
        None => {
            return Err(EventFramingError::NotAnObject);
        }
    };

    obj.insert("crc32c".to_string(), Value::Number(0.into()));
    let placeholder_json = serde_json::to_string(&obj).map_err(EventFramingError::Serialize)?;

    let checksum = crc32c::crc32c(placeholder_json.as_bytes());

    obj.insert("crc32c".to_string(), Value::Number(checksum.into()));
    let final_json = serde_json::to_string(&obj).map_err(EventFramingError::Serialize)?;

    let line = format!("{final_json}\n");
    if line.len() > MAX_LINE_BYTES {
        return Err(EventFramingError::LineTooLong { byte_len: line.len() });
    }
    Ok(line)
}

/// Decode and verify one event line.
///
/// Returns `None` if the line is malformed, fails CRC, or exceeds 64 KiB.
pub fn decode_line(line: &str) -> Option<Value> {
    if line.len() > MAX_LINE_BYTES {
        return None;
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return None;
    }
    let mut obj: serde_json::Map<String, Value> = serde_json::from_str(trimmed).ok()?;

    let stored_crc = obj.get("crc32c")?.as_u64()? as u32;

    obj.insert("crc32c".to_string(), Value::Number(0.into()));
    let placeholder_json = serde_json::to_string(&obj).expect("parsed JSON object serializes"); // expect-justified: parsed JSON
    let expected = crc32c::crc32c(placeholder_json.as_bytes());

    if stored_crc != expected {
        return None;
    }

    obj.insert("crc32c".to_string(), Value::Number(stored_crc.into()));
    Some(Value::Object(obj))
}

/// Errors from event framing.
#[derive(Debug, thiserror::Error)]
pub enum EventFramingError {
    /// Event value is not a JSON object.
    #[error("event value must be a JSON object")]
    NotAnObject,
    /// JSON serialization failed.
    #[error("event serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Line exceeds the 64-KiB limit (spec §12.3 step 1).
    #[error("event line too long: {byte_len} bytes (max {max})", max = MAX_LINE_BYTES)]
    LineTooLong {
        /// Byte length of the rejected line.
        byte_len: usize,
    },
}
