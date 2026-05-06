use chrono::{DateTime, Utc};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};

use crate::error::{SourceError, SourceResult};
use crate::hash::sha256_prefixed;
use crate::model::{ExcerptLocator, ExcerptMatchKind, ExcerptRecord, SourceArtifactId};

pub fn create_excerpt_records(
    artifact_id: &SourceArtifactId,
    extracted_text: &str,
    requested_quotes: &[String],
    created_at: DateTime<Utc>,
) -> SourceResult<Vec<ExcerptRecord>> {
    if requested_quotes.is_empty() {
        return Err(SourceError::ExcerptNotFound("at least one exact quote is required".to_string()));
    }
    let classifier = DeterministicPrivacyClassifier::new();
    requested_quotes
        .iter()
        .enumerate()
        .map(|(index, quote)| {
            if quote.trim().is_empty() {
                return Err(SourceError::ExcerptNotFound("empty quote".to_string()));
            }
            if safe_plaintext_fragment(&classifier, quote) != SafeFragmentDecision::Allow {
                return Err(SourceError::privacy("excerpt quote is not safe plaintext"));
            }
            let start = extracted_text
                .find(quote)
                .ok_or_else(|| SourceError::ExcerptNotFound(quote.chars().take(80).collect()))?;
            let end = start + quote.len();
            Ok(ExcerptRecord {
                excerpt_id: format!("quote_{:04}", index + 1),
                artifact_id: artifact_id.clone(),
                quote: quote.clone(),
                quote_sha256: sha256_prefixed(quote.as_bytes()),
                locator: ExcerptLocator::ByteRange { start, end },
                match_kind: ExcerptMatchKind::Exact,
                created_at,
            })
        })
        .collect()
}

pub fn verify_excerpt_anchor(extracted_text: &str, record: &ExcerptRecord) -> SourceResult<()> {
    match record.locator {
        ExcerptLocator::ByteRange { start, end } => {
            let Some(slice) = extracted_text.get(start..end) else {
                return Err(SourceError::integrity(format!("excerpt {} byte range is invalid", record.excerpt_id)));
            };
            if slice != record.quote {
                return Err(SourceError::integrity(format!(
                    "excerpt {} no longer matches extracted text",
                    record.excerpt_id
                )));
            }
            if record.quote_sha256 != sha256_prefixed(record.quote.as_bytes()) {
                return Err(SourceError::integrity(format!("excerpt {} quote hash mismatch", record.excerpt_id)));
            }
            Ok(())
        }
    }
}
