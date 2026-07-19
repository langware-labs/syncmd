---
id: dedd4778-ae3b-5a1f-af39-9f386dfb8857
---

# syncmd

**Keep your AI-harness instruction and skill files in sync — automatically, from git.**

---

## The problem

Every agentic tool reads its own instruction/context/skill files, but they almost always
mean the same thing:

| Harness | Instruction file |
| --- | --- |
| Claude Code | `CLAUDE.md` |
| Cross-tool standard | `AGENTS.md` |
| GitHub Copilot | `.github/copilot-instructions.md` |
| Cursor | `.cursorrules`, `.cursor/rules/project.mdc` |
| Gemini | `GEMINI.md` |
| Windsurf | `.windsurf/rules/rules.md` |
| Cline | `.clinerules` |
| Roo Code | `.roo/rules/rules.md` |
| JetBrains Junie | `.junie/guidelines.md` |
| Amazon Q | `.amazonq/rules/project.md` |
| Kiro | `.kiro/steering/project.md` |
| Goose | `.goosehints` |
| Warp | `WARP.md` |
| Qwen Code | `QWEN.md` |
| Aider | `CONVENTIONS.md` |
| OpenHands | `.openhands/microagents/repo.md` |
| Augment | `.augment-guidelines` |
| Replit | `replit.md` |

Run `syncmd formats` to list every built-in harness label and its mount points. By default
`plan`/`sync` cover **all** formats; restrict with a comma-separated list:

```bash
syncmd sync --formats=claude,agents,cursor
```

A teammate edits `CLAUDE.md`. Now `AGENTS.md`, `.cursorrules`, and the rest silently drift out
of date — and whoever opens the repo with a *different* tool gets stale guidance. The same
happens to **skills** (`SKILL.md` under `.claude/`, `.agents/`, `.github/`) and **specs**.

`syncmd` treats a group of equivalent files as **one logical asset with several native mount
points**, decides which copy is the *latest authoritative version* using **git history plus
your working tree**, and propagates it to the rest. Edit whichever file your tool uses; run
`syncmd`; everything converges.

> It is a **compiler**, not a copier: it figures out which member actually changed since the
> group last agreed, so a concurrent edit is reported as a conflict instead of being silently
> clobbered.

---

## Install

```bash
# CLI
cargo install syncmd-cli          # provides the `syncmd` binary
npm install -g syncmd             # downloads the matching prebuilt CLI

# Python SDK
pip install syncmd                # provides the `syncmd` module
```

Building from source:

```bash
cargo build --release             # CLI at target/release/syncmd
maturin develop -m crates/syncmd-py/pyproject.toml   # Python module into the active venv
```

`syncmd` requires the target path to be **inside a git repository** — the whole "align to the
latest" decision is derived from history. It never initializes a repo or commits on your
behalf.

---

## Usage

The basic unit is **`syncmd sync <path>`**, where `<path>` is a file or directory inside a git
repo:

```bash
syncmd sync .                     # sync every known group in the repo
syncmd sync CLAUDE.md             # sync just the group CLAUDE.md belongs to
syncmd sync .claude/skills/foo/   # sync the `foo` skill across all roots
```

Because the unit is a path, it composes upward — point it at a project, or (later) a whole
home directory of projects, and each repo is handled independently.

### What "latest" means

For each member `syncmd` computes a recency:

1. **Uncommitted / untracked edits rank as _now_** — your live edit to `CLAUDE.md` beats a
   week-old committed `AGENTS.md`.
2. Otherwise, the committer date of the last commit that touched the file.
3. A file that doesn't exist yet is only ever a *creation target*.

It finds the **baseline** — the most recent commit where the group last agreed — and looks at
what changed since:

- **One member changed** → unambiguous, propagate it.
- **Several changed independently** → a real conflict; resolved per `--strategy`, and losers
  are always backed up and reported.

### Common commands

```bash
# Preview only — compute the plan, write nothing
syncmd plan .
syncmd sync . --dry-run

# Choose how divergence is resolved
syncmd sync . --strategy newest        # default: newest edit wins (losers backed up)
syncmd sync . --strategy error         # refuse to write, report the conflict, exit 1
syncmd sync . --strategy interactive   # show diffs, you pick the winner

# Machine-readable output
syncmd sync . --json

# Safety toggles
syncmd sync . --no-backup              # don't write <member>.syncmd.bak
syncmd sync . --allow-delete           # propagate a deletion across the group
```

### Worked examples

| Situation | Result |
| --- | --- |
| **A. Live edit** — you edit `CLAUDE.md` (uncommitted), `AGENTS.md` already synced | `CLAUDE.md` wins; `AGENTS.md` rewritten; re-run is a no-op |
| **B. Bootstrap** — only `CLAUDE.md` exists | the other members are *created* from it |
| **C. Divergence** — `AGENTS.md` and `CLAUDE.md` edited in separate commits | `newest` wins & backs up the other; `error` writes nothing and exits 1 |
| **D. Converged** — all members byte-identical | no-op, exit 0 |
| **E. Skill** — `foo` exists only under `.claude/` | created under `.agents/` and `.github/` |

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | All groups in sync or successfully propagated |
| `1` | At least one unresolved conflict (`--strategy error`/`interactive` abort) |
| `2` | Precondition failure (not a git repo, bad path, bad config) |
| `3` | I/O or git error |

---

## Configuration

`syncmd` ships sensible built-in defaults (instructions, skills, agents, specs) — zero config
needed. To extend or override, drop a `syncmd.toml` at the repo root:

```toml
# A fixed list of equivalent files (order = canonical preference, used only to break ties)
[[rule]]
group = "instructions"
type = "claude_md"
layout = "file"
strategy = "newest"
mounts = [
  { harness = "agents",  pattern = "AGENTS.md" },
  { harness = "claude",  pattern = "CLAUDE.md" },
  { harness = "copilot", pattern = ".github/copilot-instructions.md" },
  { harness = "cursor",  pattern = ".cursorrules" },
  { harness = "gemini",  pattern = "GEMINI.md" },
]

# A templated, folder-backed group — each {name} becomes its own asset
[[rule]]
group = "skill:{name}"
type = "skill"
layout = "folder"
strategy = "newest"
mounts = [
  { harness = "claude", pattern = ".claude/skills/{name}/SKILL.md" },
  { harness = "agents", pattern = ".agents/skills/{name}/SKILL.md" },
  { harness = "github", pattern = ".github/skills/{name}/SKILL.md" },
]
```

---

## SDK (Python)

The Rust core is exposed as a Python module. `sync()` and `plan()` return a report object
whose `to_dict()` matches the flowpad-sdk record shape (`type`/`id` first, snake_case, nulls
omitted), so it drops straight into flow-sdk tooling.

```python
import json
import syncmd

# Preview — no writes
report = syncmd.plan(".")

for group in report.groups:
    print(group.name, group.decision, group.winner_oid)

# Apply, choosing a strategy
report = syncmd.sync(".", strategy="newest", backup=True)

print(report.summary.written, "files updated")

# flow-sdk-shaped dict / JSON
print(json.dumps(report.to_dict(), indent=2))
print(report.to_json())
```

```python
# Round-trips into flow-sdk records
from flow_sdk.fs_store import FSRecord
rec = FSRecord.from_dict(report.to_dict())
```

`plan(path, *, recursive=False)` and
`sync(path, *, strategy="newest", dry_run=False, backup=True, create_missing=True,
allow_delete=False, recursive=False)` mirror the CLI exactly — the CLI and the SDK are both
thin wrappers over the same core functions.

---

## How it works (one paragraph)

The blob **object id (OID)** is the only content primitive: two files are "in sync" when they
point at the same blob. Equality, "changed since the baseline", and group agreement are all
OID comparisons — `syncmd` never diffs bytes until it writes the single winning file. The
decision is made once per group (who wins); the actions are applied per member (write / create
/ skip / back up). All writes are atomic (temp file + rename) and overwritten files are backed
up to `<member>.syncmd.bak` first.

See [`docs/sync.md`](docs/sync.md) for the full algorithm specification.
See [`docs/releasing.md`](docs/releasing.md) for the multi-registry release flow.

---

## Status

v1 in development. In scope: markdown instruction/skill/agent/spec groups, git-history-based
recency, propagation, divergence detection. Out of scope (for now): non-markdown/binary
assets, the adapter-compiler layer (rendering one canonical source into harness-specific
manifests like MCP `config.toml`), networked/multi-repo sync, and auto-commit.
