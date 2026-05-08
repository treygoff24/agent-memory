use serde::{Deserialize, Serialize};

use crate::oklch::OklchColor;
use crate::resolver::{ResolvedColor, Resolver};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorTokens {
    pub bg: OklchColor,
    pub surface: OklchColor,
    pub surface_2: OklchColor,
    pub border: OklchColor,
    pub border_soft: OklchColor,
    pub fg: OklchColor,
    pub fg_muted: OklchColor,
    pub fg_dim: OklchColor,
    pub accent: OklchColor,
    pub accent_soft: OklchColor,
    pub status_ok: OklchColor,
    pub status_warn: OklchColor,
    pub status_bad: OklchColor,
    pub status_info: OklchColor,
    pub glyph_review: OklchColor,
    pub glyph_recall: OklchColor,
    pub glyph_conflict: OklchColor,
    pub glyph_dream: OklchColor,
    pub glyph_due: OklchColor,
    pub glyph_memory: OklchColor,
    pub selection_gutter: OklchColor,
    pub palette_bg: OklchColor,
    pub palette_match: OklchColor,
}

impl ColorTokens {
    pub const REQUIRED: [&'static str; 23] = [
        "bg",
        "surface",
        "surface_2",
        "border",
        "border_soft",
        "fg",
        "fg_muted",
        "fg_dim",
        "accent",
        "accent_soft",
        "status_ok",
        "status_warn",
        "status_bad",
        "status_info",
        "glyph_review",
        "glyph_recall",
        "glyph_conflict",
        "glyph_dream",
        "glyph_due",
        "glyph_memory",
        "selection_gutter",
        "palette_bg",
        "palette_match",
    ];

    pub fn resolve(&self, resolver: &Resolver) -> ResolvedColorTokens {
        ResolvedColorTokens {
            bg: resolver.resolve_oklch(&self.bg),
            surface: resolver.resolve_oklch(&self.surface),
            surface_2: resolver.resolve_oklch(&self.surface_2),
            border: resolver.resolve_oklch(&self.border),
            border_soft: resolver.resolve_oklch(&self.border_soft),
            fg: resolver.resolve_oklch(&self.fg),
            fg_muted: resolver.resolve_oklch(&self.fg_muted),
            fg_dim: resolver.resolve_oklch(&self.fg_dim),
            accent: resolver.resolve_oklch(&self.accent),
            accent_soft: resolver.resolve_oklch(&self.accent_soft),
            status_ok: resolver.resolve_oklch(&self.status_ok),
            status_warn: resolver.resolve_oklch(&self.status_warn),
            status_bad: resolver.resolve_oklch(&self.status_bad),
            status_info: resolver.resolve_oklch(&self.status_info),
            glyph_review: resolver.resolve_oklch(&self.glyph_review),
            glyph_recall: resolver.resolve_oklch(&self.glyph_recall),
            glyph_conflict: resolver.resolve_oklch(&self.glyph_conflict),
            glyph_dream: resolver.resolve_oklch(&self.glyph_dream),
            glyph_due: resolver.resolve_oklch(&self.glyph_due),
            glyph_memory: resolver.resolve_oklch(&self.glyph_memory),
            selection_gutter: resolver.resolve_oklch(&self.selection_gutter),
            palette_bg: resolver.resolve_oklch(&self.palette_bg),
            palette_match: resolver.resolve_oklch(&self.palette_match),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedColorTokens {
    pub bg: ResolvedColor,
    pub surface: ResolvedColor,
    pub surface_2: ResolvedColor,
    pub border: ResolvedColor,
    pub border_soft: ResolvedColor,
    pub fg: ResolvedColor,
    pub fg_muted: ResolvedColor,
    pub fg_dim: ResolvedColor,
    pub accent: ResolvedColor,
    pub accent_soft: ResolvedColor,
    pub status_ok: ResolvedColor,
    pub status_warn: ResolvedColor,
    pub status_bad: ResolvedColor,
    pub status_info: ResolvedColor,
    pub glyph_review: ResolvedColor,
    pub glyph_recall: ResolvedColor,
    pub glyph_conflict: ResolvedColor,
    pub glyph_dream: ResolvedColor,
    pub glyph_due: ResolvedColor,
    pub glyph_memory: ResolvedColor,
    pub selection_gutter: ResolvedColor,
    pub palette_bg: ResolvedColor,
    pub palette_match: ResolvedColor,
}
