//! Per-harness source parsers. Each parser reads a discovered memory root and
//! emits a `Vec<ParsedMemory>` for the pipeline to dedup, plan, and write.
//!
//! - `claude` (T02): parses `~/.claude/projects/<encoded>/memory/<topic>.md`
//!   files plus `user_profile.md`. Handles the single-fact vs. multi-section
//!   dossier split.
//! - `codex` (T03): parses `~/.codex/memories/MEMORY.md` Task Groups plus
//!   `extensions/ad_hoc/notes/*.md`.

pub mod claude;
pub mod codex;
