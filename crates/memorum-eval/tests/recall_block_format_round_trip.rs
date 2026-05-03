use memorum_eval::assertions::{assert_memory_in_recall, parse_recall_block};
use memoryd::recall::{
    render_memory_entry, render_startup_frame, RecallEntry, RecallExplanation, RecallSectionName,
    RenderedRecallSection, SessionBinding,
};

#[test]
fn stream_e_renderer_output_is_parseable_by_stream_h_assertions() {
    let expected_refs = ["mem-alpha", "mem-beta"];
    let recent_memory = [
        RecallEntry {
            id: expected_refs[0].to_owned(),
            summary: "Alpha summary with <xml> & escaped text".to_owned(),
            snippet: Some("Alpha snippet with > comparison & stable escaping".to_owned()),
            updated: "2026-05-02T12:00:00Z".to_owned(),
            source_kind: "agent_primary".to_owned(),
            confidence: "0.97".to_owned(),
        },
        RecallEntry {
            id: expected_refs[1].to_owned(),
            summary: "Beta summary".to_owned(),
            snippet: None,
            updated: "2026-05-02T12:00:01Z".to_owned(),
            source_kind: "tool_result".to_owned(),
            confidence: "0.91".to_owned(),
        },
    ]
    .iter()
    .map(render_memory_entry)
    .collect::<Vec<_>>()
    .join("\n");

    let recall_block = render_startup_frame(
        &SessionBinding {
            session_id: "sess_round_trip".to_owned(),
            harness: "memorum-eval".to_owned(),
            harness_version: None,
            cwd: "/tmp/memorum-eval".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned(), "agent".to_owned()],
        },
        &RecallExplanation::empty(3600),
        &[RenderedRecallSection { name: RecallSectionName::RecentMemory, body: recent_memory }],
    );

    let block = parse_recall_block(&recall_block).expect("Stream E recall block should parse");

    let found_refs = block.memories.iter().map(|memory| memory.ref_id.as_str()).collect::<Vec<_>>();
    assert_eq!(found_refs, expected_refs);
    for ref_id in expected_refs {
        assert_memory_in_recall(&block, ref_id).expect("renderer ref id should be assertable");
    }
}
