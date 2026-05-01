use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use memory_privacy::{MaskingSession, MaskingSessionId, PrivacySpan};

use super::types::{DreamError, MaskingContext};

#[derive(Debug, Default, Clone)]
pub struct MaskingDropObserver {
    drops: Arc<AtomicUsize>,
}

impl MaskingDropObserver {
    pub fn drops(&self) -> usize {
        self.drops.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct DreamMaskingSession {
    id: MaskingSessionId,
    id_text: String,
    session: MaskingSession,
    original_private_values: Vec<String>,
    drop_observer: Option<MaskingDropObserver>,
}

impl DreamMaskingSession {
    pub fn new(scope: &str, run_id: &str) -> Self {
        Self::with_drop_observer(scope, run_id, None)
    }

    pub fn with_drop_observer(scope: &str, run_id: &str, drop_observer: Option<MaskingDropObserver>) -> Self {
        let id_text = format!("dream:{scope}:{run_id}");
        let id = MaskingSessionId::new(id_text.clone());
        Self {
            session: MaskingSession::new(id.clone()),
            id,
            id_text,
            original_private_values: Vec::new(),
            drop_observer,
        }
    }

    pub fn context(&self) -> MaskingContext {
        MaskingContext { session_id: self.id_text.clone(), seed_surrogate: format!("mask_seed:{}", self.id_text) }
    }

    pub fn mask(&mut self, text: &str, spans: &[PrivacySpan]) -> Result<String, DreamError> {
        for span in spans {
            if span.end <= text.len() && text.is_char_boundary(span.start) && text.is_char_boundary(span.end) {
                self.original_private_values.push(text[span.start..span.end].to_string());
            }
        }
        self.session
            .mask(text, spans)
            .map_err(|error| DreamError::invalid_request(format!("failed to mask dream input: {error}")))
    }

    pub fn restore(&self, text: &str) -> Result<String, DreamError> {
        self.session
            .restore(&self.id, text)
            .map_err(|error| DreamError::invalid_request(format!("failed to restore dream candidate: {error}")))
    }

    pub fn contains_original_private_value(&self, text: &str) -> bool {
        self.original_private_values.iter().any(|value| !value.is_empty() && text.contains(value))
    }
}

impl Drop for DreamMaskingSession {
    fn drop(&mut self) {
        if let Some(observer) = &self.drop_observer {
            observer.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
}
