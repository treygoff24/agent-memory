use std::net::SocketAddr;

use memory_source::url_safety::{validate_initial_url, AddressPolicy, StaticDnsResolver};

#[tokio::test]
async fn rejects_unsafe_schemes_hosts_credentials_and_addresses() {
    let public = StaticDnsResolver::new(vec!["93.184.216.34:443".parse::<SocketAddr>().unwrap()]);
    for url in ["file:///tmp/a", "ftp://example.com", "example.com", "http://user:pass@example.com"] {
        assert!(validate_initial_url(url, &public, AddressPolicy::PublicOnly).await.is_err(), "{url}");
    }
    for url in [
        "http://localhost",
        "http://127.0.0.1",
        "http://[::1]",
        "http://[::ffff:127.0.0.1]",
        "http://[::ffff:169.254.169.254]",
        "http://169.254.169.254",
    ] {
        assert!(validate_initial_url(url, &public, AddressPolicy::PublicOnly).await.is_err(), "{url}");
    }
    for addr in ["10.0.0.1:80", "172.16.0.1:80", "192.168.0.1:80", "100.64.0.1:80", "192.0.2.1:80"] {
        let resolver = StaticDnsResolver::new(vec![addr.parse().unwrap()]);
        assert!(
            validate_initial_url("https://example.test", &resolver, AddressPolicy::PublicOnly).await.is_err(),
            "{addr}"
        );
    }
}

#[tokio::test]
async fn allows_public_when_all_resolved_addresses_are_public() {
    let resolver = StaticDnsResolver::new(vec!["93.184.216.34:443".parse().unwrap()]);
    assert!(validate_initial_url("https://example.com/path", &resolver, AddressPolicy::PublicOnly).await.is_ok());
    let mixed = StaticDnsResolver::new(vec!["93.184.216.34:443".parse().unwrap(), "10.0.0.1:443".parse().unwrap()]);
    assert!(validate_initial_url("https://example.com/path", &mixed, AddressPolicy::PublicOnly).await.is_err());
}
