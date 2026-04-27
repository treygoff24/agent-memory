//! Shared utilities used by multiple lints in this crate.

use rustc_hir::{Expr, ExprKind, QPath};
use rustc_lint::LateContext;
use rustc_middle::ty;
use rustc_span::{sym, Symbol};

pub fn is_option(cx: &LateContext<'_>, ty: ty::Ty<'_>) -> bool {
    if let ty::Adt(adt_def, _) = ty.kind() {
        cx.tcx.is_diagnostic_item(sym::Option, adt_def.did())
    } else {
        false
    }
}

pub fn is_result(cx: &LateContext<'_>, ty: ty::Ty<'_>) -> bool {
    if let ty::Adt(adt_def, _) = ty.kind() {
        cx.tcx.is_diagnostic_item(sym::Result, adt_def.did())
    } else {
        false
    }
}

pub fn path_last_segment_name_from_qpath(qpath: &QPath<'_>) -> Option<Symbol> {
    match qpath {
        QPath::Resolved(_, path) => path.segments.last().map(|segment| segment.ident.name),
        QPath::TypeRelative(_, segment) => Some(segment.ident.name),
        QPath::LangItem(_, _) => None,
    }
}

pub fn path_last_segment_name_from_expr(expr: &Expr<'_>) -> Option<Symbol> {
    let ExprKind::Path(qpath) = expr.kind else {
        return None;
    };

    path_last_segment_name_from_qpath(&qpath)
}

pub fn peel_expr<'tcx>(mut expr: &'tcx Expr<'tcx>) -> &'tcx Expr<'tcx> {
    loop {
        match expr.kind {
            ExprKind::DropTemps(inner) => {
                expr = inner;
            }
            ExprKind::Block(block, _) if block.stmts.is_empty() => {
                if let Some(inner) = block.expr {
                    expr = inner;
                } else {
                    return expr;
                }
            }
            _ => return expr,
        }
    }
}
