use encoding_rs::{Encoding, UTF_8};
use scraper::{Html, Selector};

use crate::error::SourceResult;

pub const DEFAULT_EXTRACTED_TEXT_CAP: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedText {
    pub text: String,
    pub warnings: Vec<String>,
    pub unsupported_reason: Option<String>,
}

impl ExtractedText {
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self { text: String::new(), warnings: Vec::new(), unsupported_reason: Some(reason.into()) }
    }

    pub fn is_supported(&self) -> bool {
        self.unsupported_reason.is_none()
    }
}

pub fn extract_text(content_type: Option<&str>, raw_bytes: &[u8]) -> SourceResult<ExtractedText> {
    extract_text_with_cap(content_type, raw_bytes, DEFAULT_EXTRACTED_TEXT_CAP)
}

pub fn extract_text_with_cap(content_type: Option<&str>, raw_bytes: &[u8], cap: usize) -> SourceResult<ExtractedText> {
    let content_type = content_type.unwrap_or("application/octet-stream");
    let media_type = content_type.split(';').next().unwrap_or(content_type).trim().to_ascii_lowercase();
    let charset = charset_from_content_type(content_type);
    match media_type.as_str() {
        "text/plain" => decode_bounded(raw_bytes, charset, cap).map(|(text, warnings)| ExtractedText {
            text,
            warnings,
            unsupported_reason: None,
        }),
        "text/html" | "application/xhtml+xml" => {
            let (html, mut warnings) = decode_bounded(raw_bytes, charset, cap.saturating_mul(4))?;
            let text = extract_visible_html_text(&html);
            let text = truncate_at_char_boundary(text, cap);
            if text.len() >= cap {
                warnings.push("extracted_text_truncated".to_string());
            }
            Ok(ExtractedText { text, warnings, unsupported_reason: None })
        }
        other => Ok(ExtractedText::unsupported(format!("unsupported content type `{other}`"))),
    }
}

pub fn raw_textual_projection(content_type: Option<&str>, raw_bytes: &[u8]) -> Option<String> {
    let content_type = content_type?;
    let media_type = content_type.split(';').next()?.trim().to_ascii_lowercase();
    if !matches!(media_type.as_str(), "text/plain" | "text/html" | "application/xhtml+xml") {
        return None;
    }
    Some(decode_all(raw_bytes, charset_from_content_type(content_type)))
}

fn decode_bounded(raw_bytes: &[u8], charset: Option<&str>, cap: usize) -> SourceResult<(String, Vec<String>)> {
    let encoding = charset.and_then(|value| Encoding::for_label(value.trim().as_bytes())).unwrap_or(UTF_8);
    let (decoded, _encoding_used, had_errors) = encoding.decode(raw_bytes);
    let mut warnings = Vec::new();
    if had_errors {
        warnings.push("decode_replacement_characters".to_string());
    }
    let text = truncate_at_char_boundary(decoded.into_owned(), cap);
    Ok((text, warnings))
}

fn decode_all(raw_bytes: &[u8], charset: Option<&str>) -> String {
    let encoding = charset.and_then(|value| Encoding::for_label(value.trim().as_bytes())).unwrap_or(UTF_8);
    let (decoded, _encoding_used, _had_errors) = encoding.decode(raw_bytes);
    decoded.into_owned()
}

fn charset_from_content_type(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches('"'))
    })
}

fn extract_visible_html_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let blocked = Selector::parse("script, style, noscript, template, [hidden]").expect("static selector parses");
    let blocked_nodes = document.select(&blocked).flat_map(|node| node.text()).collect::<Vec<_>>();
    let mut parts = Vec::new();
    for text in document.root_element().text() {
        if blocked_nodes.iter().any(|blocked| std::ptr::eq(blocked.as_ptr(), text.as_ptr())) {
            continue;
        }
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.is_empty() {
            parts.push(normalized);
        }
    }
    parts.join(" ")
}

fn truncate_at_char_boundary(mut text: String, cap: usize) -> String {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}
