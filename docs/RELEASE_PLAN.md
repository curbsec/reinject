# Release Plan — Marketplace & Public GitHub

## 1. Fix Plugin Self-Containment

**Status: BLOCKING**

When CC installs a plugin, it copies it to `~/.claude/plugins/cache`. Consumer hooks (like `supabase-context.sh`) currently hardcode `$HOME/reinject/hooks/lib/should-reinject.sh`. That works for manual install but breaks for marketplace installs because the path changes.

Consumer hooks need to use `${CLAUDE_PLUGIN_ROOT}/hooks/lib/should-reinject.sh` — but `CLAUDE_PLUGIN_ROOT` is only set when hooks defined in the plugin's `hooks.json` run. Consumer hooks are defined in the USER's settings, not the plugin's `hooks.json`. So consumers can't rely on `CLAUDE_PLUGIN_ROOT`.

**Options:**
- **a)** Symlink from `~/.claude/hooks/lib/` → plugin cache (fragile, breaks on updates)
- **b)** Consumer hooks discover the plugin path at runtime (e.g., `find ~/.claude/plugins/cache -name should-reinject.sh`)
- **c)** Install the library to a fixed path (`~/.local/lib/reinject/`) as a post-install step
- **d)** Ship the library as a standalone npm or brew package

**Decision: TBD**

## 2. Add LICENSE

**Status: Easy, need preference**

MIT is standard for dev tools. Apache-2.0 if you want patent protection. Pick one.

## 3. Validate Plugin Structure

```bash
claude plugin validate ~/reinject
```

Haven't run this yet — need to verify the structure passes CC's validator.

## 4. Test as Plugin with --plugin-dir

```bash
claude --plugin-dir ~/reinject
```

Verify the monitor and compact-reset hooks auto-register and fire correctly.

## 5. Rust Binary Distribution

The binary is platform-specific (macOS arm64 right now). For the plugin:
- Default stays jq (works everywhere)
- Rust is opt-in: user runs `cargo build --release` in `parsers/rust/` and sets `REINJECT_PARSER`
- Add a `make install-rust` or install script to the README
- Don't ship prebuilt binaries in git (bloat + wrong platform)

## 6. Create Marketplace Entry

Two paths:

**a) Own marketplace** (you control distribution):
```json
// .claude-plugin/marketplace.json
{
  "name": "yonatan-plugins",
  "owner": { "name": "Yonatan Horan" },
  "plugins": [{
    "name": "reinject",
    "source": { "source": "github", "repo": "yonatan-genai/reinject" },
    "description": "Context rot prevention — re-injects context when it drifts out of recency"
  }]
}
```

**b) Official Anthropic marketplace** (more visibility, review process):
Submit at `claude.ai/settings/plugins/submit` or `platform.claude.com/plugins/submit`

## 7. Transfer Repo

Currently at `yonatan-genai/reinject`. If you want it under your personal account or a dedicated org, transfer it before marketplace submission (the source URL is permanent).

## 8. Polish README for Public Audience

Current README assumes you know what context rot is. Public version needs:
- One-paragraph "why this matters" without jargon
- GIF or screenshot showing injection in action (optional but helps)
- Clear install → first-consumer-hook in <2 minutes

## 9. Add CI

- GitHub Action running `bash tests/test-should-reinject.sh`
- Optional: `cargo test` and `cargo build --release` for the Rust parser
- Keeps the repo credible for marketplace reviewers

---

## Priority Order

| Priority | Steps | Notes |
|----------|-------|-------|
| **Blocking** | #1, #3, #4 | Self-containment, validate, test as plugin |
| **Before submit** | #2, #6, #8, #9 | License, marketplace entry, README, CI |
| **Nice-to-have** | #5, #7 | Rust install script, repo transfer |

**Biggest open question:** #1 — how consumers find the library when the plugin is installed via marketplace.
