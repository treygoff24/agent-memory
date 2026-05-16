use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use tokio::time::timeout;

#[tokio::test]
async fn dream_journal_file_is_not_searchable_as_canonical_memory() {
    let suffix = unique_suffix();
    let canonical_sentinel = format!("META_CANONICAL_CONTROL_{suffix}");
    let dream_sentinel = format!("META_DREAM_EXCLUSION_{suffix}");
    let scaffold =
        fresh_scaffold_with_seed(|tree| seed_search_fixture(tree, &canonical_sentinel, &dream_sentinel)).await;

    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));
    let canonical_observations =
        agent.run_script([SimulatorAction::Search { query: canonical_sentinel.clone(), namespace: None }]).await;
    assert!(
        canonical_observations.last_search_result_count.is_some_and(|count| count > 0),
        "seeded canonical memory should be indexed before the dream exclusion assertion: {:#?}",
        canonical_observations.last_search_json
    );

    let dream_observations =
        agent.run_script([SimulatorAction::Search { query: dream_sentinel.clone(), namespace: None }]).await;

    assert_eq!(
        dream_observations.last_search_result_count,
        Some(0),
        "dream journal text should not be indexed as canonical memory: {:#?}",
        dream_observations.last_search_json
    );

    // Stream H is a consumer of public daemon/CLI surfaces. The lower-level
    // Substrate::read_memory_envelope(NotACanonicalMemory) check is not
    // reachable through the currently owned memorum-eval files without adding
    // a new dependency or daemon API, so this meta test preserves the public
    // behavioral assertion: dream scratchpad text is not searchable memory.
}

fn seed_search_fixture(tree: &Path, canonical_sentinel: &str, dream_sentinel: &str) {
    write_canonical_memory(tree, canonical_sentinel);
    write_dream_journal(tree, dream_sentinel);
}

fn write_canonical_memory(tree: &Path, sentinel: &str) {
    let id = format!("mem_20260516_{:016x}_{:06}", unix_seconds(), std::process::id() % 1_000_000);
    let memory_path = tree.join("agent").join("patterns").join(format!("{id}.md"));
    std::fs::create_dir_all(memory_path.parent().expect("canonical memory path has parent"))
        .expect("create canonical memory dir");
    std::fs::write(&memory_path, canonical_memory_doc(&id, sentinel)).expect("write canonical memory fixture");
}

fn canonical_memory_doc(id: &str, sentinel: &str) -> String {
    format!(
        r#"---
schema_version: 1
id: {id}
type: pattern
scope: agent
summary: Canonical search control
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-05-16T12:00:00Z
updated_at: 2026-05-16T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: memorum-eval
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: test
---
Canonical memory body for dream exclusion positive control.
{sentinel}
"#
    )
}

fn write_dream_journal(tree: &Path, sentinel: &str) {
    let dream_path = tree.join("dreams").join("journal").join("me").join(format!("{}.md", today_utc()));
    std::fs::create_dir_all(dream_path.parent().expect("dream path has parent")).expect("create dream journal dir");
    std::fs::write(&dream_path, format!("dream scratchpad without frontmatter\n{sentinel}\n"))
        .expect("write noncanonical dream journal file");
}

fn unique_suffix() -> String {
    format!("{}_{}", std::process::id(), unix_seconds())
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock after unix epoch").as_secs()
}

fn today_utc() -> String {
    let days = (unix_seconds() / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

async fn fresh_scaffold_with_seed(seed: impl FnOnce(&Path)) -> DaemonScaffold {
    timeout(Duration::from_secs(10), DaemonScaffold::fresh_with_seed(seed))
        .await
        .expect("fresh seeded daemon scaffold should not hang")
}
