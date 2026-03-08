# reinject

Context rot prevention for Claude Code hooks.

## The Problem

Claude Code gives you several ways to instruct the model, each with a different failure mode as conversations grow:

1. **Static instructions** (CLAUDE.md, `.claude/rules/*.md`) are loaded at session start and pinned at the top of context. They benefit from primacy (the model pays more attention to what comes first) and survive compaction since Claude Code re-loads them after compressing the conversation. The downside is signal dilution: as the context window fills up, 200 lines of instructions have to compete with 100K tokens of tool results and code. Attention dilutes even at position zero.

2. **Hooks (without reinject)** fire on specific events (PreToolUse, PostToolUse, etc.) and inject contextual instructions. Unlike static files, they're relevant to what's happening right now. But without reinject you have two bad options:
   - **Inject every time** the hook fires, which floods the context with redundant copies. Run 50 commands and you've shoved 50 copies of the same rules in, burning tokens and accelerating the exact problem you're trying to solve by pushing real conversation into the dead zone faster.
   - **Inject once** and hope for the best. The injection drifts from the recency zone into the middle of context (the "dead zone," 15-85% of the window), where the model's attention is weakest ([Liu et al., 2023](https://arxiv.org/abs/2307.03172)). Compaction can also obliterate it entirely since, unlike static instructions, hook injections are conversation content and get summarized or dropped.

3. **Hooks + reinject** injects only when the math says the model is likely forgetting. It tracks context growth and injection position, re-injecting when either threshold is exceeded. No flooding, no drifting.

Reinject solves the positional problem (lost-in-the-middle) and handles compaction recovery. It doesn't solve signal dilution. If your context window is 200K tokens deep, even primacy-zone content loses influence, and that's a density problem, not a positioning problem.

## How It Works

The **monitor** (`context-monitor.sh`) fires on every user message and tool result. It parses the JSONL transcript delta since the last check, counts bytes of non-thinking and thinking text separately, and writes cumulative totals to a status file. Just counting.

The **consumer library** (`should-reinject.sh`) is sourced by your hooks. Before a tool runs, your hook calls `should_reinject("my-hook-name")`. The library reads the monitor's byte counts and compares them against the counts from the last time *this specific hook* injected. Two things can trigger a re-injection:

- **Growth threshold**: enough new text has accumulated since last injection. Configurable per hook: 52KB for critical rules, 105KB for medium, 175KB for nice-to-have.
- **Dead zone position**: the last injection landed between 15-85% of total context, where attention is weakest.

If either condition is met, reinject. Otherwise skip.

**Compaction reset**: when Claude Code compresses the conversation, all byte counts become meaningless. State is wiped and the next relevant tool call triggers a fresh injection.

**Sub-agent skip**: both monitor and consumer detect sub-agents (via `agent_id` in hook input) and exit immediately. Sub-agents are short-lived and tracking their context growth is pointless.

There are **no race conditions** because the monitor completes before the next PreToolUse consumer fires, so the status file is always current.

## In Practice

Say your supabase-context hook injects DB connection rules. On the first tool call it injects. Over the next 30K tokens of conversation, `should_reinject` returns false and nothing happens. Then the growth threshold fires and the rules get re-injected. They stay fresh without spamming every tool call.

## Installation

### As a Claude Code plugin

```bash
claude plugins add /path/to/reinject
```

This auto-registers the monitor (UserPromptSubmit + PostToolUse) and compaction reset (SessionStart compact). You write your own consumer hooks.

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

All via environment variables, set before sourcing the library:

| Variable | Default | Description |
|----------|---------|-------------|
| `REINJECT_GROWTH_BYTES` | `105000` | Growth threshold in non-thinking text bytes |
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

We don't tokenize. Running a tokenizer on every hook invocation would add latency to every tool call, and we don't need exact token counts anyway since we're comparing against thresholds, not computing precise positions. Instead we divide text bytes by 3.5 to approximate tokens. The 3.5 ratio is the conservative (lower) end of the empirically observed 3.5-4.5 bytes-per-token range for English text with code. Using the lower bound means we assume faster attenuation: reinject triggers sooner rather than later, which is the safe direction to err in. The approximation is accurate to about 15% and takes sub-millisecond time.

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

- [PLAN.md](docs/PLAN.md) - full architecture and design decisions
- [ASSUMPTIONS.md](docs/ASSUMPTIONS.md) - what we're building on, with confidence levels
- [RESEARCH.md](docs/RESEARCH.md) - academic research backing the approach
