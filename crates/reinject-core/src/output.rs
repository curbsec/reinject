//! Claude Code hook JSON output builder.
//!
//! Port of `reinject_output` from `hooks/lib/should-reinject.sh`.

use serde_json::{json, Value};

/// Build the JSON response for a Claude Code hook event.
///
/// Produces `hookSpecificOutput` with `additionalContext` and, when a
/// `system_message` is provided, a top-level `systemMessage` field.
///
/// The default system message when `None` is passed is:
/// `"Context refreshed — key rules were fading"`.
#[must_use]
pub fn hook_output(hook_event: &str, context: &str, system_message: Option<&str>) -> String {
    let msg = system_message.unwrap_or("Context refreshed \u{2014} key rules were fading");

    let value: Value = if msg.is_empty() {
        json!({
            "hookSpecificOutput": {
                "hookEventName": hook_event,
                "additionalContext": context
            }
        })
    } else {
        json!({
            "hookSpecificOutput": {
                "hookEventName": hook_event,
                "additionalContext": context
            },
            "systemMessage": msg
        })
    };

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_system_message_included() {
        let out = hook_output("PreToolUse", "my context", None);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "my context");
        assert_eq!(
            v["systemMessage"],
            "Context refreshed \u{2014} key rules were fading"
        );
    }

    #[test]
    fn custom_system_message() {
        let out = hook_output("PostToolUse", "ctx", Some("custom msg"));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["systemMessage"], "custom msg");
    }

    #[test]
    fn empty_system_message_omits_field() {
        let out = hook_output("PreToolUse", "ctx", Some(""));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("systemMessage").is_none());
    }

    #[test]
    fn output_is_valid_json() {
        let out = hook_output("UserPromptSubmit", "context text", None);
        assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok());
    }
}
