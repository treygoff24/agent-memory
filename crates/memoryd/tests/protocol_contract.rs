use memoryd::protocol::{
    GetResponse, RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, SearchHit, SearchResponse,
    WriteNoteResponse,
};

#[test]
fn protocol_contract_round_trips_request_variants_as_snake_case_json() {
    let requests = [
        RequestEnvelope::new("req-status", RequestPayload::Status),
        RequestEnvelope::new("req-doctor", RequestPayload::Doctor),
        RequestEnvelope::new(
            "req-search",
            RequestPayload::Search { query: "daemon socket protocol".to_owned(), limit: Some(5), include_body: false },
        ),
        RequestEnvelope::new(
            "req-get",
            RequestPayload::Get { id: "mem_20260428_0123456789abcdef_000001".to_owned(), include_provenance: true },
        ),
        RequestEnvelope::new(
            "req-write-note",
            RequestPayload::WriteNote { text: "observed a useful pattern".to_owned() },
        ),
    ];

    for request in requests {
        let line = request.to_json_line().expect("request serializes");
        assert!(line.ends_with('\n'), "protocol frames are newline-delimited");
        assert!(!line[..line.len() - 1].contains('\n'), "one JSON value per line");

        let value: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert!(value.get("request").is_some(), "request envelope contains request field");
        assert!(!line.contains("WriteNote"), "variant names are snake_case");

        let decoded = RequestEnvelope::from_json_line(&line).expect("request deserializes");
        assert_eq!(decoded, request);
    }
}

#[test]
fn protocol_contract_success_responses_are_bounded_and_guided() {
    let search = ResponseEnvelope::success(
        "req-search",
        ResponsePayload::Search(SearchResponse {
            hits: vec![SearchHit {
                id: "mem_20260428_0123456789abcdef_000001".to_owned(),
                summary: "Protocol DTOs are newline-delimited JSON.".to_owned(),
                snippet: "Protocol DTOs are newline-delimited JSON with bounded snippets.".to_owned(),
                score: 0.87,
            }],
            total: 1,
            guidance: "Bounded snippets only; call memory_get for full body.".to_owned(),
        }),
    );
    let get = ResponseEnvelope::success(
        "req-get",
        ResponsePayload::Get(GetResponse {
            id: "mem_20260428_0123456789abcdef_000001".to_owned(),
            summary: "Protocol DTOs are stable.".to_owned(),
            body: "Short bounded body preview.".to_owned(),
            truncated: true,
            guidance: "Body preview truncated; call memory_get for full body.".to_owned(),
        }),
    );
    let write = ResponseEnvelope::success(
        "req-write-note",
        ResponsePayload::WriteNote(WriteNoteResponse {
            id: "mem_20260428_0123456789abcdef_000002".to_owned(),
            summary: "Note accepted.".to_owned(),
        }),
    );

    for response in [search, get, write] {
        let line = response.to_json_line().expect("response serializes");
        let decoded = ResponseEnvelope::from_json_line(&line).expect("response deserializes");
        assert_eq!(decoded, response);

        let json: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(json["result"].as_object().expect("result object").len(), 1);
        assert!(line.contains("call memory_get for full body") || line.contains("Note accepted"));
    }
}

#[test]
fn protocol_contract_error_response_preserves_id_and_structured_error() {
    let response = ResponseEnvelope::error("req-bad", "invalid_request", "missing required search query", true);

    let line = response.to_json_line().expect("error response serializes");
    let decoded = ResponseEnvelope::from_json_line(&line).expect("error response deserializes");
    assert_eq!(decoded, response);

    let json: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
    assert_eq!(json["id"], "req-bad");
    assert_eq!(json["result"]["error"]["code"], "invalid_request");
    assert_eq!(json["result"]["error"]["retryable"], true);
}
