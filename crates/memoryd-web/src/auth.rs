use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::{WebState, DASHBOARD_AUTH_COOKIE, DASHBOARD_AUTH_HEADER, DASHBOARD_AUTH_QUERY};

pub use crate::state::{CSRF_HEADER, DASHBOARD_AUTH_HEADER as AUTH_HEADER};

const AUTH_COOKIE_MAX_AGE_SECONDS: u32 = 60 * 60 * 24;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DashboardAuthSource {
    Cookie,
    Header,
    Query,
}

pub async fn require_dashboard_auth(
    State(state): State<WebState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(source) = dashboard_auth_source(&state, &request) else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    if source == DashboardAuthSource::Query {
        // A bearer token in the query string must never ride along on a
        // state-changing request: it would leak into access logs, the `Referer`
        // header, and browser history. For GET/HEAD we can recover by
        // redirecting to a URL without `?auth=` while setting the auth cookie;
        // for POST/PUT/DELETE/PATCH a redirect would drop the request body, so
        // we reject outright. The client must authenticate via the
        // `Authorization`-style header or the cookie instead.
        if matches!(*request.method(), Method::GET | Method::HEAD) {
            return dashboard_auth_redirect_response(&state, &request);
        }
        return Err(StatusCode::FORBIDDEN);
    }

    let mut response = next.run(request).await;
    // Only a header-authenticated request reaches here without a cookie (query
    // auth always redirected or was rejected above); mint one so the browser
    // carries the session forward without re-sending the header.
    if source == DashboardAuthSource::Header {
        set_dashboard_auth_cookie(&mut response, state.dashboard_auth_token().as_str())?;
    }
    Ok(response)
}

pub async fn require_csrf(
    State(state): State<WebState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.csrf_token().matches_header(&request) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn dashboard_auth_source(state: &WebState, request: &Request<Body>) -> Option<DashboardAuthSource> {
    let expected = state.dashboard_auth_token();
    if request
        .headers()
        .get(DASHBOARD_AUTH_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| expected.constant_time_eq(value))
    {
        return Some(DashboardAuthSource::Header);
    }

    if auth_cookie_value(request.headers()).is_some_and(|value| expected.constant_time_eq(value)) {
        return Some(DashboardAuthSource::Cookie);
    }

    if auth_query_value(request.uri().query()).is_some_and(|value| expected.constant_time_eq(value)) {
        return Some(DashboardAuthSource::Query);
    }

    None
}

fn auth_cookie_value(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::COOKIE).and_then(|value| value.to_str().ok()).and_then(|cookies| {
        cookies.split(';').find_map(|cookie| {
            let (name, value) = cookie.trim().split_once('=')?;
            (name == DASHBOARD_AUTH_COOKIE).then_some(value)
        })
    })
}

fn auth_query_value(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == DASHBOARD_AUTH_QUERY).then_some(value)
    })
}

fn dashboard_auth_cookie(token: &str) -> String {
    format!("{DASHBOARD_AUTH_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={AUTH_COOKIE_MAX_AGE_SECONDS}")
}

fn dashboard_auth_redirect_response(state: &WebState, request: &Request<Body>) -> Result<Response, StatusCode> {
    let mut response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, uri_without_dashboard_auth_query(request))
        .body(Body::empty())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    set_dashboard_auth_cookie(&mut response, state.dashboard_auth_token().as_str())?;
    Ok(response)
}

fn set_dashboard_auth_cookie(response: &mut Response, token: &str) -> Result<(), StatusCode> {
    response.headers_mut().insert(
        header::SET_COOKIE,
        dashboard_auth_cookie(token).parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(())
}

fn uri_without_dashboard_auth_query(request: &Request<Body>) -> String {
    let path = request.uri().path();
    let Some(query) = request.uri().query() else {
        return path.to_owned();
    };
    let filtered = query
        .split('&')
        .filter(|pair| {
            pair.split_once('=').map_or(*pair != DASHBOARD_AUTH_QUERY, |(name, _)| name != DASHBOARD_AUTH_QUERY)
        })
        .collect::<Vec<_>>()
        .join("&");
    if filtered.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{filtered}")
    }
}

/// DNS-rebinding / cross-origin defense for the localhost dashboard.
///
/// The server binds loopback only (enforced in `config::validate_localhost`),
/// but that does not stop a browser on the same machine from connecting to
/// `127.0.0.1` with an attacker-controlled `Host` header after a DNS rebind.
/// Without this guard, every unauthenticated data-bearing GET route
/// (`/api/search`, `/api/audit/*`, `/api/entity-graph`, ...) is reachable from
/// a malicious page and can exfiltrate plaintext memory bodies.
///
/// Applied to the whole router so it fires before any handler. Rejects:
/// - any `Host` that is not a loopback host (`127.0.0.1`, `[::1]`, `localhost`,
///   with an optional port), and
/// - any request that carries an `Origin`/`Referer` whose host is not loopback.
///
/// Requests without an `Origin`/`Referer` (e.g. the initial navigation or a
/// `curl` against the loopback socket) are allowed as long as the `Host` is a
/// loopback host — browsers always attach `Origin`/`Referer` on cross-origin
/// fetches, so the absence of those headers cannot be a cross-origin read.
///
/// A DNS-rebinding attack always carries a non-loopback `Host` header (the
/// browser navigated to the attacker's rebinding domain), so the presence of a
/// non-loopback `Host` is the actual attack signature. A `Host` that is present
/// and non-loopback is rejected; an absent `Host` (which a browser fetch to a
/// rebind domain can never produce) is allowed.
pub async fn require_local_host(request: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let headers = request.headers();

    if let Some(host) = host_header_value(headers) {
        if !is_loopback_authority(host) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    if let Some(origin) = header_str(headers, header::ORIGIN) {
        if !origin_is_loopback(origin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    if let Some(referer) = header_str(headers, header::REFERER) {
        if !origin_is_loopback(referer) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(request).await)
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn host_header_value(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::HOST)
}

/// True when `authority` is `host[:port]` and `host` is a loopback name.
fn is_loopback_authority(authority: &str) -> bool {
    let authority = authority.trim();
    if authority.is_empty() {
        return false;
    }
    // IPv6 literal authority, e.g. `[::1]` or `[::1]:7137`.
    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, port_suffix)) => is_loopback_host(host) && port_suffix_is_valid(port_suffix),
            None => false,
        };
    }
    let (host, port_suffix) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let port_ok = match port_suffix {
        Some(port) => !port.is_empty() && port.chars().all(|character| character.is_ascii_digit()),
        None => true,
    };
    is_loopback_host(host) && port_ok
}

/// Validates the part after `]` in an IPv6 authority: either empty or `:port`.
fn port_suffix_is_valid(suffix: &str) -> bool {
    match suffix.strip_prefix(':') {
        Some(port) => !port.is_empty() && port.chars().all(|character| character.is_ascii_digit()),
        None => suffix.is_empty(),
    }
}

fn is_loopback_host(host: &str) -> bool {
    // Host names are case-insensitive (RFC 3986/7230), so `LOCALHOST` is the
    // same loopback host as `localhost`. The IP literals stay byte-exact.
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

/// True when a full `Origin`/`Referer` value resolves to a loopback host.
///
/// Accepts `http://localhost:7137`, `http://127.0.0.1`, `http://[::1]:7137`,
/// etc. A bare `Origin: null` (sandboxed iframe, some redirects) is rejected.
fn origin_is_loopback(origin: &str) -> bool {
    let origin = origin.trim();
    let after_scheme = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    let Some(after_scheme) = after_scheme else {
        return false;
    };
    // Strip any path/query so only the authority remains.
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    is_loopback_authority(authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_authority_matches_localhost_case_insensitively() {
        // Host names are case-insensitive, so an uppercase `LOCALHOST` is still
        // the loopback host and must be accepted.
        assert!(is_loopback_authority("LOCALHOST:7137"));
        assert!(is_loopback_authority("Localhost"));
        assert!(is_loopback_authority("127.0.0.1:7137"));
        assert!(is_loopback_authority("[::1]:7137"));
    }

    #[test]
    fn loopback_authority_rejects_non_loopback_host() {
        // No regression / no bypass: a genuinely external host stays rejected,
        // including look-alikes that merely contain the loopback name.
        assert!(!is_loopback_authority("evil.com:7137"));
        assert!(!is_loopback_authority("localhost.evil.com"));
        assert!(!is_loopback_authority("notlocalhost"));
        assert!(!is_loopback_authority("10.0.0.1"));
    }

    #[test]
    fn origin_loopback_matches_uppercase_localhost() {
        assert!(origin_is_loopback("http://LOCALHOST:7137"));
        assert!(!origin_is_loopback("http://evil.com"));
        assert!(!origin_is_loopback("null"));
    }
}
