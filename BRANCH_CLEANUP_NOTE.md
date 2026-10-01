# Branch cleanup — `collord/wasim` (noted 2026-10-01)

Snapshot and recommended cleanup for the repo's branches. Written for later action because
branch deletion is **blocked for the Claude Code agent** (see "Why I couldn't do it" below) —
you'll need to run the deletions yourself.

## TL;DR

Go from **12 branches → 2**. Keep `main` and `claude/vensim-mdl-import-40llsf` (active work).
Delete the other **10**: 7 are already fully merged into `main`, 3 are dead legacy branches with
unrelated history. No open PRs exist, so deleting closes nothing.

## Verdict (vs `main` @ `d877eac`, 2026-08-04)

| Branch | Status | Why | Action |
|---|---|---|---|
| `claude/emitter-lens-targeting-t2r2s1` | fully merged (0 ahead, 3 behind) | all commits in main (PR #20) | delete |
| `claude/abm-wasim-lens-authoring-6dsw31` | fully merged (0 ahead, 13 behind) | all commits in main | delete |
| `claude/market-research-chat-ha1zoo` | fully merged (0 ahead, 25 behind) | all commits in main | delete |
| `claude/wasim-lenses-ui-planning-f52b7z` | fully merged (0 ahead, 35 behind) | all commits in main | delete |
| `claude/analytica-test-fixtures-vcz3wm` | fully merged (0 ahead, 66 behind) | all commits in main | delete |
| `claude/analytica-wasim-conversion-72rgfr` | fully merged (0 ahead, 76 behind) | all commits in main | delete |
| `claude/monte-carlo-handbook-memory-tf58ax` | fully merged (0 ahead, 97 behind) | all commits in main | delete |
| `engine-v2-schema` | legacy, **unrelated history** | no common ancestor with main; only unique top-level content is `schema-old`; its themes (v2 engine, units registry, dimensional validation, frontend v2 bridge) re-landed in main's lineage | delete |
| `claude/pull-main-code-xzfjc6` | legacy, **unrelated history** | no common ancestor; only unique content is `schema-old`; its themes (§17 copilot, §12 dashboards, §5 inspector editors, §11 results_spec) re-landed in main | delete |
| `claude/wasim-goldsim-gap-analysis-tmcon2` | legacy, **unrelated history** | no common ancestor; only unique content is `schema-old`; its themes (GoldSim gap analysis, BoundCrossing, calendar time_ref, authoring spec) re-landed in main | delete |
| **`main`** | canonical | — | **keep** |
| **`claude/vensim-mdl-import-40llsf`** | active (13 ahead, 1 behind, merges clean) | Vensim `.mdl` import work, this session | **keep (open)** |

### On the 3 legacy branches (the important nuance)
They report as "clean to merge" but share **no common ancestor** with `main` — the repo was
re-initialized at some point and these predate the reset. A "merge" would dump 126k–181k lines
across 400+ files of a parallel lineage into main. **Do not merge them.** They're superseded; the
only thing they carry that main lacks is a dir literally named `schema-old`. Safe to delete.

## The cleanup command

Run locally (where you have branch-delete rights), or use the GitHub UI (repo → Branches → trash
icon per branch):

```sh
git push origin --delete \
  claude/emitter-lens-targeting-t2r2s1 \
  claude/abm-wasim-lens-authoring-6dsw31 \
  claude/market-research-chat-ha1zoo \
  claude/wasim-lenses-ui-planning-f52b7z \
  claude/analytica-test-fixtures-vcz3wm \
  claude/analytica-wasim-conversion-72rgfr \
  claude/monte-carlo-handbook-memory-tf58ax \
  engine-v2-schema \
  claude/pull-main-code-xzfjc6 \
  claude/wasim-goldsim-gap-analysis-tmcon2
```

To re-verify a branch is fully merged before deleting (expect `0`):
`git rev-list --count origin/main..origin/<branch>`

## Why I couldn't do it

`git push --delete` returns **HTTP 403 — an organization-policy denial**: the agent's GitHub
access permits push and PRs but not branch deletion, and the GitHub MCP server exposes no
branch-delete tool. The agent proxy's own guidance is to report policy denials, not retry them.
So this is a manual step for a human with delete rights.

---
_Noted by Claude Code during the Vensim `.mdl` import work. Re-check `git branch -r` before acting —
branch state may have changed since 2026-10-01._
