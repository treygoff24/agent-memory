use std::fs;

use memorum_eval::assertions::{
    assert_memory_in_recall, assert_no_memory_in_recall, assert_no_pii_on_disk, assert_xml_valid, parse_recall_block,
    AssertionError,
};
use memoryd::recall::{
    render_memory_entry, render_startup_frame, RecallEntry, RecallExplanation, RecallSectionName,
    RenderedRecallSection, SessionBinding,
};

fn sample_recall_xml() -> String {
    let memories = [
        RecallEntry {
            id: "mem-alpha".to_owned(),
            summary: "Alpha body".to_owned(),
            snippet: Some("Alpha snippet".to_owned()),
            updated: "2026-05-02T00:00:00Z".to_owned(),
            source_kind: "agent_primary".to_owned(),
            confidence: "0.95".to_owned(),
        },
        RecallEntry {
            id: "mem-beta".to_owned(),
            summary: "Beta body".to_owned(),
            snippet: None,
            updated: "2026-05-02T00:00:01Z".to_owned(),
            source_kind: "agent_primary".to_owned(),
            confidence: "0.90".to_owned(),
        },
    ]
    .iter()
    .map(render_memory_entry)
    .collect::<Vec<_>>()
    .join("\n");

    render_startup_frame(
        &SessionBinding {
            session_id: "sess_eval_assertions".to_owned(),
            harness: "memorum-eval".to_owned(),
            harness_version: None,
            cwd: "/tmp/memorum-eval".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned(), "agent".to_owned()],
        },
        &RecallExplanation::empty(3600),
        &[
            RenderedRecallSection { name: RecallSectionName::RecentMemory, body: memories },
            RenderedRecallSection {
                name: RecallSectionName::PendingAttention,
                body: r#"<item kind="drift" count="3">Review stale claims</item>"#.to_owned(),
            },
        ],
    )
}

#[test]
fn parses_renderer_generated_recall_block_memories_and_pending_attention() {
    let block = parse_recall_block(&sample_recall_xml()).expect("recall block should parse");

    assert_eq!(block.memories.len(), 2);
    assert_eq!(block.memories[0].ref_id, "mem-alpha");
    assert!(block.memories[0].body.contains("<summary>Alpha body</summary>"));
    assert!(block.memories[0].body.contains("<snippet>Alpha snippet</snippet>"));
    assert_eq!(block.memories[1].ref_id, "mem-beta");
    assert_eq!(block.omitted_count, None);
    assert_eq!(block.pending_attention_items.len(), 1);
    assert_eq!(block.pending_attention_items[0].kind, Some("drift".to_string()));
    assert_eq!(block.pending_attention_items[0].count, Some(3));
    assert_eq!(block.pending_attention_items[0].text, "Review stale claims");
}

#[test]
fn parses_omitted_count_attribute_when_present() {
    let block =
        parse_recall_block(r#"<memory-recall omitted_count="2"></memory-recall>"#).expect("recall block should parse");

    assert_eq!(block.omitted_count, Some(2));
}

#[test]
fn parses_omitted_count_element_when_attribute_is_absent() {
    let block = parse_recall_block(
        r#"<memory-recall>
            <omitted_count>4</omitted_count>
            <memory ref="mem-gamma">Gamma body</memory>
        </memory-recall>"#,
    )
    .expect("recall block should parse");

    assert_eq!(block.omitted_count, Some(4));
    assert_eq!(block.memories[0].ref_id, "mem-gamma");
}

#[test]
fn asserts_memory_presence_with_rich_failure() {
    let block = parse_recall_block(&sample_recall_xml()).expect("recall block should parse");

    assert_memory_in_recall(&block, "mem-alpha").expect("present memory should pass");

    let error = assert_memory_in_recall(&block, "missing-ref").expect_err("missing memory should fail");
    assert!(matches!(error, AssertionError::MemoryMissing { .. }));
    let message = error.to_string();
    assert!(message.contains("expected memory ref `missing-ref`"));
    assert!(message.contains("found refs [mem-alpha, mem-beta]"));
    assert!(message.contains("assert_memory_in_recall"));
}

#[test]
fn asserts_memory_absence_with_rich_failure() {
    let block = parse_recall_block(&sample_recall_xml()).expect("recall block should parse");

    assert_no_memory_in_recall(&block, "missing-ref").expect("absent memory should pass");

    let error = assert_no_memory_in_recall(&block, "mem-beta").expect_err("present memory should fail");
    assert!(matches!(error, AssertionError::UnexpectedMemory { .. }));
    let message = error.to_string();
    assert!(message.contains("expected no memory ref `mem-beta`"));
    assert!(message.contains("found refs [mem-alpha, mem-beta]"));
    assert!(message.contains("assert_no_memory_in_recall"));
}

#[test]
fn validates_well_formed_and_rejects_malformed_xml() {
    assert_xml_valid(&sample_recall_xml()).expect("well-formed recall block should be valid");

    let error = assert_xml_valid("<memory-recall><memory ref=\"oops\"></memory-recall>")
        .expect_err("malformed block should fail");
    assert!(matches!(error, AssertionError::MalformedRecallBlock { .. }));
    assert!(error.to_string().contains("mismatched closing tag"));
}

#[test]
fn asserts_pii_string_is_not_written_anywhere_under_tree() {
    let pii = "+15550000001";
    let tree_dir = tempfile::tempdir().expect("fixture directory should be created");
    fs::create_dir_all(tree_dir.path().join("nested")).expect("nested fixture directory should be created");
    fs::write(tree_dir.path().join("safe.md"), "safe synthetic content").expect("safe file write");

    assert_no_pii_on_disk(tree_dir.path(), pii).expect("tree without PII should pass");

    fs::write(tree_dir.path().join("nested/leak.md"), format!("leaked {pii}")).expect("leaky file write");
    let error = assert_no_pii_on_disk(tree_dir.path(), pii).expect_err("PII on disk should fail");
    assert!(matches!(error, AssertionError::PiiFoundOnDisk { .. }));
    let message = error.to_string();
    assert!(message.contains("assert_no_pii_on_disk"));
    assert!(message.contains("leak.md"));
}
