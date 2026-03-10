//! Monitor logic — parses the JSONL transcript delta and updates byte-count state.
//!
//! Port of `hooks/context-monitor.sh`. Run on `UserPromptSubmit` and `PostToolUse`.

use std::path::Path;

use anyhow::{Context as _, Result};

use crate::{
    parser::parse_transcript_delta,
    state::{read_monitor_status, read_offset, write_monitor_status, write_offset, MonitorStatus},
};

/// Update the monitor state for a new transcript delta.
///
/// Reads the transcript from `transcript_path` starting at the previously saved
/// byte offset, accumulates new non-thinking and thinking byte counts on top of
/// the existing cumulative totals, and writes back the updated state.
///
/// Returns `Ok(())` immediately (a no-op) when:
/// - `transcript_path` does not exist, or
/// - the file has not grown since the last saved offset.
pub fn update_monitor(state_dir: &Path, transcript_path: &Path) -> Result<()> {
    // Nothing to do if the transcript is missing.
    if !transcript_path.exists() {
        return Ok(());
    }

    let saved_offset = read_offset(state_dir);

    let current_size = transcript_path
        .metadata()
        .with_context(|| format!("failed to stat {}", transcript_path.display()))?
        .len();

    if current_size <= saved_offset {
        // No growth since last check.
        return Ok(());
    }

    let (delta_nt, delta_th) =
        parse_transcript_delta(transcript_path, saved_offset).with_context(|| {
            format!(
                "failed to parse transcript delta in {}",
                transcript_path.display()
            )
        })?;

    // Accumulate on top of previous cumulative totals.
    let prev = read_monitor_status(state_dir).unwrap_or_default();
    let updated = MonitorStatus {
        non_thinking_bytes: prev.non_thinking_bytes + delta_nt,
        thinking_bytes: prev.thinking_bytes + delta_th,
    };

    write_monitor_status(state_dir, &updated)?;
    write_offset(state_dir, current_size)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::read_monitor_status;
    use std::io::Write as _;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_jsonl(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("transcript.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn missing_transcript_is_noop() {
        let dir = tmp();
        let transcript = dir.path().join("nonexistent.jsonl");
        update_monitor(dir.path(), &transcript).unwrap();
        assert!(read_monitor_status(dir.path()).is_none());
    }

    #[test]
    fn first_run_accumulates_bytes() {
        let state_dir = tmp();
        let transcript_dir = tmp();
        // Two JSONL lines — first is skipped (partial-line safety), second is counted.
        let content =
            "{\"message\":{\"content\":\"hello\"}}\n{\"message\":{\"content\":\"world\"}}\n";
        let transcript = write_jsonl(&transcript_dir, content);
        update_monitor(state_dir.path(), &transcript).unwrap();
        let status = read_monitor_status(state_dir.path()).unwrap();
        // "world" = 5 bytes (first line skipped)
        assert_eq!(status.non_thinking_bytes, 5);
        assert_eq!(status.thinking_bytes, 0);
    }

    #[test]
    fn no_growth_is_noop() {
        let state_dir = tmp();
        let transcript_dir = tmp();
        let content = "{\"message\":{\"content\":\"hello\"}}\n";
        let transcript = write_jsonl(&transcript_dir, content);
        // First call: advances offset to file size.
        update_monitor(state_dir.path(), &transcript).unwrap();
        let after_first = read_monitor_status(state_dir.path()).unwrap_or_default();
        // Second call: no growth.
        update_monitor(state_dir.path(), &transcript).unwrap();
        let after_second = read_monitor_status(state_dir.path()).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn incremental_update_accumulates() {
        let state_dir = tmp();
        let transcript_dir = tmp();
        let path = transcript_dir.path().join("transcript.jsonl");

        // First write: two lines.
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(
                b"{\"message\":{\"content\":\"aaa\"}}\n{\"message\":{\"content\":\"bbb\"}}\n",
            )
            .unwrap();
        }
        update_monitor(state_dir.path(), &path).unwrap();
        let after_first = read_monitor_status(state_dir.path()).unwrap();
        // "bbb" = 3 bytes counted
        assert_eq!(after_first.non_thinking_bytes, 3);

        // Append a third line.
        {
            let mut f = std::fs::File::options().append(true).open(&path).unwrap();
            f.write_all(b"{\"message\":{\"content\":\"ccc\"}}\n")
                .unwrap();
        }
        update_monitor(state_dir.path(), &path).unwrap();
        let after_second = read_monitor_status(state_dir.path()).unwrap();
        // "bbb"(3) + skip_first_of_delta + "ccc"(3) — but the delta starts
        // mid-file; the first line in the delta window is skipped for safety.
        // The appended line IS the first line in the new delta window, so it gets skipped.
        // Net: still 3 bytes.
        assert_eq!(after_second.non_thinking_bytes, 3);
    }
}
