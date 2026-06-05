//! Source capture and trust-artifact request handlers: web/local artifact capture
//! (with capture-mode mapping and location validation) and trust-artifact rendering.

use super::*;

pub(crate) async fn trust_artifact_response(
    substrate: &Substrate,
    state: &HandlerState,
    id: &str,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = HandlerError::parse_memory_id(id)?;
    let artifact = crate::trust_artifact::TrustArtifactBuilder::new(substrate)
        .with_claim_locks(state.claim_locks())
        .build(&memory_id)
        .await
        .map_err(HandlerError::trust_artifact)?;
    Ok(ResponsePayload::TrustArtifact(Box::new(artifact)))
}

pub(crate) async fn capture_source_response(
    substrate: &Substrate,
    payload: SourceCapturePayload,
) -> Result<ResponsePayload, HandlerError> {
    let SourceCapturePayload { source, mode, excerpts, note, local_path } = payload;
    if excerpts.is_empty() {
        return Err(HandlerError::invalid_request("source capture requires at least one excerpt"));
    }
    if excerpts.len() > 8 {
        return Err(HandlerError::invalid_request("source capture accepts at most 8 excerpts"));
    }
    for excerpt in &excerpts {
        if excerpt.trim().is_empty() {
            return Err(HandlerError::invalid_request("source capture excerpts must be non-empty"));
        }
        if excerpt.len() > 2 * 1024 {
            return Err(HandlerError::invalid_request("source capture excerpts must be at most 2 KiB"));
        }
    }
    if let Some(note) = &note {
        if note.len() > 2 * 1024 {
            return Err(HandlerError::invalid_request("source capture note must be at most 2 KiB"));
        }
        if !is_safe_plaintext_for_indexing(note) {
            return Err(HandlerError::invalid_request("source capture note must not contain sensitive material"));
        }
    }
    validate_source_capture_location(mode, local_path.as_deref())?;
    let encryption_key = FileKeyProvider::runtime_default(&substrate.roots().runtime);
    let key_path = encryption_key.path().exists().then(|| encryption_key.path().to_path_buf());
    let response = capture_web_source(
        substrate.roots().repo.clone(),
        CaptureWebSourceRequest {
            url: source,
            excerpts,
            note,
            mode: source_mode_to_capture_mode(mode),
            local_path,
            key_path,
        },
    )
    .await
    .map_err(HandlerError::source_capture)?;
    Ok(ResponsePayload::CaptureSource(CaptureSourceResponse {
        artifact_id: response.artifact_id,
        source_refs: response.source_refs,
        mode,
        final_url: response.final_url,
        captured_at: response.captured_at,
        capture_status: response.capture_status,
        warnings: response.warnings,
    }))
}

fn validate_source_capture_location(mode: CaptureSourceMode, local_path: Option<&Path>) -> Result<(), HandlerError> {
    match mode {
        CaptureSourceMode::HttpStatic => Ok(()),
        CaptureSourceMode::LocalArtifact => {
            let path = local_path
                .ok_or_else(|| HandlerError::invalid_request("local_artifact source capture requires local_path"))?;
            if path.components().any(|component| matches!(component, Component::ParentDir)) {
                return Err(HandlerError::invalid_request("source capture local_path must not contain path traversal"));
            }
            Ok(())
        }
        CaptureSourceMode::PdfText
        | CaptureSourceMode::BrowserRendered
        | CaptureSourceMode::Screenshot
        | CaptureSourceMode::Authenticated
        | CaptureSourceMode::Unsupported => {
            if local_path
                .is_some_and(|path| path.components().any(|component| matches!(component, Component::ParentDir)))
            {
                return Err(HandlerError::invalid_request("source capture local_path must not contain path traversal"));
            }
            Ok(())
        }
    }
}

fn source_mode_to_capture_mode(mode: CaptureSourceMode) -> CaptureMode {
    match mode {
        CaptureSourceMode::HttpStatic => CaptureMode::HttpStatic,
        CaptureSourceMode::LocalArtifact => CaptureMode::LocalArtifact,
        CaptureSourceMode::PdfText => CaptureMode::PdfText,
        CaptureSourceMode::BrowserRendered => CaptureMode::BrowserRendered,
        CaptureSourceMode::Screenshot => CaptureMode::Screenshot,
        CaptureSourceMode::Authenticated => CaptureMode::Authenticated,
        CaptureSourceMode::Unsupported => CaptureMode::Unsupported,
    }
}
