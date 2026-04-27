#![no_main]

use libfuzzer_sys::fuzz_target;
use memory_substrate::merge::{MergeInput, merge_markdown};

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = merge_markdown(MergeInput { base: text, ours: text, theirs: text, path: "fuzz.md" });
    }
});
