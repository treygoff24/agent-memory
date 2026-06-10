//! Use-driven memory strength (memory-dynamics-v0.1 §2).
//!
//! ```text
//! strength(m) = w_f·freq_norm(m) + w_r·recency(m) + w_c·corroboration(m)
//! ```
//!
//! - `freq_norm(m) = log1p(recall_count_30d) / log1p(max_recall_count_30d_active)`
//!   — log-scaled so a 50-hit memory does not drown a 10-hit one 5:1; normalized
//!   over the active candidate pool. `0` when the pool max is `0`.
//! - `recency(m) = exp(-days_since_last_recall / τ)`, `τ = 14d` default. `0` when
//!   the memory has never been recalled.
//! - `corroboration(m)` — the binary `≥2 distinct source_harness` cross-source
//!   signal, identical to `reality_check::cross_source_corroboration`.
//! - Default weights `0.45 / 0.35 / 0.20`, **truly renormalized** (divided by
//!   their sum), not validate-or-discard. Result clamped to `[0, 1]`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Strength component weights (spec §2).
///
/// **True renormalization.** [`StrengthWeights::renormalized`] divides each weight
/// by their sum so any non-negative, positive-sum triple is honored as-given
/// proportions. This is deliberately *not* RC `ScoreWeights::normalized_or_default`'s
/// posture (validate `sum≈1.0`, else silently discard the user's weights) — that
/// is a footgun for a dogfood-tunable surface. RC's behavior is unchanged; this is
/// a separate type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrengthWeights {
    #[serde(default = "default_frequency")]
    pub frequency: f64,
    #[serde(default = "default_recency")]
    pub recency: f64,
    #[serde(default = "default_corroboration")]
    pub corroboration: f64,
}

impl StrengthWeights {
    pub const DEFAULT: Self = Self { frequency: 0.45, recency: 0.35, corroboration: 0.20 };

    /// Renormalize so the three weights sum to 1, preserving their ratios.
    ///
    /// Guard: every weight must be finite and `>= 0` and the sum must be `> 0`.
    /// On a malformed triple, fall back to [`StrengthWeights::DEFAULT`] with a
    /// `tracing::warn!` (spec §2).
    pub fn renormalized(self) -> Self {
        let sum = self.frequency + self.recency + self.corroboration;
        let all_valid = [self.frequency, self.recency, self.corroboration]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0);
        if all_valid && sum > 0.0 {
            Self {
                frequency: self.frequency / sum,
                recency: self.recency / sum,
                corroboration: self.corroboration / sum,
            }
        } else {
            tracing::warn!(
                frequency = self.frequency,
                recency = self.recency,
                corroboration = self.corroboration,
                "invalid dynamics strength weights; falling back to defaults"
            );
            Self::DEFAULT
        }
    }
}

impl Default for StrengthWeights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn default_frequency() -> f64 {
    0.45
}

fn default_recency() -> f64 {
    0.35
}

fn default_corroboration() -> f64 {
    0.20
}

/// The per-memory inputs the strength function reduces (spec §2.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrengthFacts {
    /// `recall_hit` count over the trailing 30 days.
    pub recall_count_30d: u32,
    /// Most recent `recall_hit`; `None` if never recalled.
    pub last_recalled_at: Option<DateTime<Utc>>,
    /// Pool maximum `recall_count_30d` over the active candidate set, the
    /// `freq_norm` denominator.
    pub max_recall_30d_active: u32,
    /// Distinct `source_harness` count across the supersession chain.
    pub distinct_sources: u32,
}

/// `freq_norm(m) = log1p(count) / log1p(pool_max)`; `0` when the pool max is `0`.
pub fn frequency_norm(recall_count_30d: u32, max_recall_30d_active: u32) -> f64 {
    if max_recall_30d_active == 0 {
        return 0.0;
    }
    let denominator = f64::from(max_recall_30d_active).ln_1p();
    if denominator <= 0.0 {
        return 0.0;
    }
    (f64::from(recall_count_30d).ln_1p() / denominator).clamp(0.0, 1.0)
}

/// `recency(m) = exp(-days_since_last_recall / τ)`; `0` when never recalled.
///
/// `tau_days <= 0` is treated as "recency disabled" and returns `0`, never a
/// division by zero or a NaN.
pub fn recency(last_recalled_at: Option<DateTime<Utc>>, now: DateTime<Utc>, tau_days: f64) -> f64 {
    let Some(last) = last_recalled_at else {
        return 0.0;
    };
    // NaN must also land here, not just non-positive values.
    if !tau_days.is_finite() || tau_days <= 0.0 {
        return 0.0;
    }
    let days = now.signed_duration_since(last).num_seconds().max(0) as f64 / 86_400.0;
    (-days / tau_days).exp().clamp(0.0, 1.0)
}

/// The binary `≥2 distinct source_harness` cross-source signal (spec §2).
pub fn corroboration(distinct_sources: u32) -> f64 {
    if distinct_sources >= 2 {
        1.0
    } else {
        0.0
    }
}

/// Compute `strength(m) ∈ [0, 1]` from its inputs (spec §2).
///
/// `weights` are renormalized before use. `tau_days` is the recency
/// time-constant; a non-positive value simply zeroes the recency term.
pub fn strength(facts: StrengthFacts, weights: StrengthWeights, tau_days: f64, now: DateTime<Utc>) -> f64 {
    let weights = weights.renormalized();
    let freq = frequency_norm(facts.recall_count_30d, facts.max_recall_30d_active);
    let rec = recency(facts.last_recalled_at, now, tau_days);
    let corr = corroboration(facts.distinct_sources);
    (weights.frequency * freq + weights.recency * rec + weights.corroboration * corr).clamp(0.0, 1.0)
}

/// `strength_points(m) = min(floor(strength × alpha_points), alpha_points - 1)`
/// (memory-dynamics-v0.1 §3 amended 2026-06-10).
///
/// The bounded additive ranking component. Because `strength ∈ [0, 1]`, the result
/// is in `[0, alpha_points - 1]` when `alpha_points > 0`, preserving the
/// invariant that strength can never tie or overcome a structural ranking gap
/// `>= alpha_points`.
pub fn strength_points(strength: f64, alpha_points: u32) -> i64 {
    if alpha_points == 0 {
        return 0;
    }
    let cap = alpha_points - 1;
    ((strength.clamp(0.0, 1.0) * f64::from(alpha_points)).floor() as u32).min(cap) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamics::DEFAULT_TAU_DAYS;
    use chrono::TimeZone;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn never_recalled_has_zero_frequency_and_recency() {
        let facts = StrengthFacts {
            recall_count_30d: 0,
            last_recalled_at: None,
            max_recall_30d_active: 10,
            distinct_sources: 1,
        };
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        assert_eq!(strength(facts, StrengthWeights::DEFAULT, DEFAULT_TAU_DAYS, now), 0.0);
    }

    #[test]
    fn pool_max_zero_gives_zero_frequency_norm() {
        assert_eq!(frequency_norm(0, 0), 0.0);
        assert_eq!(frequency_norm(5, 0), 0.0);
    }

    #[test]
    fn single_memory_pool_saturates_frequency_norm() {
        // count == pool max → log1p(x)/log1p(x) == 1.0 for any positive x.
        assert!((frequency_norm(7, 7) - 1.0).abs() < 1e-12);
        assert!((frequency_norm(1, 1) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn log_scaling_compresses_high_counts() {
        // A 50-hit memory must NOT be 5x a 10-hit memory (that would be linear).
        let ten = frequency_norm(10, 50);
        let fifty = frequency_norm(50, 50);
        assert_eq!(fifty, 1.0);
        assert!(ten > 0.5, "log-scaled 10/50 should be well above the linear 0.2, got {ten}");
    }

    #[test]
    fn recency_decays_to_one_at_zero_days_and_toward_zero_far_out() {
        let now = at("2026-06-01T00:00:00Z");
        assert!((recency(Some(now), now, 14.0) - 1.0).abs() < 1e-9);
        let fourteen_days_ago = at("2026-05-18T00:00:00Z");
        // exp(-1) ≈ 0.3679 at exactly τ days.
        assert!((recency(Some(fourteen_days_ago), now, 14.0) - std::f64::consts::E.recip()).abs() < 1e-3);
        let far = at("2025-01-01T00:00:00Z");
        assert!(recency(Some(far), now, 14.0) < 0.01);
    }

    #[test]
    fn tau_extremes_never_panic_or_nan() {
        let now = at("2026-06-01T00:00:00Z");
        let last = at("2026-05-30T00:00:00Z");
        // Non-positive tau disables recency rather than dividing by zero.
        assert_eq!(recency(Some(last), now, 0.0), 0.0);
        assert_eq!(recency(Some(last), now, -5.0), 0.0);
        // Huge tau → recency ≈ 1 (barely any decay).
        assert!(recency(Some(last), now, 1e9).is_finite());
        assert!(recency(Some(last), now, 1e9) > 0.99);
    }

    #[test]
    fn corroboration_is_binary_at_two_sources() {
        assert_eq!(corroboration(0), 0.0);
        assert_eq!(corroboration(1), 0.0);
        assert_eq!(corroboration(2), 1.0);
        assert_eq!(corroboration(9), 1.0);
    }

    #[test]
    fn weights_renormalize_proportionally() {
        let weights = StrengthWeights { frequency: 9.0, recency: 7.0, corroboration: 4.0 };
        let norm = weights.renormalized();
        assert!((norm.frequency + norm.recency + norm.corroboration - 1.0).abs() < 1e-12);
        // Ratios preserved.
        assert!((norm.frequency / norm.recency - 9.0 / 7.0).abs() < 1e-12);
    }

    #[test]
    fn weights_fall_back_to_default_on_invalid() {
        assert_eq!(
            StrengthWeights { frequency: 0.0, recency: 0.0, corroboration: 0.0 }.renormalized(),
            StrengthWeights::DEFAULT
        );
        assert_eq!(
            StrengthWeights { frequency: -1.0, recency: 1.0, corroboration: 1.0 }.renormalized(),
            StrengthWeights::DEFAULT
        );
        assert_eq!(
            StrengthWeights { frequency: f64::NAN, recency: 1.0, corroboration: 1.0 }.renormalized(),
            StrengthWeights::DEFAULT
        );
    }

    #[test]
    fn full_strength_clamps_to_one() {
        let now = at("2026-06-01T00:00:00Z");
        let facts = StrengthFacts {
            recall_count_30d: 50,
            last_recalled_at: Some(now),
            max_recall_30d_active: 50,
            distinct_sources: 3,
        };
        // freq=1, recency=1, corr=1 → weighted sum == 1 after renorm.
        assert!((strength(facts, StrengthWeights::DEFAULT, 14.0, now) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn strength_points_floor_and_ceiling() {
        assert_eq!(strength_points(0.0, 12), 0);
        assert_eq!(strength_points(1.0, 12), 11);
        assert_eq!(strength_points(0.99, 12), 11);
        assert_eq!(strength_points(0.5, 12), 6);
        assert_eq!(strength_points(1.0, 1), 0);
        assert_eq!(strength_points(1.0, 0), 0);
        // Out-of-range clamps.
        assert_eq!(strength_points(1.5, 12), 11);
        assert_eq!(strength_points(-0.2, 12), 0);
    }
}
