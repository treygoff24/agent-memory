use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use memorum_eval::assertions::{
    assert_memory_in_recall, assert_no_memory_in_recall, assert_no_pii_on_disk, assert_xml_valid, parse_recall_block,
    AssertionError,
};

fn sample_recall_xml() -> &'static str {
    r#"<memory-recall omitted_count="2">
        <memory ref="mem-alpha">Alpha body</memory>
        <memory ref="mem-beta" />
        <pending-attention>
            <item kind="drift" count="3">Review stale claims</item>
        </pending-attention>
    </memory-recall>"#
}

#[test]
fn parses_recall_block_memories_omissions_and_pending_attention() {
    let block = parse_recall_block(sample_recall_xml()).expect("recall block should parse");

    assert_eq!(block.memories.len(), 2);
    assert_eq!(block.memories[0].ref_id, "mem-alpha");
    assert_eq!(block.memories[0].body, "Alpha body");
    assert_eq!(block.memories[1].ref_id, "mem-beta");
    assert_eq!(block.omitted_count, Some(2));
    assert_eq!(block.pending_attention_items.len(), 1);
    assert_eq!(block.pending_attention_items[0].kind, Some("drift".to_string()));
    assert_eq!(block.pending_attention_items[0].count, Some(3));
    assert_eq!(block.pending_attention_items[0].text, "Review stale claims");
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
    let block = parse_recall_block(sample_recall_xml()).expect("recall block should parse");

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
    let block = parse_recall_block(sample_recall_xml()).expect("recall block should parse");

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
    assert_xml_valid(sample_recall_xml()).expect("well-formed recall block should be valid");

    let error = assert_xml_valid("<memory-recall><memory ref=\"oops\"></memory-recall>")
        .expect_err("malformed block should fail");
    assert!(matches!(error, AssertionError::MalformedRecallBlock { .. }));
    assert!(error.to_string().contains("mismatched closing tag"));
}

#[test]
fn asserts_pii_string_is_not_written_anywhere_under_tree() {
    let pii = "+15550000001";
    let tree_dir = unique_temp_tree();
    fs::create_dir_all(tree_dir.join("nested")).expect("fixture directory should be created");
    fs::write(tree_dir.join("safe.md"), "safe synthetic content").expect("safe file write");

    assert_no_pii_on_disk(&tree_dir, pii).expect("tree without PII should pass");

    fs::write(tree_dir.join("nested/leak.md"), format!("leaked {pii}")).expect("leaky file write");
    let error = assert_no_pii_on_disk(&tree_dir, pii).expect_err("PII on disk should fail");
    assert!(matches!(error, AssertionError::PiiFoundOnDisk { .. }));
    let message = error.to_string();
    assert!(message.contains("assert_no_pii_on_disk"));
    assert!(message.contains("leak.md"));

    fs::remove_dir_all(&tree_dir).expect("fixture directory should be cleaned up");
}

fn unique_temp_tree() -> std::path::PathBuf {
    let nanos =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("system time should be after Unix epoch").as_nanos();
    std::env::temp_dir().join(format!("memorum-eval-assertions-{nanos}"))
}
