/// Escape a string for embedding inside a JSON double-quoted string value.
///
/// Handles the mandatory JSON escapes (`"`, `\`, `\n`, `\r`, `\t`) and escapes
/// every other ASCII control character as `\uXXXX`. Non-ASCII printable Unicode
/// passes through unchanged.
pub(crate) fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => escaped.push_str(&format!("\\u{:04x}", character as u32)),
            character => escaped.push(character),
        }
    }
    escaped
}

pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("block_on: failed to build single-threaded tokio runtime")
        .block_on(future)
}
