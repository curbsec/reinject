#!/bin/bash
# SessionStart compact hook — resets all state after compaction.
#
# When CC compacts context, old injections are summarized away and lose
# specific details. This clears monitor and consumer state so the next
# user prompt re-parses and the next relevant tool use re-injects.

INPUT=$(cat)
_session_id=$(printf '%s' "$INPUT" | jq -r '.session_id // empty' 2>/dev/null)
STATE_DIR="${REINJECT_STATE_DIR:-/tmp/claude-reinject-${_session_id:-$PPID}}"

if [ -d "$STATE_DIR" ]; then
  rm -f "$STATE_DIR"/*
  echo "[INFO] context-hooks: compaction detected, reset all state" >&2
fi

exit 0
