use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{NaiveDate, Utc};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::recall::budget::truncate_utf8_bytes;
use crate::recall::render::escape_xml_text;

pub const CAP_SECTION: &str = "cap_section";
pub const CAP_TOTAL: &str = "cap_total";
pub const NO_ENTITY_MATCH: &str = "no_entity_match";
pub const UNSAFE_FRAGMENT: &str = "unsafe_fragment";
pub const MALFORMED_RECORD: &str = "malformed_record";

const PER_SCOPE_CAP: usize = 2;
const TOTAL_CAP: usize = 6;
const QUESTION_TEXT_MAX_BYTES: usize = 240;
const RECENT_WINDOW_DAYS: i64 = 7;
const RECENT_SURFACED_RING_LIMIT: usize = 1_024;

static RECENT_SURFACED_QUESTIONS: OnceLock<Mutex<RecentSurfacedQuestionStore>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamQuestionSelection {
    pub lines: Vec<String>,
    pub omitted_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateQuestion {
    scope: String,
    question: String,
    overlap: usize,
    date: NaiveDate,
    novelty_hash: [u8; 32],
}

#[derive(Debug, Deserialize)]
struct QuestionRecord {
    entities: Vec<String>,
    question: String,
}

pub fn select_pending_attention_questions(
    repo: &Path,
    namespaces_in_scope: &[String],
    active_entity_ids: &BTreeSet<String>,
) -> DreamQuestionSelection {
    let mut omitted_total = BTreeMap::new();
    let mut candidates = Vec::new();
    let classifier = DeterministicPrivacyClassifier::new();
    let today = Utc::now().date_naive();
    let recent_surfaced_hashes = recent_surfaced_hashes(repo, today);

    for scope in namespaces_in_scope {
        let Some((date, path)) = most_recent_question_file(repo, scope, today) else {
            continue;
        };
        collect_candidates_from_file(
            QuestionFileSelection {
                scope,
                date,
                path: &path,
                active_entity_ids,
                classifier: &classifier,
                recent_surfaced_hashes: &recent_surfaced_hashes,
            },
            &mut candidates,
            &mut omitted_total,
        );
    }

    candidates.sort_by(compare_candidates);
    let selected = apply_caps(candidates, &mut omitted_total);
    record_surfaced_hashes(repo, today, selected.iter().map(|candidate| candidate.novelty_hash));

    DreamQuestionSelection { lines: selected.into_iter().map(render_question_line).collect(), omitted_total }
}

pub fn render_pending_attention_body(review_line: Option<String>, dream_question_lines: &[String]) -> String {
    review_line.into_iter().chain(dream_question_lines.iter().cloned()).collect::<Vec<_>>().join("\n")
}

fn collect_candidates_from_file(
    selection: QuestionFileSelection<'_>,
    candidates: &mut Vec<CandidateQuestion>,
    omitted_total: &mut BTreeMap<String, u64>,
) {
    let Ok(text) = fs::read_to_string(selection.path) else {
        increment(omitted_total, MALFORMED_RECORD);
        return;
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<QuestionRecord>(line) else {
            increment(omitted_total, MALFORMED_RECORD);
            continue;
        };
        if record.question.trim().is_empty() || record.entities.iter().any(|entity| entity.trim().is_empty()) {
            increment(omitted_total, MALFORMED_RECORD);
            continue;
        }
        if record.entities.is_empty() {
            continue;
        }

        let overlap = entity_overlap(&record.entities, selection.active_entity_ids);
        if overlap == 0 {
            increment(omitted_total, NO_ENTITY_MATCH);
            continue;
        }
        if safe_plaintext_fragment(selection.classifier, &record.question) != SafeFragmentDecision::Allow {
            increment(omitted_total, UNSAFE_FRAGMENT);
            continue;
        }

        let question = truncate_utf8_bytes(record.question.trim(), QUESTION_TEXT_MAX_BYTES).value;
        let novelty_hash = novelty_hash(&question);
        if selection.recent_surfaced_hashes.contains(&novelty_hash) {
            continue;
        }
        candidates.push(CandidateQuestion {
            scope: selection.scope.to_owned(),
            novelty_hash,
            question,
            overlap,
            date: selection.date,
        });
    }
}

struct QuestionFileSelection<'a> {
    scope: &'a str,
    date: NaiveDate,
    path: &'a Path,
    active_entity_ids: &'a BTreeSet<String>,
    classifier: &'a DeterministicPrivacyClassifier,
    recent_surfaced_hashes: &'a BTreeSet<[u8; 32]>,
}

fn most_recent_question_file(repo: &Path, scope: &str, today: NaiveDate) -> Option<(NaiveDate, PathBuf)> {
    let dir = repo.join("dreams/questions").join(scope_path(scope)?);
    let entries = fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| question_file_date(&entry.path()).map(|date| (date, entry.path())))
        .filter(|(date, _)| *date <= today)
        .max_by_key(|(date, _)| *date)
}

fn scope_path(scope: &str) -> Option<PathBuf> {
    match scope {
        "me" | "agent" => Some(PathBuf::from(scope)),
        _ => scope
            .strip_prefix("project:")
            .map(|id| PathBuf::from("project").join(id))
            .or_else(|| scope.strip_prefix("org:").map(|id| PathBuf::from("org").join(id))),
    }
}

fn question_file_date(path: &Path) -> Option<NaiveDate> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

fn entity_overlap(entities: &[String], active_entity_ids: &BTreeSet<String>) -> usize {
    entities.iter().filter(|entity| active_entity_ids.contains(entity.as_str())).collect::<BTreeSet<_>>().len()
}

fn apply_caps(candidates: Vec<CandidateQuestion>, omitted_total: &mut BTreeMap<String, u64>) -> Vec<CandidateQuestion> {
    let mut selected = Vec::new();
    let mut selected_by_scope = BTreeMap::<String, usize>::new();

    for candidate in candidates {
        if selected.len() >= TOTAL_CAP {
            increment(omitted_total, CAP_TOTAL);
            continue;
        }

        let scope_count = selected_by_scope.entry(candidate.scope.clone()).or_default();
        if *scope_count >= PER_SCOPE_CAP {
            increment(omitted_total, CAP_SECTION);
            continue;
        }

        *scope_count += 1;
        selected.push(candidate);
    }

    selected
}

fn compare_candidates(left: &CandidateQuestion, right: &CandidateQuestion) -> std::cmp::Ordering {
    Reverse(left.overlap)
        .cmp(&Reverse(right.overlap))
        .then_with(|| Reverse(left.date).cmp(&Reverse(right.date)))
        .then_with(|| left.novelty_hash.cmp(&right.novelty_hash))
        .then_with(|| left.scope.cmp(&right.scope))
        .then_with(|| left.question.cmp(&right.question))
}

fn render_question_line(candidate: CandidateQuestion) -> String {
    format!("- [{}] {}", escape_xml_text(&candidate.scope), escape_xml_text(&candidate.question))
}

fn novelty_hash(question: &str) -> [u8; 32] {
    Sha256::digest(question.as_bytes()).into()
}

#[derive(Debug, Default)]
struct RecentSurfacedQuestionStore {
    hashes_by_repo: BTreeMap<PathBuf, VecDeque<RecentSurfacedQuestion>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecentSurfacedQuestion {
    hash: [u8; 32],
    surfaced_on: NaiveDate,
}

fn recent_surfaced_hashes(repo: &Path, today: NaiveDate) -> BTreeSet<[u8; 32]> {
    let mut store = recent_surfaced_question_store().lock().expect("recent dream question store lock not poisoned");
    let entries = store.hashes_by_repo.entry(repo.to_path_buf()).or_default();
    prune_recent_surfaced_entries(entries, today);
    entries.iter().map(|entry| entry.hash).collect()
}

fn record_surfaced_hashes(repo: &Path, today: NaiveDate, hashes: impl IntoIterator<Item = [u8; 32]>) {
    let mut store = recent_surfaced_question_store().lock().expect("recent dream question store lock not poisoned");
    let entries = store.hashes_by_repo.entry(repo.to_path_buf()).or_default();
    prune_recent_surfaced_entries(entries, today);

    let mut known_hashes = entries.iter().map(|entry| entry.hash).collect::<BTreeSet<_>>();
    for hash in hashes {
        if known_hashes.insert(hash) {
            entries.push_back(RecentSurfacedQuestion { hash, surfaced_on: today });
        }
    }
    while entries.len() > RECENT_SURFACED_RING_LIMIT {
        entries.pop_front();
    }
}

fn prune_recent_surfaced_entries(entries: &mut VecDeque<RecentSurfacedQuestion>, today: NaiveDate) {
    entries.retain(|entry| is_inside_recent_window(entry.surfaced_on, today));
}

fn is_inside_recent_window(surfaced_on: NaiveDate, today: NaiveDate) -> bool {
    let age_days = today.signed_duration_since(surfaced_on).num_days();
    (0..RECENT_WINDOW_DAYS).contains(&age_days)
}

fn recent_surfaced_question_store() -> &'static Mutex<RecentSurfacedQuestionStore> {
    RECENT_SURFACED_QUESTIONS.get_or_init(|| Mutex::new(RecentSurfacedQuestionStore::default()))
}

fn increment(omitted_total: &mut BTreeMap<String, u64>, reason: &str) {
    *omitted_total.entry(reason.to_owned()).or_default() += 1;
}
