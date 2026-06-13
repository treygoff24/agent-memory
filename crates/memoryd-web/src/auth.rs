use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::WebState;

pub use crate::state::CSRF_HEADER;

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
    matches!(host, "127.0.0.1" | "::1" | "localhost")
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
