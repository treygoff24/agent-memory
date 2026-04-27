//! Deterministic fault injection hooks used by tests.

use std::collections::HashSet;

/// Named fault injection points.
#[derive(Clone, Debug, Default)]
pub struct FaultSet {
    points: HashSet<String>,
}

impl FaultSet {
    /// Enable a fault point.
    pub fn enable(&mut self, point: impl Into<String>) {
        self.points.insert(point.into());
    }
    /// Check whether a point is enabled.
    pub fn enabled(&self, point: &str) -> bool {
        self.points.contains(point)
    }
}
