//! CLI-side first-write success signal.
//!
//! After a successful `memoryd write` or `memoryd write-note`, the CLI issues a
//! follow-up `Status` query and emits a one-shot banner to stderr if this looks
//! like the user's very first memory landing in the substrate. The detection is
//! purely client-side; the daemon does not know whether it just served the first
//! ever write or the 10,000th. Race condition is acknowledged in the plan: if a
//! concurrent peer write lands between our `WriteMemory` response and our
//! `Status` query, the banner could mis-fire or miss. Acceptable on a fresh
//! install where concurrent writes are unlikely.

use std::io::{self, Write};

use crate::protocol::StatusResponse;

/// Returns `true` when the post-write `StatusResponse` indicates this was the
/// user's first memory: the substrate index now reports exactly one active
/// memory. Survives daemon restart because the counter is persisted in the
/// SQLite index, not in process memory.
pub fn should_emit_first_write_banner(status: &StatusResponse) -> bool {
    status.index_stats.as_ref().is_some_and(|stats| stats.active_memories == 1)
}

/// Emit the first-write banner to `stderr` (or any writer). Format is fixed by
/// the plan so docs and screenshots stay stable.
pub fn emit_first_write_banner(writer: &mut impl Write, id: &str) -> io::Result<()> {
    writeln!(writer, "✓ First memory saved: {id}")?;
    writeln!(writer, "  view: memoryd get --id {id}")?;
    writeln!(writer, "  list: memoryd search \"\"")?;
    writeln!(writer, "  docs: docs/getting-started.md")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{IndexStats, StatusResponse};

    fn status_with_count(count: u64) -> StatusResponse {
        StatusResponse {
            index_stats: Some(IndexStats { active_memories: count, last_reindex: None }),
            ..StatusResponse::default()
        }
    }

    #[test]
    fn emits_banner_only_when_index_reports_exactly_one_active_memory() {
        assert!(should_emit_first_write_banner(&status_with_count(1)));
        assert!(!should_emit_first_write_banner(&status_with_count(0)));
        assert!(!should_emit_first_write_banner(&status_with_count(2)));
        assert!(!should_emit_first_write_banner(&status_with_count(100)));
    }

    #[test]
    fn does_not_emit_when_index_stats_missing() {
        let status = StatusResponse { index_stats: None, ..StatusResponse::default() };
        assert!(!should_emit_first_write_banner(&status));
    }

    #[test]
    fn banner_lists_view_list_and_docs_commands() {
        let mut buf = Vec::new();
        emit_first_write_banner(&mut buf, "mem_20260527_a1b2c3d4e5f60718_000001").expect("write to buffer");
        let banner = String::from_utf8(buf).expect("utf-8");
        assert!(banner.contains("First memory saved"));
        assert!(banner.contains("memoryd get --id mem_20260527_a1b2c3d4e5f60718_000001"));
        assert!(banner.contains("memoryd search"));
        assert!(banner.contains("docs/getting-started.md"));
    }
}
