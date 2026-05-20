//! Rust boundary checks for Stream A.

use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if let Err(err) = run(&root) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run(root: &Path) -> Result<(), String> {
    check_no_absolute_test_paths(root)?;
    check_no_unwrap_expect(root)?;
    Ok(())
}

fn check_no_absolute_test_paths(root: &Path) -> Result<(), String> {
    let tests = required_dir(root, "crates/memory-substrate/tests")?;
    for file in rust_files(&tests)? {
        let text = std::fs::read_to_string(&file).map_err(|err| err.to_string())?;
        let tokens = rust_tokens(&file, &text)?;
        if let Some(line) = absolute_path_literal_lines(&tokens).first() {
            return Err(format!("absolute path literal in {}:{}", file.display(), line));
        }
    }
    Ok(())
}

fn check_no_unwrap_expect(root: &Path) -> Result<(), String> {
    let src = required_dir(root, "crates/memory-substrate/src")?;
    for file in rust_files(&src)? {
        let text = std::fs::read_to_string(&file).map_err(|err| err.to_string())?;
        let lines: Vec<&str> = text.lines().collect();
        let tokens = rust_tokens(&file, &text)?;
        let string_ranges = string_literal_line_ranges(&tokens);
        let violations: Vec<MethodCallLocation> = forbidden_method_call_locations(&tokens)
            .into_iter()
            .filter(|location| {
                location.line == 0 || !has_adjacent_justification_comment(&lines, &string_ranges, location)
            })
            .collect();
        if let Some(location) = violations.first() {
            return Err(format!("raw unwrap/expect in {}:{}", file.display(), location.line));
        }
    }
    Ok(())
}

fn required_dir(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    let metadata =
        std::fs::metadata(&path).map_err(|err| format!("inspect required directory {}: {err}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!("required path is not a directory: {}", path.display()));
    }
    Ok(path)
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.map_err(|err| format!("walk {}: {err}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(files)
}

fn rust_tokens(file: &Path, text: &str) -> Result<TokenStream, String> {
    syn::parse_file(text).map_err(|err| format!("parse Rust {}: {err}", file.display()))?;
    text.parse::<TokenStream>().map_err(|err| format!("tokenize Rust {}: {err}", file.display()))
}

fn absolute_path_literal_lines(tokens: &TokenStream) -> Vec<usize> {
    let mut lines = Vec::new();
    collect_absolute_path_literal_lines(tokens, &mut lines);
    lines
}

#[derive(Clone, Copy)]
struct LineRange {
    start: usize,
    end: usize,
}

fn string_literal_line_ranges(tokens: &TokenStream) -> Vec<LineRange> {
    let mut ranges = Vec::new();
    collect_string_literal_line_ranges(tokens, &mut ranges);
    ranges
}

fn collect_string_literal_line_ranges(tokens: &TokenStream, ranges: &mut Vec<LineRange>) {
    for tree in tokens.clone() {
        match tree {
            TokenTree::Literal(literal) if token_string_value(&literal.to_string()).is_some() => {
                let span = literal.span();
                let start = span.start().line;
                let end = span.end().line;
                if start < end {
                    ranges.push(LineRange { start, end });
                }
            }
            TokenTree::Group(group) => collect_string_literal_line_ranges(&group.stream(), ranges),
            TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

fn collect_absolute_path_literal_lines(tokens: &TokenStream, lines: &mut Vec<usize>) {
    for tree in tokens.clone() {
        match tree {
            TokenTree::Literal(literal) => {
                if literal_has_absolute_path(&literal.to_string()) {
                    lines.push(literal.span().start().line);
                }
            }
            TokenTree::Group(group) => collect_absolute_path_literal_lines(&group.stream(), lines),
            TokenTree::Ident(_) | TokenTree::Punct(_) => {}
        }
    }
}

fn literal_has_absolute_path(token: &str) -> bool {
    token_string_value(token).is_some_and(|literal| {
        const ROOTS: &[&str] = &["/Users", "/home", "/var", "/tmp"];
        // Multi-line raw strings can carry absolute paths on interior lines
        // (e.g. r#"\n/tmp/memorum\n"#), so scan every line, not just the head.
        literal.lines().any(|line| ROOTS.iter().any(|root| line == *root || line.starts_with(&format!("{root}/"))))
    })
}

#[derive(Clone, Copy)]
struct MethodCallLocation {
    line: usize,
    column: usize,
}

fn forbidden_method_call_locations(tokens: &TokenStream) -> Vec<MethodCallLocation> {
    let mut locations = Vec::new();
    collect_forbidden_method_call_locations(tokens, &mut locations);
    locations
}

fn collect_forbidden_method_call_locations(tokens: &TokenStream, locations: &mut Vec<MethodCallLocation>) {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if is_forbidden_method(tree) && is_method_or_ufcs_call(&trees, index) {
            let start = tree.span().start();
            locations.push(MethodCallLocation { line: start.line, column: start.column });
        }
    }
    for tree in trees {
        if let TokenTree::Group(group) = tree {
            collect_forbidden_method_call_locations(&group.stream(), locations);
        }
    }
}

fn is_method_or_ufcs_call(trees: &[TokenTree], method_index: usize) -> bool {
    if method_index >= 2 && is_colon(&trees[method_index - 2]) && is_colon(&trees[method_index - 1]) {
        return true;
    }
    method_index > 0 && is_dot(&trees[method_index - 1]) && call_arguments_start_at(trees, method_index + 1).is_some()
}

fn call_arguments_start_at(trees: &[TokenTree], mut index: usize) -> Option<usize> {
    if trees.get(index).is_some_and(is_parenthesized_group) {
        return Some(index);
    }
    if !(trees.get(index).is_some_and(is_colon) && trees.get(index + 1).is_some_and(is_colon)) {
        return None;
    }
    index += 2;
    if !matches!(trees.get(index), Some(TokenTree::Punct(punct)) if punct.as_char() == '<') {
        return None;
    }
    let mut angle_depth = 0usize;
    while let Some(tree) = trees.get(index) {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == '<' => angle_depth += 1,
            TokenTree::Punct(punct) if punct.as_char() == '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                if angle_depth == 0 {
                    index += 1;
                    break;
                }
            }
            _ => {}
        }
        index += 1;
    }
    trees.get(index).is_some_and(is_parenthesized_group).then_some(index)
}

fn is_dot(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == '.')
}

fn is_colon(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ':')
}

fn is_forbidden_method(tree: &TokenTree) -> bool {
    match tree {
        TokenTree::Ident(ident) => {
            let name = ident.to_string();
            let normalized = name.strip_prefix("r#").unwrap_or(&name);
            normalized == "unwrap" || normalized == "expect"
        }
        _ => false,
    }
}

fn is_parenthesized_group(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis)
}

fn has_adjacent_justification_comment(
    lines: &[&str],
    string_ranges: &[LineRange],
    location: &MethodCallLocation,
) -> bool {
    let line = location.line - 1;
    justification_marker_in_comment_after(lines[line], location.column)
        || line.checked_sub(1).is_some_and(|previous| is_comment_only_justification(lines, string_ranges, previous))
        || is_comment_only_justification(lines, string_ranges, line + 1)
}

fn justification_marker_in_comment_after(line: &str, minimum_column: usize) -> bool {
    comment_start_after(line, minimum_column).is_some_and(|index| has_justification_marker(&line[index + 2..]))
}

fn is_comment_only_justification(lines: &[&str], string_ranges: &[LineRange], line: usize) -> bool {
    let Some(value) = lines.get(line) else {
        return false;
    };
    let line_number = line + 1;
    if string_ranges.iter().any(|range| range.start < line_number && line_number <= range.end) {
        return false;
    }
    let trimmed = value.trim_start();
    trimmed.starts_with("//") && has_justification_marker(trimmed.trim_start_matches('/'))
}

fn has_justification_marker(value: &str) -> bool {
    value.contains("unwrap-justified:") || value.contains("expect-justified:")
}

fn comment_start_after(line: &str, minimum_column: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'r' {
            if let Some((_literal, end)) = parse_raw_string_literal(line, index) {
                index = end;
                continue;
            }
        }
        if bytes[index] == b'"' {
            if let Some((_literal, end)) = parse_string_literal(line, index) {
                index = end;
                continue;
            }
        }
        if bytes[index] == b'/' && bytes[index + 1] == b'/' {
            if index >= minimum_column {
                return Some(index);
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    None
}

fn token_string_value(token: &str) -> Option<String> {
    if token.starts_with("br") {
        parse_raw_string_literal(token, 1).map(|(literal, _end)| literal)
    } else if token.starts_with("cr") {
        parse_raw_string_literal(token, 1).map(|(literal, _end)| literal)
    } else if token.starts_with("b\"") {
        parse_string_literal(token, 1).map(|(literal, _end)| literal)
    } else if token.starts_with("c\"") {
        parse_string_literal(token, 1).map(|(literal, _end)| literal)
    } else if token.starts_with('r') {
        parse_raw_string_literal(token, 0).map(|(literal, _end)| literal)
    } else if token.starts_with('"') {
        parse_string_literal(token, 0).map(|(literal, _end)| literal)
    } else {
        None
    }
}

fn parse_raw_string_literal(line: &str, start: usize) -> Option<(String, usize)> {
    let bytes = line.as_bytes();
    let mut cursor = start + 1;
    let mut hashes = 0;
    while bytes.get(cursor) == Some(&b'#') {
        hashes += 1;
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    let content_start = cursor + 1;
    cursor = content_start;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes.get(cursor + 1..cursor + 1 + hashes).is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some((line[content_start..cursor].to_string(), cursor + 1 + hashes));
        }
        cursor += 1;
    }
    None
}

fn parse_string_literal(line: &str, start: usize) -> Option<(String, usize)> {
    let bytes = line.as_bytes();
    let mut literal = String::new();
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == b'"' {
            return Some((literal, cursor + 1));
        }
        if byte == b'\\' {
            cursor += 1;
            if cursor >= bytes.len() {
                return None;
            }
            match bytes[cursor] {
                b'n' => literal.push('\n'),
                b'r' => literal.push('\r'),
                b't' => literal.push('\t'),
                b'0' => literal.push('\0'),
                b'\\' => literal.push('\\'),
                b'"' => literal.push('"'),
                b'x' => {
                    if let Some((decoded, next)) = decode_hex_escape(bytes, cursor) {
                        literal.push(decoded);
                        cursor = next;
                        continue;
                    }
                    literal.push('x');
                }
                b'u' => {
                    if let Some((decoded, next)) = decode_unicode_escape(line, cursor) {
                        literal.push(decoded);
                        cursor = next;
                        continue;
                    }
                    literal.push('u');
                }
                b'\n' => {
                    cursor = skip_string_continuation_whitespace(bytes, cursor + 1);
                    continue;
                }
                b'\r' => {
                    let next = if bytes.get(cursor + 1) == Some(&b'\n') { cursor + 2 } else { cursor + 1 };
                    cursor = skip_string_continuation_whitespace(bytes, next);
                    continue;
                }
                other => literal.push(other as char),
            }
        } else {
            literal.push(byte as char);
        }
        cursor += 1;
    }
    None
}

fn skip_string_continuation_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        cursor += 1;
    }
    cursor
}

fn decode_hex_escape(bytes: &[u8], escape_start: usize) -> Option<(char, usize)> {
    let high = hex_value(*bytes.get(escape_start + 1)?)?;
    let low = hex_value(*bytes.get(escape_start + 2)?)?;
    Some((char::from((high << 4) | low), escape_start + 3))
}

fn decode_unicode_escape(line: &str, escape_start: usize) -> Option<(char, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(escape_start + 1) != Some(&b'{') {
        return None;
    }
    let digits_start = escape_start + 2;
    let mut digits_end = digits_start;
    while digits_end < bytes.len() && bytes[digits_end] != b'}' {
        digits_end += 1;
    }
    if bytes.get(digits_end) != Some(&b'}') {
        return None;
    }
    let digits: String = line[digits_start..digits_end].chars().filter(|ch| *ch != '_').collect();
    let value = u32::from_str_radix(&digits, 16).ok()?;
    Some((char::from_u32(value)?, digits_end + 1))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_fails_when_required_tree_is_missing() {
        let temp = tempfile::tempdir().expect("tempdir");

        let error = run(temp.path()).expect_err("empty root must fail closed");

        assert!(error.contains("crates/memory-substrate/tests"));
    }

    #[test]
    fn absolute_path_literal_is_not_exempted_by_roots_string_marker() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn bad_path() {
    let path = "/tmp/memorum";
    let marker = "Roots";
    assert!(!path.is_empty() && !marker.is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn absolute_path_literal_inside_roots_builder_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn roots_fixture() {
    let _roots = Roots::new("/tmp/memorum/repo", "/tmp/memorum/runtime");
}
"#,
        );

        let error = run(temp.path()).expect_err("absolute paths are rejected even in Roots fixtures");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn multiline_raw_absolute_path_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r##"#[test]
fn raw_path() {
    let path = r#"
/tmp/memorum
"#;
    assert!(!path.is_empty());
}
"##,
        );

        let error = run(temp.path()).expect_err("raw absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn c_string_absolute_path_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn c_path() {
    let path = c"/tmp/memorum";
    assert!(!path.to_bytes().is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("c-string absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn exact_tmp_root_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn tmp_root() {
    let path = "/tmp";
    assert!(!path.is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("exact /tmp path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn escaped_absolute_path_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn escaped_path() {
    let path = "\x2Ftmp\u{2f}memorum";
    assert!(!path.is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("escaped absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn unicode_escape_with_underscore_absolute_path_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn unicode_escaped_path() {
    let path = "\u{2_f}tmp/memorum";
    assert!(!path.is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("unicode escaped absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn string_continuation_absolute_path_literal_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/tests/path.rs",
            r#"#[test]
fn continuation_path() {
    let path = "/t\
        mp/memorum";
    assert!(!path.is_empty());
}
"#,
        );

        let error = run(temp.path()).expect_err("continued absolute path should fail");

        assert!(error.contains("absolute path literal"));
    }

    #[test]
    fn expect_call_with_whitespace_and_non_comment_marker_fails() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    let _marker = "expect-justified: fixture";
    input.expect ("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("raw expect should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_with_dot_whitespace_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    input
        . expect("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("syn catches whitespace before method");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_inside_macro_tokens_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"macro_rules! unwrap_value {
    ($value:expr) => {
        $value.expect("value present")
    };
}
"#,
        );

        let error = run(temp.path()).expect_err("macro expect should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn ufcs_unwrap_call_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    Option::unwrap(input)
}
"#,
        );

        let error = run(temp.path()).expect_err("UFCS unwrap should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn raw_identifier_expect_call_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    input.r#expect("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("raw identifier expect should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn method_turbofish_expect_call_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    input.expect::<u8>("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("method turbofish expect should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn ufcs_turbofish_unwrap_call_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    Option::unwrap::<u8>(input)
}
"#,
        );

        let error = run(temp.path()).expect_err("UFCS turbofish unwrap should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn ufcs_unwrap_function_item_is_rejected() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    let unwrap = Option::unwrap;
    unwrap(input)
}
"#,
        );

        let error = run(temp.path()).expect_err("UFCS unwrap function item should fail");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_is_not_justified_by_marker_inside_string_literal() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    let _url = "https://example.test// expect-justified: not a comment"; input.expect("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("string marker should not justify expect");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_is_not_justified_by_marker_inside_multiline_string_prefix() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    let _marker = "
// expect-justified: not a comment"; input.expect("value present")
}
"#,
        );

        let error = run(temp.path()).expect_err("multiline string marker should not justify expect");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_is_not_justified_by_adjacent_raw_string_marker_line() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r##"fn value(input: Option<u8>) -> u8 {
    let _marker = r#"
// expect-justified: not a comment
"#; input.expect("value present")
}
"##,
        );

        let error = run(temp.path()).expect_err("raw string marker line should not justify expect");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_is_not_justified_by_raw_string_closing_marker_line() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r##"fn value(input: Option<u8>) -> u8 {
    let _marker = r#"
// expect-justified: not a comment"#;
    input.expect("value present")
}
"##,
        );

        let error = run(temp.path()).expect_err("raw string closing marker line should not justify expect");

        assert!(error.contains("raw unwrap/expect"));
    }

    #[test]
    fn expect_call_with_adjacent_comment_marker_is_allowed() {
        let temp = fixture_root();
        write_fixture(
            temp.path(),
            "crates/memory-substrate/src/lib.rs",
            r#"fn value(input: Option<u8>) -> u8 {
    input.expect ("value present")
    // expect-justified: fixture invariant
}
"#,
        );

        run(temp.path()).expect("comment-only justification is allowed");
    }

    fn fixture_root() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("crates/memory-substrate/tests")).expect("tests dir");
        std::fs::create_dir_all(temp.path().join("crates/memory-substrate/src")).expect("src dir");
        write_fixture(temp.path(), "crates/memory-substrate/src/lib.rs", "");
        temp
    }

    fn write_fixture(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture parent dir");
        std::fs::write(path, text).expect("write fixture");
    }
}
