//! Implementation of the [`PROVABLY_UNNECESSARY_FALLBACK`](crate::PROVABLY_UNNECESSARY_FALLBACK) lint.
//!
//! Detects method calls such as `unwrap_or`, `or`, and `map_or` where the receiver is
//! visibly constructed as `Some(..)` or `Ok(..)`, making the fallback argument dead code.

use clippy_utils::diagnostics::span_lint;
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};

use crate::utils::{is_option, is_result, path_last_segment_name_from_expr, peel_expr};

rustc_session::declare_lint_pass!(ProvablyUnnecessaryFallback => [crate::PROVABLY_UNNECESSARY_FALLBACK]);

impl<'tcx> LateLintPass<'tcx> for ProvablyUnnecessaryFallback {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::MethodCall(segment, receiver, args, _) = expr.kind else {
            return;
        };

        let method = segment.ident.as_str();
        if !matches!(method.as_ref(), "unwrap_or" | "unwrap_or_else" | "or" | "or_else" | "map_or" | "map_or_else") {
            return;
        }

        let Some(variant) = guaranteed_success_variant(cx, receiver) else {
            return;
        };

        let is_supported_arity = match method.as_ref() {
            "unwrap_or" | "unwrap_or_else" | "or" | "or_else" => args.len() == 1,
            "map_or" | "map_or_else" => args.len() == 2,
            _ => false,
        };

        if !is_supported_arity {
            return;
        }

        let message = match variant {
            GuaranteedVariant::Some => "fallback is unnecessary because receiver is `Some(..)`",
            GuaranteedVariant::Ok => "fallback is unnecessary because receiver is `Ok(..)`",
        };

        span_lint(cx, crate::PROVABLY_UNNECESSARY_FALLBACK, expr.span, message);
    }
}

#[derive(Copy, Clone)]
enum GuaranteedVariant {
    Some,
    Ok,
}

fn guaranteed_success_variant<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> Option<GuaranteedVariant> {
    let expr = peel_expr(expr);

    let ExprKind::Call(callee, ctor_args) = expr.kind else {
        return None;
    };

    if ctor_args.len() != 1 {
        return None;
    }

    let ctor_name = path_last_segment_name_from_expr(callee)?;
    let expr_ty = cx.typeck_results().expr_ty(expr);

    match ctor_name.as_str() {
        "Some" if is_option(cx, expr_ty) => Some(GuaranteedVariant::Some),
        "Ok" if is_result(cx, expr_ty) => Some(GuaranteedVariant::Ok),
        _ => None,
    }
}
