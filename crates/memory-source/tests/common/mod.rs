use std::net::SocketAddr;

use memory_source::StaticDnsResolver;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub async fn spawn_server(responses: Vec<String>) -> (String, StaticDnsResolver) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    let resolver = StaticDnsResolver::new(vec![SocketAddr::new("127.0.0.1".parse().unwrap(), addr.port())]);
    (format!("http://example.test:{}", addr.port()), resolver)
}
