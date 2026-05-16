use std::net::SocketAddr;

use memory_source::url_safety::{
    redact_sensitive_location_header, validate_initial_url, validate_redirect_url, AddressPolicy, StaticDnsResolver,
};
use memory_source::RedirectHop;
use url::Url;

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
    for addr in
        ["[fd00::1]:443", "[fe80::1]:443", "[::]:443", "[ff02::1]:443", "[2001:db8::1]:443", "[::ffff:127.0.0.1]:443"]
    {
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

#[tokio::test]
async fn redirect_validation_allows_exactly_five_hops() {
    let resolver = StaticDnsResolver::new(vec!["93.184.216.34:443".parse().unwrap()]);
    let current = Url::parse("https://example.com/four").unwrap();
    let five_hop_chain = redirect_chain(5);

    validate_redirect_url(&current, "/final", &resolver, AddressPolicy::PublicOnly, &five_hop_chain)
        .await
        .expect("the fifth redirect hop is within the documented limit");

    let err = validate_redirect_url(&current, "/too-far", &resolver, AddressPolicy::PublicOnly, &redirect_chain(6))
        .await
        .expect_err("the sixth redirect hop exceeds the documented limit");

    assert!(err.to_string().contains("redirect chain exceeded 5 hops"), "{err}");
}

#[tokio::test]
async fn redirect_validation_rejects_unsafe_targets_and_resolved_addresses() {
    let public = StaticDnsResolver::new(vec!["93.184.216.34:443".parse().unwrap()]);
    let private = StaticDnsResolver::new(vec!["10.0.0.1:443".parse().unwrap()]);
    let current = Url::parse("https://example.com/start").unwrap();

    for (location, resolver) in [
        ("file:///tmp/a", &public),
        ("http://user:pass@example.com", &public),
        ("http://localhost/admin", &public),
        ("http://127.0.0.1/admin", &public),
        ("//169.254.169.254/latest/meta-data", &public),
        ("https://example.test/admin", &private),
    ] {
        assert!(
            validate_redirect_url(&current, location, resolver, AddressPolicy::PublicOnly, &[]).await.is_err(),
            "{location}"
        );
    }
}

#[test]
fn redacts_sensitive_relative_location_without_forcing_absolute_url() {
    let base = Url::parse("https://example.com/start/page").unwrap();

    assert_eq!(redact_sensitive_location_header("next?token=secret&keep=yes#session=secret", &base), "next?keep=yes");
    assert_eq!(
        redact_sensitive_location_header("../final?api_key=secret&keep=yes#section", &base),
        "../final?keep=yes#section"
    );
    assert_eq!(
        redact_sensitive_location_header("//cdn.example.com/final?token=secret&keep=yes", &base),
        "https://cdn.example.com/final?keep=yes"
    );
}

fn redirect_chain(len: usize) -> Vec<RedirectHop> {
    (0..len)
        .map(|index| RedirectHop {
            url: format!("https://example.com/{index}"),
            status: 302,
            location: format!("/{index}"),
        })
        .collect()
}
