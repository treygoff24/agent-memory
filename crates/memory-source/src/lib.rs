#![deny(unsafe_op_in_unsafe_fn)]
//! Web source capture artifacts for local memory grounding.

pub mod capture;
pub mod error;
pub mod excerpt;
pub mod extract;
pub mod hash;
pub mod model;
pub mod storage;
pub mod url_safety;

pub use capture::{
    capture_web_source, capture_web_source_with_resolver, CaptureWebSourceRequest, CaptureWebSourceResponse,
};
pub use error::{SourceError, SourceResult};
pub use model::{
    CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator, ExcerptMatchKind,
    ExcerptRecord, RawStorage, RedirectHop, SourceArtifactId, WebCaptureManifest, WebCaptureSourceRef,
};
pub use storage::{ArtifactStore, SourceArtifactPath, WebCaptureArtifact};
pub use url_safety::{AddressPolicy, DefaultDnsResolver, DnsResolver, StaticDnsResolver, ValidatedHop};
