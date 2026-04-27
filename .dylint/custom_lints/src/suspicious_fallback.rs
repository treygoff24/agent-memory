//! Implementation of the [`SUSPICIOUS_FALLBACK`](crate::SUSPICIOUS_FALLBACK) lint.
//!
//! Detects `match` expressions on `Result`/`Option` where a failure arm (`Err`/`None`)
//! visibly recovers to a success variant (`Ok(..)`/`Some(..)`), suggesting the fallback
//! may be hiding unintended silent error recovery.

use clippy_utils::diagnostics::span_lint;
use rustc_hir::{Expr, ExprKind, Pat, PatExpr, PatExprKind, PatKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::{sym, Symbol};

use crate::utils::{is_option, is_result, path_last_segment_name_from_expr, path_last_segment_name_from_qpath};

rustc_session::declare_lint_pass!(SuspiciousFallback => [crate::SUSPICIOUS_FALLBACK]);

impl<'tcx> LateLintPass<'tcx> for SuspiciousFallback {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Match(scrutinee, arms, _) = expr.kind else {
            return;
        };

        let Some(family) = recovery_family(cx, scrutinee) else {
            return;
        };

        for arm in arms {
            if !is_failure_arm_pattern(arm.pat, family) {
                continue;
            }

            if !arm_recovers_to_success(cx, arm.body, family) {
                continue;
            }

            let message = match family {
                RecoveryFamily::Result => "suspicious fallback: `Err` arm recovers to `Ok(..)`",
                RecoveryFamily::Option => "suspicious fallback: `None` arm recovers to `Some(..)`",
            };

            span_lint(cx, crate::SUSPICIOUS_FALLBACK, arm.body.span, message);
        }
    }
}

#[derive(Copy, Clone)]
enum RecoveryFamily {
    Result,
    Option,
}

fn recovery_family<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) -> Option<RecoveryFamily> {
    let ty = cx.typeck_results().expr_ty(expr);

    if is_result(cx, ty) {
        Some(RecoveryFamily::Result)
    } else if is_option(cx, ty) {
        Some(RecoveryFamily::Option)
    } else {
        None
    }
}

fn is_failure_arm_pattern(pat: &Pat<'_>, family: RecoveryFamily) -> bool {
    match pattern_variant_name(pat) {
        Some(sym::Err) => matches!(family, RecoveryFamily::Result),
        Some(sym::None) => matches!(family, RecoveryFamily::Option),
        _ => false,
    }
}

fn pattern_variant_name(pat: &Pat<'_>) -> Option<Symbol> {
    match pat.kind {
        PatKind::TupleStruct(ref qpath, ..) | PatKind::Struct(ref qpath, ..) => {
            path_last_segment_name_from_qpath(qpath)
        }
        PatKind::Expr(PatExpr { kind: PatExprKind::Path(qpath), .. }) => path_last_segment_name_from_qpath(qpath),
        PatKind::Binding(_, _, _, Some(inner)) | PatKind::Ref(inner, _) => pattern_variant_name(inner),
        PatKind::Or(pats) => pats.iter().find_map(|inner| pattern_variant_name(inner)),
        _ => None,
    }
}

fn arm_recovers_to_success<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>, family: RecoveryFamily) -> bool {
    match expr.kind {
        ExprKind::DropTemps(inner) => arm_recovers_to_success(cx, inner, family),
        ExprKind::Block(block, _) => block.expr.map(|tail| arm_recovers_to_success(cx, tail, family)).unwrap_or(false),
        ExprKind::Ret(Some(inner)) => arm_recovers_to_success(cx, inner, family),
        ExprKind::If(_, then_expr, else_expr) => {
            arm_recovers_to_success(cx, then_expr, family)
                || else_expr.map(|inner| arm_recovers_to_success(cx, inner, family)).unwrap_or(false)
        }
        ExprKind::Match(_, arms, _) => arms.iter().any(|arm| arm_recovers_to_success(cx, arm.body, family)),
        ExprKind::Call(callee, ctor_args) if ctor_args.len() == 1 => {
            let ctor = path_last_segment_name_from_expr(callee);
            match family {
                RecoveryFamily::Result if ctor == Some(sym::Ok) => {
                    let ty = cx.typeck_results().expr_ty(expr);
                    is_result(cx, ty)
                }
                RecoveryFamily::Option if ctor == Some(sym::Some) => {
                    let ty = cx.typeck_results().expr_ty(expr);
                    is_option(cx, ty)
                }
                _ => false,
            }
        }
        _ => false,
    }
}
