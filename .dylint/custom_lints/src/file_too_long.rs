//! Implementation of the [`FILE_TOO_LONG`](crate::FILE_TOO_LONG) lint.
//!
//! Warns when a Rust source file exceeds the configured line limit (default: 400).
//! The check runs when visiting items and reports each real source file once.

use std::collections::HashSet;

use rustc_ast::ast::Item;
use rustc_lint::{EarlyContext, EarlyLintPass, LintContext};
use rustc_session::impl_lint_pass;

/// Default maximum number of lines before the lint fires.
const MAX_LINES: usize = 400;

#[derive(Default)]
pub struct FileTooLong {
    seen_files: HashSet<u32>,
}

impl_lint_pass!(FileTooLong => [crate::FILE_TOO_LONG]);

impl EarlyLintPass for FileTooLong {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        let sm = cx.sess().source_map();
        let source_span = item.span.source_callsite();
        let source_file = sm.lookup_char_pos(source_span.lo()).file;

        if !source_file.name.is_real() {
            return;
        }

        let file_key = source_file.start_pos.0;
        if !self.seen_files.insert(file_key) {
            return;
        }

        let line_count = source_file.count_lines();
        if line_count > MAX_LINES {
            cx.lint(crate::FILE_TOO_LONG, |diag| {
                diag.primary_message(format!(
                    "file is {line_count} lines long (limit: {MAX_LINES}); consider splitting it into smaller modules"
                ));
                diag.span(source_span);
            });
        }
    }
}
