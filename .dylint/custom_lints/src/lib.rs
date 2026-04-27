//! # custom_lints
//!
//! A [Dylint](https://github.com/trailofbits/dylint) library that provides custom lints
//! complementing the baseline Clippy configuration.
//!
//! ## Lints
//!
//! | Lint | Default level | Summary |
//! |------|---------------|---------|
//! | [`FILE_TOO_LONG`] | `warn` | Source file exceeds 400 lines |
//! | [`SUSPICIOUS_FALLBACK`] | `warn` | `Err`/`None` arm in a `match` visibly recovers to `Ok(..)`/`Some(..)` |
//! | [`PROVABLY_UNNECESSARY_FALLBACK`] | `warn` | Fallback call on a receiver that is visibly `Ok(..)`/`Some(..)` |
//!
//! ## Usage
//!
//! ```bash
//! cargo +nightly-2025-09-18 dylint --lib custom_lints --path .dylint/custom_lints
//! ```

#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_ast;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod file_too_long;
mod suspicious_fallback;
mod unnecessary_fallback;
mod utils;

dylint_linting::dylint_library!();

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Warns when a source file exceeds 400 lines.
    ///
    /// ### Why is this bad?
    ///
    /// Large files are harder to navigate, review, and reason about.
    /// Files over 400 lines are a signal to consider splitting the code
    /// into smaller, more focused modules.
    ///
    /// ### Suppression
    ///
    /// If the size is intentional (e.g. a large generated file or test fixture),
    /// use:
    ///
    /// ```rust
    /// #![allow(file_too_long)]
    /// ```
    pub FILE_TOO_LONG,
    Warn,
    "source file exceeds the recommended line limit"
}

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Flags fallback expressions that are provably unnecessary.
    ///
    /// ### Why is this bad?
    ///
    /// Fallback branches can hide dead logic and make intent less clear.
    ///
    /// ### High-confidence scope
    ///
    /// This lint intentionally only checks calls where the receiver is visibly `Some(..)` or
    /// `Ok(..)`, so the fallback cannot be used.
    ///
    /// ### Suppression
    ///
    /// If a warning is intentional, use:
    ///
    /// ```rust
    /// #[allow(provably_unnecessary_fallback)]
    /// let value = Some(1).unwrap_or(2);
    /// ```
    pub PROVABLY_UNNECESSARY_FALLBACK,
    Warn,
    "fallback code that is provably unreachable"
}

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Flags suspicious fallback control-flow where a failure branch recovers to success.
    ///
    /// ### Why is this bad?
    ///
    /// Manual fallback paths can hide operational complexity and make behavior harder to reason about.
    ///
    /// ### High-confidence scope
    ///
    /// This lint only checks `match` on `Result`/`Option`, and only reports failure arms (`Err`/`None`)
    /// when the arm body clearly produces success (`Ok(..)`/`Some(..)`).
    ///
    /// ### Suppression
    ///
    /// If a warning is intentional, use:
    ///
    /// ```rust
    /// #[allow(suspicious_fallback)]
    /// let value = match maybe_open() {
    ///     Ok(v) => Ok(v),
    ///     Err(_) => Ok(default_value()),
    /// };
    /// ```
    pub SUSPICIOUS_FALLBACK,
    Warn,
    "suspicious fallback from failure to success"
}

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[FILE_TOO_LONG, PROVABLY_UNNECESSARY_FALLBACK, SUSPICIOUS_FALLBACK]);
    lint_store.register_early_pass(|| Box::new(file_too_long::FileTooLong::default()));
    lint_store.register_late_pass(|_| Box::new(unnecessary_fallback::ProvablyUnnecessaryFallback));
    lint_store.register_late_pass(|_| Box::new(suspicious_fallback::SuspiciousFallback));
}
