//! Human-readable rendering of peer coordination responses for the CLI.
//!
//! These functions turn wire-protocol responses (`PeerStatusResponse`,
//! `PeerActivityResponse`) into the plaintext shown by `memoryd peer status`
//! and `memoryd peer activity`. They live with the CLI that displays them
//! rather than in `protocol.rs`, which is the wire-format module.

use chrono::Utc;

use crate::protocol::{PeerActivityResponse, PeerStatusResponse};

pub fn render_peer_status_human(status: &PeerStatusResponse) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Coordination level: {} ({})\n\n",
        status.coordination_level,
        coordination_level_label(status.coordination_level)
    ));

    output.push_str("Active peer sessions (same device):\n");
    if status.active_sessions.is_empty() {
        output.push_str("  [none]\n");
    } else {
        for session in &status.active_sessions {
            let entities = if session.salient_entities.is_empty() {
                "[none]".to_owned()
            } else {
                session.salient_entities.join(", ")
            };
            output.push_str(&format!(
                "  {}:{}   project:{}   entities: {}\n",
                session.harness,
                truncated_session_id(&session.session_id),
                session.namespace,
                entities
            ));
            output.push_str(&format!(
                "  started {}, last heartbeat {} ago\n",
                session
                    .started_at
                    .map_or_else(|| "unknown".to_owned(), |started_at| { started_at.format("%H:%M").to_string() }),
                human_duration_seconds(session.last_heartbeat_age_seconds)
            ));
        }
    }

    output.push_str("\nActive claim locks:\n");
    if status.claim_locks.is_empty() {
        output.push_str("  [none]\n");
    } else {
        let now = Utc::now();
        for lock in &status.claim_locks {
            let ttl_seconds =
                lock.expires_at.signed_duration_since(now).to_std().map_or(0, |duration| duration.as_secs());
            output.push_str(&format!(
                "  {}   held by {}:{}   expires in {}\n",
                lock.memory_id,
                lock.holder_harness,
                lock.holder_session_id,
                human_duration_seconds(ttl_seconds)
            ));
        }
    }

    output.push_str("\nRecent peer-update deliveries (this session):\n");
    if status.recent_deliveries.is_empty() {
        output.push_str("  [none - run memoryd peer activity for session history]\n");
    } else {
        for delivery in &status.recent_deliveries {
            output.push_str(&format!(
                "  {}:{} -> {}:{}   {}   relevance={:.2}\n",
                delivery.from_harness,
                truncated_session_id(&delivery.from_session_id),
                delivery.to_harness,
                truncated_session_id(&delivery.to_session_id),
                delivery.memory_id,
                delivery.relevance
            ));
        }
    }

    output
}

pub fn render_peer_activity_human(activity: &PeerActivityResponse) -> String {
    let mut output = format!("Peer-update audit (last {} deliveries, this device):\n\n", activity.limit);
    if activity.entries.is_empty() {
        output.push_str("[none]\n");
        return output;
    }

    for entry in &activity.entries {
        output.push_str(&format!(
            "{}  {}:{} -> {}:{}   {}   relevance={:.2}\n",
            entry.delivered_at.format("%Y-%m-%d %H:%M"),
            entry.from_harness,
            truncated_session_id(&entry.from_session_id),
            entry.to_harness,
            truncated_session_id(&entry.to_session_id),
            entry.memory_id,
            entry.relevance
        ));
        output.push_str(&format!("  summary: \"{}\"\n\n", entry.summary));
    }
    output
}

fn coordination_level_label(level: u8) -> &'static str {
    match level {
        1 => "minimal",
        2 => "default - writes + candidates + notes",
        3 => "collaborative",
        _ => "unknown",
    }
}

fn truncated_session_id(session_id: &str) -> String {
    session_id.chars().take(6).collect()
}

fn human_duration_seconds(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m {}s", seconds % 60);
    }
    format!("{}h {}m", minutes / 60, minutes % 60)
}
