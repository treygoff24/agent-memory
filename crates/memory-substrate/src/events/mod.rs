//! Recoverable JSONL event log.

pub(crate) mod framing;
pub(crate) mod log;
pub(crate) mod recovery;
pub(crate) mod sequence;

pub use framing::{decode_line, encode_event_line, EventFramingError, MAX_LINE_BYTES};
pub use log::{
    append_event, read_events, read_events_strict, refuse_duplicate_device_logs, CommitOutcome, Event, EventKind,
    EventLogError, EVENT_SCHEMA_VERSION,
};
pub use recovery::recover_event_log;
pub(crate) use sequence::{reserve_event_sequence, stamp_event_sequence, sync_event_sequence_state};
