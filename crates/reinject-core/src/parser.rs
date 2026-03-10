//! JSONL transcript parser — counts non-thinking and thinking text bytes.
//!
//! Ported from `parsers/rust/src/main.rs` and promoted to a library function
//! so both the CLI binary and `parsers/rust` (backwards-compat binary) can use it.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

/// A single line from the Claude Code JSONL transcript.
#[derive(Deserialize)]
pub struct TranscriptLine {
    pub(crate) message: Option<Message>,
}

#[derive(Deserialize)]
pub(crate) struct Message {
    pub(crate) content: Option<Content>,
}

/// Content is either a plain string or an array of typed content blocks.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Content {
    Plain(String),
    Blocks(Vec<ContentBlock>),
}

/// A typed content block within a message.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ContentBlock {
    #[serde(rename = "thinking")]
    Thinking { thinking: Option<String> },
    #[serde(rename = "text")]
    Text { text: Option<String> },
    #[serde(rename = "tool_use")]
    ToolUse { input: Option<serde_json::Value> },
    #[serde(rename = "tool_result")]
    ToolResult { content: Option<ToolResultContent> },
    /// Catch-all for block types we don't care about (e.g. "image").
    #[serde(other)]
    Unknown,
}

/// `tool_result` content: either a string or an array of text objects.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolResultContent {
    Plain(String),
    Parts(Vec<ToolResultPart>),
}

#[derive(Deserialize)]
pub(crate) struct ToolResultPart {
    pub(crate) text: Option<String>,
}

/// Parse the JSONL transcript delta starting at `offset` bytes into the file.
///
/// Returns `(non_thinking_bytes, thinking_bytes)` accumulated over all new lines.
/// The first incomplete line at `offset` is always skipped (the monitor writes a
/// full line per call, but the seek may land mid-line if the offset was recorded
/// before the newline was flushed — skipping the first line is the safe choice,
/// matching the original shell implementation).
pub fn parse_transcript_delta(path: &Path, offset: u64) -> Result<(u64, u64)> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file_len <= offset {
        return Ok((0, 0));
    }

    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seek failed in {}", path.display()))?;

    let reader = BufReader::new(file);
    let mut total_nt: u64 = 0;
    let mut total_th: u64 = 0;
    let mut first_line = true;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if first_line {
            first_line = false;
            continue;
        }

        if line.is_empty() {
            continue;
        }

        let parsed: TranscriptLine = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let (nt, th) = count_text_bytes(&parsed);
        total_nt += nt;
        total_th += th;
    }

    Ok((total_nt, total_th))
}

/// Returns `(non_thinking_bytes, thinking_bytes)` for a single transcript line.
pub(crate) fn count_text_bytes(line: &TranscriptLine) -> (u64, u64) {
    let content = match line.message.as_ref().and_then(|m| m.content.as_ref()) {
        Some(c) => c,
        None => return (0, 0),
    };

    match content {
        Content::Plain(s) => (s.len() as u64, 0),
        Content::Blocks(blocks) => {
            let mut nt: u64 = 0;
            let mut th: u64 = 0;
            for block in blocks {
                match block {
                    ContentBlock::Thinking { thinking: Some(s) } => {
                        th += s.len() as u64;
                    }
                    ContentBlock::Text { text: Some(s) } => {
                        nt += s.len() as u64;
                    }
                    ContentBlock::ToolUse { input: Some(v) } => {
                        nt += v.to_string().len() as u64;
                    }
                    ContentBlock::ToolResult { content: Some(c) } => {
                        nt += count_tool_result_bytes(c);
                    }
                    ContentBlock::Thinking { thinking: None }
                    | ContentBlock::Text { text: None }
                    | ContentBlock::ToolUse { input: None }
                    | ContentBlock::ToolResult { content: None }
                    | ContentBlock::Unknown => {}
                }
            }
            (nt, th)
        }
    }
}

pub(crate) fn count_tool_result_bytes(content: &ToolResultContent) -> u64 {
    match content {
        ToolResultContent::Plain(s) => s.len() as u64,
        ToolResultContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| p.text.as_ref())
            .map(|s| s.len() as u64)
            .sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_count(json: &str) -> (u64, u64) {
        let line: TranscriptLine = serde_json::from_str(json).unwrap();
        count_text_bytes(&line)
    }

    #[test]
    fn plain_string_content() {
        let json = r#"{"message":{"content":"hello world"}}"#;
        assert_eq!(parse_and_count(json), (11, 0));
    }

    #[test]
    fn text_block() {
        let json = r#"{"message":{"content":[{"type":"text","text":"abc"}]}}"#;
        assert_eq!(parse_and_count(json), (3, 0));
    }

    #[test]
    fn thinking_block() {
        let json = r#"{"message":{"content":[{"type":"thinking","thinking":"deep thoughts"}]}}"#;
        assert_eq!(parse_and_count(json), (0, 13));
    }

    #[test]
    fn mixed_blocks() {
        let json = r#"{"message":{"content":[
            {"type":"text","text":"visible"},
            {"type":"thinking","thinking":"hidden"},
            {"type":"text","text":"more"}
        ]}}"#;
        assert_eq!(parse_and_count(json), (11, 6));
    }

    #[test]
    fn tool_use_block() {
        let json = r#"{"message":{"content":[{"type":"tool_use","input":{"key":"val"}}]}}"#;
        let (nt, th) = parse_and_count(json);
        assert_eq!(nt, r#"{"key":"val"}"#.len() as u64);
        assert_eq!(th, 0);
    }

    #[test]
    fn tool_result_plain_string() {
        let json = r#"{"message":{"content":[{"type":"tool_result","content":"result text"}]}}"#;
        assert_eq!(parse_and_count(json), (11, 0));
    }

    #[test]
    fn tool_result_parts_array() {
        let json = r#"{"message":{"content":[{"type":"tool_result","content":[{"text":"part1"},{"text":"part2"}]}]}}"#;
        assert_eq!(parse_and_count(json), (10, 0));
    }

    #[test]
    fn no_message_field() {
        let json = r#"{"type":"system"}"#;
        assert_eq!(parse_and_count(json), (0, 0));
    }

    #[test]
    fn no_content_field() {
        let json = r#"{"message":{"role":"assistant"}}"#;
        assert_eq!(parse_and_count(json), (0, 0));
    }

    #[test]
    fn unknown_block_type_ignored() {
        let json = r#"{"message":{"content":[{"type":"image","source":"whatever"},{"type":"text","text":"hi"}]}}"#;
        assert_eq!(parse_and_count(json), (2, 0));
    }

    #[test]
    fn empty_blocks_array() {
        let json = r#"{"message":{"content":[]}}"#;
        assert_eq!(parse_and_count(json), (0, 0));
    }

    #[test]
    fn thinking_block_null_text() {
        let json = r#"{"message":{"content":[{"type":"thinking","thinking":null}]}}"#;
        assert_eq!(parse_and_count(json), (0, 0));
    }

    #[test]
    fn tool_result_empty_parts() {
        let json =
            r#"{"message":{"content":[{"type":"tool_result","content":[{"text":null},{}]}]}}"#;
        assert_eq!(parse_and_count(json), (0, 0));
    }
}
