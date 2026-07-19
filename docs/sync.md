---
id: 74e8fe20-b721-5e97-9af6-86761a0aeefe
---

# `syncmd` — asset synchroniser: algorithm specification

> Status: **design** — this document defines the algorithm only. No implementation yet.
>
> Audience: implementers of the Rust core and its Python/CLI bindings.

## 1. Purpose & scope

Agentic harnesses (Claude Code, Codex, GitHub Copilot, Cursor, Gemini, …) each
read their own **instruction / context / skill files** but the *intent* of those
files is usually identical. A team edits `CLAUDE.md`, and `AGENTS.md`,
`.github/copilot-instructions.md`, `.cursorrules`, etc. silently drift out of
date. The same happens to skills (`SKILL.md` under different roots).

`syncmd` keeps a **group of equivalent assets** converged on the **latest
change**, so that whichever harness a contributor used, every other harness sees
the same up-to-date content.

The guiding idea, mirroring the research:

> **One canonical content per logical asset, several native mount points.**
> `syncmd` is the *compiler* that propagates the newest authored version of an
> asset to every member of its equivalence class.

### In scope (v1)

* Detecting **equivalence groups** of markdown assets under a path.

* Deciding **which member is the latest** authoritative version, using **git
  history** plus the working tree.

* **Propagating** that content to all other members (verbatim by default,
  transformable per target).

* **Detecting divergence** (two members independently edited since they last
  agreed) and resolving it via an explicit strategy.

### Out of scope (v1, noted in §16)

* Non-markdown assets, binary assets.

* Rendering MCP manifests / `config.toml` / plugin bundles (the "adapter
  compiler" layer).

* Networked / multi-repo sync.

* Auto-commit (v1 writes files; committing is the user's choice unless
  `--commit` is requested — see §12).

## 2. Terminology

| Term                        | Meaning                                                                                                                                                             |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Asset member** (`member`) | A concrete file on disk that represents one logical asset for one harness, e.g. `CLAUDE.md`.                                                                        |
| **Asset group** (`group`)   | A set of members that are *equivalent* — they should all carry the same logical content. e.g. `{CLAUDE.md, AGENTS.md, .github/copilot-instructions.md}`.            |
| **Registry**                | The configurable mapping that defines which filenames/paths form a group (§3).                                                                                      |
| **Canonical content**       | The single normalised body that, after a sync, every member of a group should carry (after per-target transform).                                                   |
| **Baseline** (`B`)          | The most recent point in git history at which the group **last agreed** (all present members had equal normalised content). The reference for "what changed since". |
| **Recency** `t(m)`          | A comparable timestamp for "how recently member `m` was authored" (§7). Dirty/working-tree edits rank as *now*.                                                     |
| **Winner**                  | The member whose content becomes canonical for this run.                                                                                                            |
| **Divergence / conflict**   | More than one member changed since the baseline.                                                                                                                    |

## 3. The asset registry (equivalence classes)

A group is defined by a **registry**: a set of rules, each describing an
equivalence class. v1 ships built-in defaults and reads an optional
`syncmd.toml` at the repo root to extend/override.

Two rule kinds:

1. **Fixed-name instruction files** — a flat list of repo-relative paths that
   are all equivalent:

   ```toml
   [[group]]
   name = "instructions"
   # ordered by canonical preference (used as a tie-break, see §10)
   members = [
     "AGENTS.md",                          # open cross-tool standard → preferred
     "CLAUDE.md",
     ".github/copilot-instructions.md",
     ".cursorrules",
     "GEMINI.md",
   ]
   ```

2. **Templated/skill groups** — a `{name}` wildcard binds members across roots,
   so each concrete `{name}` forms its own group:

   ```toml
   [[group]]
   name = "skills"
   pattern = "skills/{name}/SKILL.md"
   roots   = [".agents", ".claude", ".github"]
   # expands per discovered {name} into e.g.
   #   {.agents/skills/foo/SKILL.md, .claude/skills/foo/SKILL.md, .github/skills/foo/SKILL.md}
   ```

Registry properties:

* **Member order is significant**: it encodes *canonical preference*, used only
  as a deterministic tie-break (§10), never to override recency.

* A member that does not exist on disk is still part of the group — it is a
  **creation target** (§11).

* Built-in defaults cover the instruction group above. Skills and others are
  opt-in in v1.

## 4. CLI / API contract

```
syncmd <path> [options]
```

* `<path>` is a file or directory **inside a git repository**.

  * **File** → resolve the group(s) that this file is a member of, sync those.

  * **Directory** → discover every group whose members fall under this path,
    sync each. (Recursive; the repo root syncs everything the registry knows.)

* Library (Rust) and Python bindings expose the same operation as a function
  returning a structured **plan/result** (see §13), so callers can preview or
  drive it programmatically. The CLI is a thin wrapper over that function.

Key options (full set in §10/§12):

| Option                                  | Default  | Effect                                               |
| --------------------------------------- | -------- | ---------------------------------------------------- |
| `--strategy {newest,error,interactive}` | `newest` | Divergence resolution (§10).                         |
| `--dry-run`                             | off      | Compute and print the plan; write nothing.           |
| `--commit`                              | off      | Stage & commit the propagated changes after writing. |
| `--backup`                              | on       | Write `*.syncmd.bak` for any overwritten member.     |
| `--allow-delete`                        | off      | Allow propagating a deletion (§11).                  |
| `--include / --exclude <glob>`          | —        | Restrict which groups/members are considered.        |

## 5. Preconditions

1. **Git is required.** If `<path>` is not inside a git work tree (`git
   rev-parse --is-inside-work-tree` fails / no `.git`), `syncmd` **errors out**
   with a clear message and a non-zero exit code. Rationale: the whole "align to
   the latest" decision is derived from history; without it we would be guessing
   which side is newer and could silently clobber the wrong file.

   * Error text suggests `git init` if the user genuinely wants tracking, but
     `syncmd` never initialises a repo on the user's behalf.
2. The path must exist and resolve inside the repo.
3. At least one member of a discovered group must exist on disk (otherwise there
   is nothing to propagate; the group is skipped with an info note).

## 6. Normalisation & transforms

Comparison and writing are separated so that cosmetic differences don't trigger
false syncs and so that targets can carry harness-specific shape.

* **`normalise(content) -> norm`** — used for *all equality/baseline
  comparisons*. v1 normalisation:

  * Convert line endings to `\n`.

  * Ensure exactly one trailing newline.

  * Do **not** alter interior content (no reflow, no case folding).

  * (Configurable hooks for future: strip a generated-header banner, ignore
    front-matter ordering.)

* **`transform(canonical, target_member) -> bytes`** — used when *writing* a
  member. v1 default is **identity** (verbatim mirror). The hook exists so a
  target can, for example, prepend a \`\` banner, or wrap content in a harness-specific preamble. A
  transform must be paired with its inverse in `normalise` so a generated file
  is not mistaken for an independent edit on the next run.

> v1 ships identity transform + the line-ending/trailing-newline normaliser.
> Anything fancier is registry-configured and out of the default path.

## 7. Recency model — what "latest" means

Each member gets a comparable recency `t(m)`:

1. **Dirty or untracked** working-tree content (working blob ≠ HEAD blob, or the
   file is untracked) ⇒ `t(m) = NOW` (ranked above every committed timestamp).
   Tie-break among multiple "now" members falls to committed time then registry
   order. *Intuition:* you just edited `CLAUDE.md` and ran `syncmd`; your live
   edit must win over a week-old committed `AGENTS.md`.
2. **Clean, tracked** ⇒ `t(m) =` committer date of the last commit that touched
   the path: `git log -1 --format=%cI -- <member>`. (Committer date, not author
   date: it reflects when the change actually landed on this branch.)
3. **Absent** ⇒ `t(m) = -∞` (a non-existent member can never be the winner
   purely on recency; it is only ever a *target*, unless it was just deleted —
   see §11).

`cur(m)` = normalised current content (working tree if present, else absent).

## 8. Baseline detection (git-history extraction)

The baseline `B` is the reference for "what changed". It is the **most recent
commit at which the group last agreed**.

Algorithm:

1. List commits that touched **any** member, newest-first, limited to the
   group's paths:
   `git rev-list HEAD -- <m1> <m2> … <mk>`
   (Restricting to member paths is safe: between two member-touching commits no
   member's content changes, so the most recent agreement can only occur at one
   of these commits — or at HEAD, already handled in §9 step 1.)
2. Walk that list newest→oldest. For each commit `c`:

   * For every member `p`, read its blob at `c`: `git show c:p`, or mark
     **ABSENT** if the path didn't exist there. Normalise each present blob.

   * Let `present` = members with content at `c`.

   * **Agreement** holds iff `|present| ≥ 2` **and** all `present` contents are
     equal. (We require ≥2 so that a lone file in early history is not mistaken
     for a group-wide baseline.)

   * The first `c` (newest) where agreement holds ⇒ `B = c`,
     `base_content = ` that agreed content. **Stop.**
3. If no commit agrees ⇒ **no baseline** (`B = ⊥`). This is the **bootstrap**
   case (the group has never been in sync, e.g. only `CLAUDE.md` ever existed).

`base(m)` for the change test (§9):

* If `B ≠ ⊥`: `base(m) = base_content` for every member (the agreed content).

* If `B = ⊥`: `base(m) = ⊥` for every member; *every present member with content
  is treated as "changed"* (so bootstrap reduces to "newest existing member
  wins", §9 step 5/6).

## 9. Core sync algorithm

For one group:

```
INPUTS:  members[], registry order, strategy
DERIVED: cur(m), t(m) for each m (§7);  B, base_content (§8)

1. EARLY EXIT — already in sync
   present = { m : exists(m) }
   if |present| == |members|  AND  all cur(m) equal across present:
       -> NO-OP for this group. (Every target already carries the content
          and none is missing.)   report: "in sync"

2. Compute base(m) (§8).

3. CHANGED SET
   changed = { m : cur(m) != base(m) }     // see encoding below
       - present member whose content != base_content      -> changed (edited)
       - absent member that existed at B                    -> changed (deleted)
       - present member that was ABSENT at B (or B = ⊥)      -> changed (created)
       - absent member that was also ABSENT at B            -> NOT changed
                                                              (just a target to fill)

4. if |changed| == 0:
       // contents differ (step 1 failed) yet nothing changed vs baseline:
       // contradictory -> treat conservatively as a conflict, do not guess.
       -> CONFLICT (report, apply strategy as in step 6 over `present`)

5. if |changed| == 1:
       winner = the single changed member
       if winner is a *deletion* (absent now, existed at B):
            -> see §11 (default: skip, warn unless --allow-delete)
       canonical = cur(winner)
       -> PROPAGATE (step 7)

6. if |changed| > 1:        // DIVERGENCE
       apply --strategy:
         newest      -> winner = argmax t(m) over `changed`
                        (tie-break: registry order / canonical preference, §10)
                        canonical = cur(winner); warn, list overridden members.
         error       -> abort group with a conflict report; non-zero exit.
         interactive -> present per-member diffs vs base_content; user picks
                        the winner (or aborts).  CLI only.

7. PROPAGATE
   for each target m in members where m != winner:
       want = transform(canonical, m)          (§6; default identity)
       if exists(m) and normalise(read(m)) == normalise(want):
            skip (already correct)
       else:
            if exists(m) and --backup: write m -> m.syncmd.bak
            atomically write `want` to m   (create parent dirs as needed)
   report: winner, written targets, skipped, backups.

8. (optional) if --commit: stage changed members and commit with a
   "syncmd: align <group> to <winner>" message.
```

### Why this is "smart align to the latest" and not blind last-writer-wins

A naive `syncmd` would just pick the file with the newest commit and copy it
everywhere. That silently destroys a concurrent edit. The baseline (§8) lets us
*distinguish* the two cases:

* **One member moved since the group last agreed** → unambiguous, propagate it.

* **Several moved independently** → real divergence; we refuse to silently pick
  unless told how (`--strategy`), and even under `newest` we **back up** the
  losers and **report** them.

## 10. Divergence resolution strategies

Selected via `--strategy` (default `newest`):

* **`newest`** — winner = member with greatest `t(m)` among the changed set.
  Deterministic tie-break when timestamps are equal (e.g. two dirty files):

  1. higher committer date, then
  2. earlier position in the registry `members` order (canonical preference —
     so `AGENTS.md` beats `CLAUDE.md` on an exact tie).
     Always emits a warning listing the overridden members and where their prior
     content was backed up.

* **`error`** — do not write anything for the diverged group; print a structured
  conflict report (each member's recency + a diff vs baseline) and exit
  non-zero. Other, non-diverged groups in the same run still proceed; the run's
  exit code reflects that at least one group could not be resolved.

* **`interactive`** — CLI-only; show diffs and let the user choose the winner or
  skip. Falls back to `error` when stdin is not a TTY.

## 11. Creation & deletion semantics

* **Creation** (a registry member that doesn't exist yet) is the normal
  propagation target — the winner's content is written to it (parent dirs
  created). This is exactly the bootstrap path: first run with only `CLAUDE.md`
  present creates `AGENTS.md` et al.

* **Deletion** (the only changed member is one that was *removed* since the
  baseline) is treated cautiously: by default `syncmd` **does not** delete the
  other members (that would let an accidental `rm` cascade across the group). It
  **warns** and leaves the group diverged. With `--allow-delete`, the deletion
  is propagated (other members removed, backups written first). Deletion never
  happens implicitly.

## 12. Idempotency & safety

* **Idempotent.** After a successful sync all members carry equal normalised
  content; an immediate re-run hits §9 step 1 (no-op).

* **Atomic writes.** Each member is written to a temp file in the same directory
  then `rename`d into place, so a crash never leaves a half-written asset.

* **Backups.** Any overwritten member is copied to `<member>.syncmd.bak` first
  (suppress with `--no-backup`). Backups are ignored by the registry/discovery.

* **Dry-run.** `--dry-run` produces the full plan (winner, per-target action,
  diffs) and writes nothing — same structured result the API returns.

* **No implicit git mutation.** v1 never commits unless `--commit` is passed and
  never runs `git add` on unrelated files. It only writes working-tree files
  otherwise, leaving the user in control of the commit.

* **No history rewriting, ever.** `syncmd` only *reads* history (§7/§8).

## 13. Outputs, reporting & exit codes

The library returns a structured **`SyncReport`** (CLI prints a human/`--json`
form of it):

```
SyncReport {
  groups: [
    GroupReport {
      name, members,
      status: in_sync | propagated | diverged_resolved | conflict | skipped,
      baseline: { commit | none },
      winner:   { member, reason: single_change | newest | chosen | bootstrap } | none,
      actions:  [ { target, action: wrote|created|skipped|backed_up|deleted } ],
      overridden: [ member ],     // losers under `newest`
      diffs?: ...                 // included on conflict / dry-run
    }, ...
  ]
}
```

Exit codes (CLI):

| Code | Meaning                                                                    |
| ---- | -------------------------------------------------------------------------- |
| `0`  | All discovered groups in sync or successfully propagated.                  |
| `1`  | At least one unresolved conflict (`--strategy error`/`interactive` abort). |
| `2`  | Precondition failure (not a git repo, bad path).                           |
| `3`  | I/O / git invocation error.                                                |

## 14. Worked examples

**A. Live edit → mirror (the common case).**
You edit `CLAUDE.md` (uncommitted) in a repo that already has a synced
`AGENTS.md`. `t(CLAUDE.md)=NOW`, `t(AGENTS.md)=` last commit. Baseline = the
commit where they agreed; only `CLAUDE.md` changed ⇒ winner `CLAUDE.md`,
`AGENTS.md` rewritten. Re-run = no-op.

**B. Bootstrap.**
Only `CLAUDE.md` exists; `AGENTS.md`, `.github/copilot-instructions.md` are
registry members but absent. No commit ever had ≥2 members ⇒ `B=⊥`. `CLAUDE.md`
is the only present (changed-from-absent) member ⇒ winner; the others are
*created*.

**C. Real divergence.**
Last week someone committed an edit to `AGENTS.md`; yesterday someone committed a
different edit to `CLAUDE.md`; never synced. Baseline = their last common commit.
Both changed ⇒ divergence. `--strategy newest` ⇒ `CLAUDE.md` (yesterday) wins,
`AGENTS.md` backed up + rewritten, warning emitted. `--strategy error` ⇒ nothing
written, conflict report, exit 1.

**D. Already converged.**
All members byte-identical (after normalisation) ⇒ §9 step 1 no-op, exit 0.

**E. Skill group.**
`syncmd .` with the skills rule discovers `foo` present only under
`.claude/skills/foo/SKILL.md`. Group = the three roots' `foo/SKILL.md`; bootstrap
creates the `.agents` and `.github` copies.

## 15. Edge cases & decisions

* **Detached HEAD / shallow clone.** History walk uses whatever is reachable
  from `HEAD`; a shallow clone may not reach the true baseline → `syncmd` may
  see `B=⊥` and treat the newest present member as winner. It **warns** when the
  walk hits a shallow boundary so the user can `--unshallow` if precision
  matters. It never fabricates a baseline.

* **Symlinked members** (a common cross-harness trick, e.g. `.claude/skills ->
  ../.agents/skills`). If two members resolve to the **same inode**, they are
  collapsed to one logical member (a symlink is already "in sync" by
  construction); `syncmd` does not rewrite through a symlink to clobber its
  target twice.

* **Member outside the repo / outside** **`<path>`.** Discovery only considers
  members within the repo; a directory `<path>` narrows to groups whose members
  live under it. A group straddling the boundary is reported as partially
  out-of-scope rather than half-synced.

* **CRLF repos /** **`.gitattributes`** **eol.** Normalisation (§6) neutralises line
  endings for comparison; writes use `\n` unless a target transform says
  otherwise.

* **Binary / non-UTF8 content** in a markdown member → group skipped with an
  error note (v1 is text-only).

* **Equal recency, different content** (two dirty files) → handled by the
  deterministic tie-break in §10; never a coin flip.

* **Baseline contradiction** (§9 step 4) → conservative conflict, never a
  silent guess.

## 16. Open questions / future work

* **Transform/adapter layer beyond identity.** Real `CLAUDE.md` vs `AGENTS.md`
  may want a thin per-target preamble. Needs a paired `transform`/`normalise`
  so generated banners don't read as independent edits (§6).

* **Adapter compiler** for non-markdown targets (MCP `config.toml`, Copilot
  agent YAML) — the research's "render one canonical manifest into each
  harness". Same baseline/recency engine, different writer.

* **Sync marker / provenance.** Optionally record the winning blob hash + source
  member in front-matter or a `.syncmd/state` file to make baseline detection
  O(1) and survive shallow clones, instead of re-deriving from history each run.

* **Auto-watch / pre-commit hook** mode.

* **Per-section merge** instead of whole-file last-writer-wins, for files that
  genuinely accrete from multiple harnesses.

```
```

