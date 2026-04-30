use std::collections::BTreeMap;

use crate::decision::{PrivacyLabel, PrivacySpan};
use crate::error::{PrivacyError, PrivacyResult};

/// Session identifier for masked synthesis.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaskingSessionId(String);

impl MaskingSessionId {
    /// Create a masking session id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// In-memory masking session. The salt table is intentionally not serializable.
#[derive(Clone, Debug)]
pub struct MaskingSession {
    id: MaskingSessionId,
    replacements: BTreeMap<String, String>,
    counters: BTreeMap<PrivacyLabel, usize>,
}

impl MaskingSession {
    /// Start a masking session.
    pub fn new(id: MaskingSessionId) -> Self {
        Self { id, replacements: BTreeMap::new(), counters: BTreeMap::new() }
    }

    /// Mask detected spans in a text.
    pub fn mask(&mut self, text: &str, spans: &[PrivacySpan]) -> PrivacyResult<String> {
        let mut output = String::new();
        let mut cursor = 0;
        for span in spans {
            if span.start < cursor
                || span.end > text.len()
                || !text.is_char_boundary(span.start)
                || !text.is_char_boundary(span.end)
            {
                return Err(PrivacyError::Masking("invalid or overlapping span".to_string()));
            }
            output.push_str(&text[cursor..span.start]);
            let original = &text[span.start..span.end];
            let token = self.token_for(span.label, original);
            output.push_str(&token);
            cursor = span.end;
        }
        output.push_str(&text[cursor..]);
        Ok(output)
    }

    /// Restore tokens with the same active session.
    pub fn restore(&self, session_id: &MaskingSessionId, text: &str) -> PrivacyResult<String> {
        if session_id != &self.id {
            return Err(PrivacyError::Masking("wrong masking session".to_string()));
        }
        let mut restored = String::with_capacity(text.len());
        let mut cursor = 0;
        while cursor < text.len() {
            let remaining = &text[cursor..];
            if let Some((token, original)) = self.replacements.iter().find(|(token, _)| remaining.starts_with(*token)) {
                restored.push_str(original);
                cursor += token.len();
            } else if let Some(ch) = remaining.chars().next() {
                restored.push(ch);
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        Ok(restored)
    }

    fn token_for(&mut self, label: PrivacyLabel, original: &str) -> String {
        if let Some((token, _)) = self.replacements.iter().find(|(_, value)| value.as_str() == original) {
            return token.clone();
        }
        let next = self.counters.entry(label).or_insert(0);
        *next += 1;
        let token = format!("{}_{}", label.token_prefix(), suffix(*next));
        self.replacements.insert(token.clone(), original.to_string());
        token
    }
}

fn suffix(index: usize) -> String {
    let letter = ((index.saturating_sub(1) % 26) as u8 + b'A') as char;
    letter.to_string()
}
