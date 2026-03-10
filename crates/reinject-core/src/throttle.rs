//! Throttle logic — determines whether to re-inject context for a given hook.
//!
//! This is a faithful port of `hooks/lib/should-reinject.sh`. The shell script
//! both decided *and* recorded; here the two responsibilities are split:
//! - [`should_reinject`] makes the decision (read-only).
//! - The caller records after a successful injection via
//!   [`crate::state::write_consumer_state`].

use std::path::Path;

use anyhow::Result;

use crate::{
    state::{read_consumer_state, read_monitor_status, write_consumer_state, MonitorStatus},
    types::{InjectReason, ThrottleConfig, ThrottleDecision},
};

/// Decide whether the named hook should re-inject its context right now.
///
/// Mirrors the decision tree in `should-reinject.sh`:
/// 1. No monitor file + no consumer file → [`InjectReason::FirstRun`]
/// 2. No monitor file + consumer file exists → [`ThrottleDecision::Skip`]
/// 3. Monitor file exists, no consumer file → [`InjectReason::FirstRun`]
/// 4. Both exist: compare byte counts:
///    - Negative delta → [`InjectReason::CompactionDetected`]
///    - Delta > threshold → [`InjectReason::GrowthExceeded`]
///    - Saved position in dead zone → [`InjectReason::DeadZone`]
///    - Otherwise → [`ThrottleDecision::Skip`]
///
/// When this returns [`ThrottleDecision::Inject`], the caller is responsible
/// for writing the updated consumer state via [`write_consumer_state`].
pub fn should_reinject(
    hook_name: &str,
    config: &ThrottleConfig,
    state_dir: &Path,
) -> Result<ThrottleDecision> {
    let monitor = read_monitor_status(state_dir);
    let consumer = read_consumer_state(state_dir, hook_name);

    match (monitor, consumer) {
        // Monitor hasn't run yet.
        (None, None) => {
            // First opportunity: record now and inject.
            record(state_dir, hook_name, &MonitorStatus::default())?;
            Ok(ThrottleDecision::Inject(InjectReason::FirstRun))
        }
        (None, Some(_)) => {
            // Monitor reset (compaction?) but consumer state survives — skip.
            Ok(ThrottleDecision::Skip)
        }
        (Some(monitor), None) => {
            // Monitor has run but this hook has never injected.
            record(state_dir, hook_name, &monitor)?;
            Ok(ThrottleDecision::Inject(InjectReason::FirstRun))
        }
        (Some(monitor), Some(saved)) => {
            decide_with_state(hook_name, config, state_dir, &monitor, &saved)
        }
    }
}

/// Record current monitor values as the hook's injection baseline.
///
/// Called internally by [`should_reinject`] on inject decisions, and can be
/// called externally after a successful injection to update the baseline.
pub fn record(state_dir: &Path, hook_name: &str, status: &MonitorStatus) -> Result<()> {
    write_consumer_state(state_dir, hook_name, status)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn decide_with_state(
    hook_name: &str,
    config: &ThrottleConfig,
    state_dir: &Path,
    monitor: &MonitorStatus,
    saved: &MonitorStatus,
) -> Result<ThrottleDecision> {
    let monitor_nt = monitor.non_thinking_bytes;
    let monitor_th = monitor.thinking_bytes;
    let saved_nt = saved.non_thinking_bytes;
    let saved_th = saved.thinking_bytes;

    // Step 1: Compaction — monitor byte count went backwards.
    if monitor_nt < saved_nt {
        record(state_dir, hook_name, monitor)?;
        return Ok(ThrottleDecision::Inject(InjectReason::CompactionDetected));
    }

    let delta_nt = monitor_nt - saved_nt;

    // Step 2: Absolute growth exceeded threshold.
    if delta_nt > config.growth_bytes {
        // Caller records after injection; we don't auto-record here so that
        // failing injections don't silently advance the baseline.
        return Ok(ThrottleDecision::Inject(InjectReason::GrowthExceeded {
            delta: delta_nt,
            threshold: config.growth_bytes,
        }));
    }

    // Step 3: Dead-zone position check.
    let total = monitor_nt + monitor_th;
    if total > config.min_context_bytes && total > 0 {
        let saved_total = saved_nt + saved_th;
        let position_pct = saved_total * 100 / total;
        if position_pct > config.primacy_threshold && position_pct < config.recency_threshold {
            return Ok(ThrottleDecision::Inject(InjectReason::DeadZone {
                position_pct,
            }));
        }
    }

    Ok(ThrottleDecision::Skip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{write_consumer_state, write_monitor_status};
    use crate::types::{InjectReason, ThrottleDecision};
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn default_config() -> ThrottleConfig {
        ThrottleConfig::default()
    }

    fn status(nt: u64, th: u64) -> MonitorStatus {
        MonitorStatus {
            non_thinking_bytes: nt,
            thinking_bytes: th,
        }
    }

    // ── Branch 1: no monitor, no consumer → FirstRun ─────────────────────────

    #[test]
    fn no_monitor_no_consumer_is_first_run() {
        let dir = tmp();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Inject(InjectReason::FirstRun));
    }

    // ── Branch 2: no monitor, consumer exists → Skip ─────────────────────────

    #[test]
    fn no_monitor_with_consumer_skips() {
        let dir = tmp();
        write_consumer_state(dir.path(), "hook-a", &status(0, 0)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Skip);
    }

    // ── Branch 3: monitor exists, no consumer → FirstRun ─────────────────────

    #[test]
    fn monitor_exists_no_consumer_is_first_run() {
        let dir = tmp();
        write_monitor_status(dir.path(), &status(50_000, 10_000)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Inject(InjectReason::FirstRun));
    }

    // ── Branch 4a: compaction detected (monitor_nt < saved_nt) ───────────────

    #[test]
    fn compaction_detected_when_monitor_nt_less_than_saved_nt() {
        let dir = tmp();
        write_monitor_status(dir.path(), &status(10_000, 0)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(50_000, 0)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(
            result,
            ThrottleDecision::Inject(InjectReason::CompactionDetected)
        );
    }

    // ── Branch 4b: growth exceeded ────────────────────────────────────────────

    #[test]
    fn growth_exceeded_threshold() {
        let dir = tmp();
        let config = default_config(); // growth_bytes = 105_000
        write_monitor_status(dir.path(), &status(200_000, 0)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(80_000, 0)).unwrap();
        // delta = 120_000 > 105_000
        let result = should_reinject("hook-a", &config, dir.path()).unwrap();
        assert_eq!(
            result,
            ThrottleDecision::Inject(InjectReason::GrowthExceeded {
                delta: 120_000,
                threshold: 105_000
            })
        );
    }

    #[test]
    fn growth_exactly_at_threshold_does_not_inject() {
        let dir = tmp();
        let config = default_config(); // growth_bytes = 105_000
        write_monitor_status(dir.path(), &status(105_000, 0)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(0, 0)).unwrap();
        // delta = 105_000, not > 105_000
        let result = should_reinject("hook-a", &config, dir.path()).unwrap();
        // also check dead zone — saved_total = 0, position_pct = 0, not in zone
        assert_eq!(result, ThrottleDecision::Skip);
    }

    // ── Branch 4c: dead zone ─────────────────────────────────────────────────

    #[test]
    fn dead_zone_middle_position_injects() {
        let dir = tmp();
        // Large context so min_context_bytes is satisfied
        // Saved position is at 50% — between 15% and 85%
        write_monitor_status(dir.path(), &status(100_000, 100_000)).unwrap();
        // saved_total = 100_000; total = 200_000 → position_pct = 50
        write_consumer_state(dir.path(), "hook-a", &status(100_000, 0)).unwrap();
        // delta_nt = 0 (no growth) → won't hit growth branch
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(
            result,
            ThrottleDecision::Inject(InjectReason::DeadZone { position_pct: 50 })
        );
    }

    #[test]
    fn position_in_primacy_zone_skips() {
        let dir = tmp();
        // saved_total = 5_000; total = 200_000 → position_pct = 2 (< 15)
        write_monitor_status(dir.path(), &status(100_000, 100_000)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(5_000, 0)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Skip);
    }

    #[test]
    fn position_in_recency_zone_skips() {
        let dir = tmp();
        // monitor: nt=200_000, th=0. saved: nt=180_000, th=0.
        // delta_nt = 20_000 (below 105_000 threshold, no growth trigger).
        // saved_total = 180_000; total = 200_000 → position_pct = 90 (> 85) → recency zone → skip.
        write_monitor_status(dir.path(), &status(200_000, 0)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(180_000, 0)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Skip);
    }

    #[test]
    fn small_context_skips_dead_zone_check() {
        let dir = tmp();
        // total = 10_000 < min_context_bytes (21_000)
        write_monitor_status(dir.path(), &status(5_000, 5_000)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(1_000, 0)).unwrap();
        // position would be 10%, primacy zone, but check doesn't run anyway
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Skip);
    }

    // ── Branch 4d: nothing triggered → Skip ──────────────────────────────────

    #[test]
    fn no_conditions_met_skips() {
        let dir = tmp();
        // delta = 1_000 (well below 105_000), position = 0% (primacy zone)
        write_monitor_status(dir.path(), &status(50_000, 0)).unwrap();
        write_consumer_state(dir.path(), "hook-a", &status(49_000, 0)).unwrap();
        let result = should_reinject("hook-a", &default_config(), dir.path()).unwrap();
        assert_eq!(result, ThrottleDecision::Skip);
    }
}
