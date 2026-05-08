use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::time::Duration;

use url::Url;

use crate::error::{SourceError, SourceResult};
use crate::model::RedirectHop;

const SENSITIVE_URL_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "apikey",
    "auth",
    "authorization",
    "code",
    "key",
    "otp",
    "password",
    "secret",
    "session",
    "signature",
    "sig",
    "token",
];

pub type ResolveFuture<'a> = Pin<Box<dyn Future<Output = SourceResult<Vec<SocketAddr>>> + Send + 'a>>;

pub trait DnsResolver: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> ResolveFuture<'a>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultDnsResolver;

impl DnsResolver for DefaultDnsResolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> ResolveFuture<'a> {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host, port)).await?.collect::<Vec<_>>();
            if addrs.is_empty() {
                return Err(SourceError::url_safety(format!("host `{host}` resolved to no addresses")));
            }
            Ok(addrs)
        })
    }
}

#[derive(Clone, Debug)]
pub struct StaticDnsResolver {
    addrs: Vec<SocketAddr>,
}

impl StaticDnsResolver {
    pub fn new(addrs: Vec<SocketAddr>) -> Self {
        Self { addrs }
    }
}

impl DnsResolver for StaticDnsResolver {
    fn resolve<'a>(&'a self, _host: &'a str, _port: u16) -> ResolveFuture<'a> {
        Box::pin(async move { Ok(self.addrs.clone()) })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressPolicy {
    PublicOnly,
    AllowLoopbackForTests,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedHop {
    pub url: Url,
    pub addrs: Vec<SocketAddr>,
}

impl ValidatedHop {
    pub fn contains_remote_addr(&self, remote_addr: SocketAddr) -> bool {
        self.addrs.iter().any(|addr| addr.ip() == remote_addr.ip() && addr.port() == remote_addr.port())
    }
}

pub async fn validate_initial_url(
    raw_url: &str,
    resolver: &dyn DnsResolver,
    policy: AddressPolicy,
) -> SourceResult<ValidatedHop> {
    let url = Url::parse(raw_url).map_err(|err| SourceError::url_safety(format!("invalid URL: {err}")))?;
    validate_url(url, resolver, policy).await
}

#[allow(clippy::too_many_arguments)]
pub async fn validate_redirect_url(
    current: &Url,
    location: &str,
    resolver: &dyn DnsResolver,
    policy: AddressPolicy,
    redirect_chain: &[RedirectHop],
) -> SourceResult<ValidatedHop> {
    if redirect_chain.len() >= 5 {
        return Err(SourceError::url_safety("redirect chain exceeded 5 hops"));
    }
    let url = current.join(location).map_err(|err| SourceError::url_safety(format!("invalid redirect URL: {err}")))?;
    validate_url(url, resolver, policy).await
}

pub fn pinned_reqwest_client(hop: &ValidatedHop) -> SourceResult<reqwest::Client> {
    let host = hop.url.host_str().ok_or_else(|| SourceError::url_safety("URL is missing host"))?;
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, &hop.addrs)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|err| SourceError::CaptureFailed(format!("build pinned HTTP client: {err}")))
}

pub fn redact_sensitive_url(url: &Url) -> Url {
    let mut redacted = url.clone();
    let retained_pairs = redacted
        .query_pairs()
        .filter(|(key, _value)| !is_sensitive_url_key(key.as_ref()))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    redacted.set_query(None);
    if !retained_pairs.is_empty() {
        let mut pairs = redacted.query_pairs_mut();
        for (key, value) in retained_pairs {
            pairs.append_pair(&key, &value);
        }
    }
    if redacted.fragment().is_some_and(fragment_contains_sensitive_key) {
        redacted.set_fragment(None);
    }
    redacted
}

pub fn redact_sensitive_location_header(raw: &str, base: &Url) -> String {
    if let Ok(url) = Url::parse(raw) {
        return redact_sensitive_url(&url).to_string();
    }
    let Ok(joined) = base.join(raw) else {
        return raw.to_string();
    };
    let redacted = redact_sensitive_url(&joined);
    if raw.starts_with('/') {
        let mut relative = redacted.path().to_string();
        if let Some(query) = redacted.query() {
            relative.push('?');
            relative.push_str(query);
        }
        if let Some(fragment) = redacted.fragment() {
            relative.push('#');
            relative.push_str(fragment);
        }
        return relative;
    }
    redacted.to_string()
}

async fn validate_url(url: Url, resolver: &dyn DnsResolver, policy: AddressPolicy) -> SourceResult<ValidatedHop> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(SourceError::url_safety(format!("unsupported URL scheme `{scheme}`"))),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SourceError::url_safety("embedded URL credentials are forbidden"));
    }
    let host = url.host_str().ok_or_else(|| SourceError::url_safety("URL is missing host"))?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err(SourceError::url_safety("localhost names are forbidden"));
    }
    let port = url.port_or_known_default().ok_or_else(|| SourceError::url_safety("URL is missing port"))?;
    let addrs = if let Some(ip) = url.host().and_then(|host| match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    }) {
        vec![SocketAddr::new(ip, port)]
    } else if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        resolver.resolve(host, port).await?
    };
    if addrs.is_empty() {
        return Err(SourceError::url_safety(format!("host `{host}` resolved to no addresses")));
    }
    for addr in &addrs {
        if !is_allowed_ip(addr.ip(), policy) {
            return Err(SourceError::url_safety(format!("host `{host}` resolved to disallowed address {addr}")));
        }
    }
    Ok(ValidatedHop { url, addrs })
}

fn fragment_contains_sensitive_key(fragment: &str) -> bool {
    fragment
        .split(['&', ';'])
        .map(|part| part.split_once('=').map_or(part, |(key, _value)| key))
        .any(is_sensitive_url_key)
}

fn is_sensitive_url_key(key: &str) -> bool {
    let key = key.trim().trim_start_matches('#').to_ascii_lowercase();
    SENSITIVE_URL_KEYS.iter().any(|sensitive| key == *sensitive || key.ends_with(&format!("_{sensitive}")))
}

pub fn is_allowed_ip(ip: IpAddr, policy: AddressPolicy) -> bool {
    match ip {
        IpAddr::V4(ip) => is_allowed_ipv4(ip, policy),
        IpAddr::V6(ip) => is_allowed_ipv6(ip, policy),
    }
}

fn is_allowed_ipv4(ip: Ipv4Addr, policy: AddressPolicy) -> bool {
    if matches!(policy, AddressPolicy::AllowLoopbackForTests) && ip.is_loopback() {
        return true;
    }
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_broadcast()
    {
        return false;
    }
    let octets = ip.octets();
    if octets[0] == 0 || octets[0] >= 224 {
        return false;
    }
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return false;
    }
    if octets[0] == 169 && octets[1] == 254 {
        return false;
    }
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 2 {
        return false;
    }
    if octets[0] == 198 && octets[1] == 51 && octets[2] == 100 {
        return false;
    }
    if octets[0] == 203 && octets[1] == 0 && octets[2] == 113 {
        return false;
    }
    if ip == Ipv4Addr::new(169, 254, 169, 254) {
        return false;
    }
    true
}

fn is_allowed_ipv6(ip: Ipv6Addr, policy: AddressPolicy) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_allowed_ipv4(mapped, policy);
    }
    if matches!(policy, AddressPolicy::AllowLoopbackForTests) && ip.is_loopback() {
        return true;
    }
    if ip.is_loopback() || ip.is_multicast() || ip.is_unspecified() {
        return false;
    }
    let segments = ip.segments();
    let first = segments[0];
    if (first & 0xfe00) == 0xfc00 {
        return false;
    }
    if (first & 0xffc0) == 0xfe80 {
        return false;
    }
    if first == 0x2001 && segments[1] == 0x0db8 {
        return false;
    }
    true
}
