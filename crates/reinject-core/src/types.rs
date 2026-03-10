//! Shared domain types for reinject.

/// Criticality tier for re-injection frequency.
///
/// Controls how aggressively a consumer hook re-injects its context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Credentials, security rules — ~15K tokens (~52 000 bytes).
    High,
    /// Workflow guides, coding conventions — ~30K tokens (~105 000 bytes).
    Medium,
    /// Nice-to-have reminders — ~50K tokens (~175 000 bytes).
    Low,
}

impl Tier {
    /// Growth-byte threshold for this tier.
    pub fn growth_bytes(self) -> u64 {
        match self {
            Tier::High => 52_000,
            Tier::Medium => 105_000,
            Tier::Low => 175_000,
        }
    }
}

/// Configuration for the throttle decision.
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Minimum non-thinking-text growth (bytes) since last injection to trigger re-inject.
    pub growth_bytes: u64,
    /// Upper boundary of the dead zone (%). Injections at positions below this and
    /// above `primacy_threshold` are considered "lost" and will be re-injected.
    pub recency_threshold: u64,
    /// Lower boundary of the dead zone (%).
    pub primacy_threshold: u64,
    /// Minimum total context bytes before the dead-zone position check activates.
    pub min_context_bytes: u64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            growth_bytes: 105_000,
            recency_threshold: 85,
            primacy_threshold: 15,
            min_context_bytes: 21_000,
        }
    }
}

/// Why a re-injection was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectReason {
    /// No prior injection has ever been recorded for this hook.
    FirstRun,
    /// Monitor byte count went backwards — compaction occurred.
    CompactionDetected,
    /// Non-thinking text growth since last injection exceeded the threshold.
    GrowthExceeded {
        /// Bytes grown since last injection.
        delta: u64,
        /// Configured threshold.
        threshold: u64,
    },
    /// The saved injection position falls in the context dead zone.
    DeadZone {
        /// Position as a percentage of total context bytes (0–100).
        position_pct: u64,
    },
}

/// Decision returned by [`crate::throttle::should_reinject`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThrottleDecision {
    /// Inject context. Contains the reason for auditing/logging.
    Inject(InjectReason),
    /// Do not inject.
    Skip,
}
