use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, StatusResponse};
use memoryd_tui::client::DaemonClient;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

#[tokio::test]
async fn fetch_snapshot_starts_with_status_request() {
    let socket_path = std::env::temp_dir().join(format!("memoryd-tui-poll-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("bind socket");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.expect("read");
        let request = RequestEnvelope::from_json_line(&line).expect("decode").request;
        let response = ResponseEnvelope::success(
            "status",
            ResponsePayload::Status(StatusResponse {
                state: "ready".into(),
                guidance: "ok".into(),
                recall: Default::default(),
                dreams: Default::default(),
                passive_notifications: Vec::new(),
            }),
        );
        stream.get_mut().write_all(response.to_json_line().expect("json").as_bytes()).await.expect("write");
        request
    });

    let client = DaemonClient::new(&socket_path);
    let snapshot = client.fetch_snapshot().await.expect("snapshot");

    assert_eq!(server.await.expect("server"), RequestPayload::Status);
    assert_eq!(snapshot.daemon_state, "ready");
    let _ = std::fs::remove_file(socket_path);
}
