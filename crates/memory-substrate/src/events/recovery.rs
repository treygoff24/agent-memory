//! Event log recovery.

use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::events::framing::decode_line;
use crate::markdown::fsync_dir;

/// Recover an event log by truncating a single malformed trailing line.
///
/// Iterates raw bytes split on `b'\n'` without detoring through
/// `String::from_utf8_lossy`. Byte offsets are accumulated from raw slice
/// lengths so `U+FFFD` expansion can never skew the truncation point.
///
/// After truncation the file and its parent directory are fsynced.
pub fn recover_event_log(path: &Path) -> std::io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let bytes = fs::read(path)?;
    let (valid_end, malformed, malformed_start) = scan_raw_byte_lines(&bytes);

    let malformed_is_single_trailing_line =
        malformed == 1 && malformed_start == Some(valid_end) && valid_end < bytes.len();

    if malformed_is_single_trailing_line {
        let mut file = fs::OpenOptions::new().write(true).open(path)?;
        file.seek(SeekFrom::Start(valid_end as u64))?;
        file.set_len(valid_end as u64)?;
        file.flush()?;
        file.sync_all()?;
        if let Some(parent) = path.parent() {
            fsync_dir(parent)?;
        }
    } else if malformed > 0 {
        return Err(std::io::Error::other("non-final malformed event log line requires operator repair"));
    }
    Ok(malformed)
}

/// Walk raw byte lines and return `(valid_end, malformed_count, first_malformed_byte_offset)`.
///
/// Never detours through `String::from_utf8_lossy`. All byte positions are
/// computed directly from raw slice lengths, so the result is always a valid
/// byte offset into the original `bytes` slice.
///
/// `valid_end` is the byte offset immediately after the last consecutive valid
/// line at the start of the file. Once a malformed line is seen, subsequent
/// valid lines do NOT extend `valid_end` — they increment `malformed_count`
/// (so the caller can detect non-final malformed lines).
fn scan_raw_byte_lines(bytes: &[u8]) -> (usize, usize, Option<usize>) {
    let mut valid_end: usize = 0;
    let mut malformed: usize = 0;
    let mut malformed_start: Option<usize> = None;
    let mut valid_after_malformed: usize = 0;
    let mut pos: usize = 0;
    let mut remaining = bytes;

    loop {
        let (slice, rest, has_nl) = match remaining.iter().position(|&b| b == b'\n') {
            Some(nl) => (&remaining[..nl], &remaining[nl + 1..], true),
            None => (remaining, &b""[..], false),
        };

        let line_byte_len = slice.len() + usize::from(has_nl);

        let is_valid = if slice.is_empty() {
            // Blank line or trailing newline — valid separator.
            true
        } else {
            match std::str::from_utf8(slice) {
                Ok(text) => {
                    let text = text.trim_end_matches('\r');
                    text.is_empty() || decode_line(text).is_some()
                }
                Err(_) => false,
            }
        };

        if is_valid {
            if malformed_start.is_none() {
                // Still in the valid prefix — extend it.
                valid_end = pos + line_byte_len;
            } else {
                // Valid line AFTER a malformed line: non-final malformed detected.
                valid_after_malformed += 1;
            }
        } else {
            malformed += 1;
            if malformed_start.is_none() {
                malformed_start = Some(pos);
            }
        }

        pos += line_byte_len;
        remaining = rest;
        if remaining.is_empty() {
            break;
        }
    }

    // If there are valid lines after a malformed line, the malformed line is
    // non-final. Report it as additional malformed count so the caller refuses.
    let effective_malformed = if valid_after_malformed > 0 { malformed + 1 } else { malformed };
    (valid_end, effective_malformed, malformed_start)
}
