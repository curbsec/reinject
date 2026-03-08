# reinject

Context rot prevention for Claude Code hooks.

## The Problem

Claude Code has three layers for giving the model instructions:

1. **CLAUDE.md** — loaded at session start, pinned at the top of context. Benefits from primacy (the model pays attention to what comes first). But as the context window fills with conversation, the signal-to-noise ratio drops. 200 lines of instructions competing with 100K tokens of tool results and code — attention dilutes even at position zero.

2. **Hooks (without reinject)** — fire on specific events (PreToolUse, PostToolUse, etc.) and inject contextual instructions. Unlike CLAUDE.md, they're relevant to what's happening right now. But you have two bad options:
   - **Inject every time** the hook fires → floods the context with redundant copies. Run 50 commands and you've shoved 50 copies of the same rules in. Burns tokens and accelerates the exact problem you're trying to solve — pushing real conversation into the dead zone faster.
   - **Inject once** and hope for the best → the injection drifts from the recency zone into the middle of context (the "dead zone," 15-85% of the window). The model's attention is weakest there ([Liu et al., 2023](https://arxiv.org/abs/2307.03172)). Your hook becomes invisible.

3. **Hooks + reinject** — injects only when the math says the model is likely forgetting. Tracks context growth and injection position, re-injects when either threshold is exceeded. No flooding, no drifting.

Reinject solves the positional problem (lost-in-the-middle). It doesn't solve signal dilution — if your context window is 200K tokens deep, even primacy-zone content loses influence. That's a density problem, not a positioning problem.

## How It Works

**Monitor** (`context-monitor.sh`) — fires on every user message and tool result. Parses the JSONL transcript delta since last check, counts bytes of non-thinking and thinking text separately, writes cumulative totals to a status file. That's it — just counting.

**Consumer library** (`should-reinject.sh`) — sourced by your hooks. Before a tool runs, your hook calls `should_reinject("my-hook-name")`. The library reads the monitor's byte counts and compares against the counts from the last time *this specific hook* injected. Two triggers:

- **Growth threshold**: enough new text has accumulated since last injection. Configurable per hook — 52KB for critical rules, 105KB for medium, 175KB for nice-to-have.
- **Dead zone position**: the last injection landed between 15-85% of total context where attention is weakest.

If either fires → re-inject. If neither → skip.

**Compaction reset** — when Claude Code compresses the conversation, all byte counts become meaningless. State is wiped; next relevant tool call triggers a fresh injection.

**Sub-agent skip** — both monitor and consumer detect sub-agents (via `agent_id` in hook input) and exit immediately. Sub-agents are short-lived — tracking their context is pointless.

**No race conditions** — the monitor completes before the next PreToolUse consumer fires, so the status file is always current.

## In Practice

Your supabase-context hook injects DB connection rules. First tool call → injects. Next 30K tokens of conversation → `should_reinject` returns false, no injection. Then the growth threshold fires → re-injects the rules. They stay fresh without spamming every tool call.

## Installation

### As a Claude Code plugin

```bash
claude plugins add /path/to/reinject
```

Auto-registers the monitor (UserPromptSubmit + PostToolUse) and compaction reset (SessionStart compact). You write your own consumer hooks.

### Manual installation

Copy `hooks/` and `parsers/` somewhere stable, then add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "/path/to/hooks/context-monitor.sh"
      }]
    }],
    "PostToolUse": [{
      "hooks": [{
        "type": "command",
        "command": "/path/to/hooks/context-monitor.sh"
      }]
    }],
    "SessionStart": [{
      "matcher": "compact",
      "hooks": [{
        "type": "command",
        "command": "/path/to/hooks/compact-reset.sh"
      }]
    }]
  }
}
```

## Writing a Consumer Hook

```bash
#!/bin/bash
INPUT=$(cat)  # MUST capture stdin first

# Your relevance check (matcher logic)
COMMAND=$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')
if ! printf '%s' "$COMMAND" | grep -qi 'my-tool'; then
  exit 0
fi

# Set your criticality tier (optional, default Medium = 105KB)
REINJECT_GROWTH_BYTES=52000  # High tier

# Source the consumer library
source "/path/to/hooks/lib/should-reinject.sh"

# Check if re-injection is needed
if ! should_reinject "my-hook-name"; then
  exit 0
fi

# Inject your context
reinject_output "PreToolUse" "Your context here"

# Record that you injected
reinject_record "my-hook-name"
exit 0
```

## Configuration

All via environment variables (set before sourcing the library):

| Variable | Default | Description |
|----------|---------|-------------|
| `REINJECT_GROWTH_BYTES` | `105000` | Growth threshold (non-thinking text bytes) |
| `REINJECT_RECENCY_THRESHOLD` | `85` | Upper dead zone boundary (%) |
| `REINJECT_PRIMACY_THRESHOLD` | `15` | Lower dead zone boundary (%) |
| `REINJECT_MIN_CONTEXT_BYTES` | `21000` | Min context for position check (~6K tokens) |
| `REINJECT_PARSER` | `jq` | Monitor parser: `jq` or path to Rust binary |

### Criticality Tiers

| Tier | Bytes | ~Tokens | Use Case |
|------|-------|---------|----------|
| High | 52,000 | 15K | Credentials, security rules |
| Medium | 105,000 | 30K | Workflow guides, conventions |
| Low | 175,000 | 50K | Nice-to-have reminders |

No tokenizer needed. Text bytes / 3.5 approximates tokens (~15% accuracy, sub-millisecond).

## Architecture

```
UserPromptSubmit / PostToolUse      PreToolUse (Bash)
       |                                   |
  context-monitor.sh              your-hook.sh (consumer)
       |                                   |
  Parse JSONL delta              source should-reinject.sh
       |                                   |
  Write status file ──────────> Read status file
  (cumulative bytes)            Compare vs own injection history
       |                                   |
  Done (before next             Inject if threshold exceeded
   PreToolUse fires)            Record injection
```

## Requirements

- `jq` 1.7+
- `bash` 4+
- Claude Code v2.1.9+ (PreToolUse `additionalContext` support)

## Docs

- [PLAN.md](docs/PLAN.md) — full architecture and design decisions
- [ASSUMPTIONS.md](docs/ASSUMPTIONS.md) — what we're building on, with confidence levels
- [RESEARCH.md](docs/RESEARCH.md) — academic research backing the approach
