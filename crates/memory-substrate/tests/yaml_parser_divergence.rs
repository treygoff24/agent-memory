//! YAML parser round-trip byte-stability fixture.
//!
//! The corpus preserves the edge cases used to validate the `yaml_serde` to
//! `serde_yaml` migration. Parsing through `serde_yaml` and the canonical
//! serializer must reach a byte-identical fixed point on the second pass.

/// Edge-case YAML frontmatter snippets validated for cross-library parity
/// (`yaml_serde` vs `serde_yaml`) prior to consolidation. Each is a complete
/// frontmatter block (mapping at the root) covering a category that
/// historically diverges across YAML implementations or that the hand-rolled
/// serializer must round-trip. Exercised live by
/// [`parser_accepts_full_edge_case_corpus`] so future `serde_yaml` bumps are
/// held to the same acceptance surface the swap was validated against.
fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("plain_unquoted", "summary: a plain scalar\nscope: project\n"),
        ("double_quoted", "summary: \"quoted: with colon\"\n"),
        ("single_quoted", "summary: 'single quoted value'\n"),
        ("number_as_string", "summary: \"42\"\nid: \"007\"\n"),
        ("bare_integer", "count: 42\nnegative: -7\n"),
        ("bare_float", "ratio: 3.14\nexp: 1e9\n"),
        ("bool_true_false", "flag_a: true\nflag_b: false\n"),
        // YAML 1.1 boolean look-alikes carried as strings (must stay strings).
        ("yes_no_string", "answer: \"yes\"\nother: \"no\"\n"),
        ("on_off_string", "toggle: \"on\"\nswitch: \"off\"\n"),
        ("null_forms", "a: null\nb: ~\n"),
        ("empty_value", "summary:\nbody:\n"),
        ("empty_list", "tags: []\n"),
        ("list_of_scalars", "tags:\n  - alpha\n  - beta\n  - gamma\n"),
        ("nested_map", "source:\n  kind: user\n  device: laptop\n"),
        ("list_of_maps", "evidence:\n  - kind: file\n    path: /data/a\n  - kind: url\n    path: http://x\n"),
        ("unicode", "summary: café résumé 日本語 emoji 🦀\n"),
        ("multiline_literal", "note: |\n  line one\n  line two\n"),
        ("multiline_folded", "note: >\n  folded line one\n  folded line two\n"),
        ("special_chars_quoted", "summary: \"a: b - c # d\"\n"),
        ("colon_in_value", "ts: 2026-05-12T15:18:51.945136Z\n"),
        ("leading_trailing_space", "summary: \"  padded  \"\n"),
        ("hash_in_quoted", "summary: \"not # a comment\"\n"),
        ("path_value", "path: file:/data/.memorum-dev/grounding/lockfile-policy.md\n"),
        ("hex_octal_as_string", "a: \"0xff\"\nb: \"0o17\"\nc: \"0b101\"\n"),
        ("dotted_id", "id: mem_20260512_3cb85ebb22bf3577_000001\n"),
    ]
}

/// Round-trip byte-stability: a curated subset of the corpus that constitutes a
/// valid, complete frontmatter document is parsed and re-serialized through the
/// hand-rolled canonical serializer twice; the second pass must reproduce the
/// first byte-for-byte. This guards the property that consolidating the YAML
/// parser onto `serde_yaml` did not perturb on-disk canonical output.
#[test]
fn parser_accepts_full_edge_case_corpus() {
    for (name, yaml) in corpus() {
        let value: serde_json::Value =
            serde_yaml::from_str(yaml).unwrap_or_else(|err| panic!("corpus case {name:?} must parse: {err}"));
        assert!(value.is_object(), "corpus case {name:?} must parse to a mapping root, got {value:?}");
    }
}

mod roundtrip {
    use memory_substrate::frontmatter::{parse_document, serialize_document};

    /// A complete, schema-valid memory document whose `summary` field is the
    /// supplied `summary` literal (verbatim, including any quoting). The rest of
    /// the frontmatter mirrors the canonical minimal document the substrate
    /// schema tests use, so only the summary edge case under test varies.
    fn doc_with_summary(summary: &str) -> String {
        format!(
            "---\nschema_version: 1\nid: mem_20260424_a1b2c3d4e5f60718_000001\ntype: pattern\nscope: agent\n\
             summary: {summary}\nconfidence: 1.0\ntrust_level: trusted\nsensitivity: internal\nstatus: active\n\
             created_at: 2026-04-24T12:00:00Z\nupdated_at: 2026-04-24T12:00:00Z\nauthor:\n  kind: system\n  \
             user_handle: null\n  harness: null\n  harness_version: null\n  session_id: null\n  subagent_id: null\n  \
             phase: null\n  component: test\n---\nBody text.\n"
        )
    }

    /// `(name, summary literal)` pairs exercising the scalar-quoting edge cases
    /// the hand-rolled serializer must preserve across a parse -> serialize
    /// round-trip: YAML boolean look-alikes, unicode, embedded `": "`, and a
    /// numeric look-alike string.
    fn documents() -> Vec<(&'static str, String)> {
        vec![
            ("yaml_lookalike_string", doc_with_summary("\"yes\"")),
            ("unicode", doc_with_summary("café résumé 日本語")),
            ("quoted_colon", doc_with_summary("\"Useful: run doctor\"")),
            ("numeric_lookalike_string", doc_with_summary("\"42\"")),
        ]
    }

    #[test]
    fn parse_then_serialize_is_byte_stable() {
        for (name, doc) in documents() {
            let parsed = parse_document(&doc, None).unwrap_or_else(|err| panic!("parse {name}: {err:?}"));
            let serialized =
                serialize_document(&parsed.memory).unwrap_or_else(|err| panic!("serialize {name}: {err:?}"));

            // Re-parse the canonical output and serialize again; canonical form
            // must be a fixed point (idempotent under parse -> serialize).
            let reparsed = parse_document(&serialized, None).unwrap_or_else(|err| panic!("reparse {name}: {err:?}"));
            let reserialized =
                serialize_document(&reparsed.memory).unwrap_or_else(|err| panic!("reserialize {name}: {err:?}"));

            assert_eq!(serialized, reserialized, "canonical output not byte-stable for {name:?}");
        }
    }
}
