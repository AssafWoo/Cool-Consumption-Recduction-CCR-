# CCR — Cool Cost Reduction

> **60–95% token savings on Claude Code tool outputs.** CCR sits between Claude and your tools, compressing what Claude reads without changing what you ask it to do.

---

## Contents

- [How It Works](#how-it-works)
- [Installation](#installation)
- [Commands](#commands)
- [Handlers](#handlers)
- [BERT Pipeline](#bert-pipeline)
- [Configuration](#configuration)
- [Session Intelligence](#session-intelligence)
- [Hook Architecture](#hook-architecture)
- [CCR vs RTK](#ccr-vs-rtk)
- [Crate Overview](#crate-overview)

---

## How It Works

```
Claude runs: cargo build
    ↓ PreToolUse hook rewrites to: ccr run cargo build
    ↓ ccr run executes cargo, filters output through Cargo handler
    ↓ Claude reads: errors + warning count only (~87% fewer tokens)

Claude runs: Read file.rs  (large file)
    ↓ PostToolUse hook: BERT pipeline using current task as query
    ↓ Claude reads: compressed file content focused on what's relevant

Claude runs: git status  (seen recently)
    ↓ PreToolUse hook rewrites to: ccr run git status
    ↓ Pre-run cache hit (same HEAD+staged+unstaged hash)
    ↓ Claude reads: [PC: cached from 2m ago — ~1.8k tokens saved]
```

After `ccr init`, **this is fully automatic** — no changes to how you use Claude Code.

**What makes CCR different from rule-based proxies:**

- **38 handlers (50+ aliases)** — purpose-built filters for common dev tools (cargo, git, kubectl, gh, terraform, pytest, tsc, …)
- **BERT semantic routing** — unknown commands fuzzy-matched to nearest handler via sentence embeddings
- **Intent-aware compression** — uses Claude's last message as the BERT query so output relevant to the current task scores highest
- **Noise learning** — learns which lines are boilerplate in your project and pre-filters them before BERT runs
- **Pre-run cache** — git commands with identical repo state return cached output instantly
- **Read/Glob compression** — file reads ≥50 lines and large glob listings go through BERT compression too
- **Session dedup** — identical outputs across turns collapse to a single reference line
- **Elastic context** — pipeline tightens automatically as the session fills up

---

## Installation

### One-liner (macOS + Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/AssafWoo/Cool-Consumption-Reduction/main/install.sh | bash
```

Downloads the pre-built binary, installs to `~/.local/bin/ccr`, and runs `ccr init`.

Make sure `~/.local/bin` is on your PATH:

```bash
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.zshrc or ~/.bashrc
```

### From source

```bash
git clone https://github.com/AssafWoo/Cool-Consumption-Reduction.git && cd Cool-Consumption-Reduction
cargo build --release
cp target/release/ccr ~/.local/bin/
ccr init
```

> **First run:** CCR downloads the BERT model (~90 MB, `all-MiniLM-L6-v2`) from HuggingFace and caches it at `~/.cache/huggingface/`. Subsequent runs are instant.

---

## Commands

### ccr gain

```bash
ccr gain                    # overall summary
ccr gain --history          # last 14 days
ccr gain --history --days 7
```

```
CCR Token Savings
═════════════════════════════════════════════════
  Runs:           142
  Tokens saved:   182.2k  (77.7%)
  Cost saved:     ~$0.547  (at $3.00/1M — Sonnet 4.6 default)
  Today:          23 runs · 31.4k saved · 74.3%

Per-Command Breakdown
─────────────────────────────────────────────────────────────
COMMAND        RUNS       SAVED   SAVINGS   AVG ms  IMPACT
─────────────────────────────────────────────────────────────
cargo            45       89.2k     87.2%      420  ████████████████████
git              31       41.1k     79.1%       82  ████████████████
curl             12       31.2k     94.3%      210  ██████████████████
(pipeline)       18       12.4k     42.1%        —  ████████
(read)            8        4.1k     61.3%        —  ████
```

Pricing uses `cost_per_million_tokens` from `ccr.toml` if set, otherwise `ANTHROPIC_MODEL` env var (Opus 4.6: $15, Sonnet 4.6: $3, Haiku 4.5: $0.80), otherwise $3.00.

### ccr init

Installs hooks into `~/.claude/settings.json`. Safe to re-run — merges into existing arrays, preserving other tools' hooks. Registers PostToolUse for Bash, Read, and Glob.

### ccr noise

```bash
ccr noise           # show learned noise patterns for this project
ccr noise --reset   # clear all patterns
```

```
Learned noise patterns  (project key: a1b2c3d4e5f60718)
─────────────────────────────────────────────────────────
COUNT  SUPPR  RATE   STATUS    PATTERN
  24     22   91%   promoted  downloading [progress]
  18     16   88%   learning  warning: unused import
  12     11   91%   promoted  compiling package v
```

Lines seen ≥10 times with ≥90% suppression rate are promoted to permanent pre-filters. Error/warning/panic lines are never promoted.

### ccr expand

```bash
ccr expand ZI_3       # print original lines from a collapsed block
ccr expand --list     # list all available IDs in this session
```

When CCR collapses output, it embeds an ID in the marker:
```
[5 lines collapsed — ccr expand ZI_3]
```

### ccr update

```bash
ccr update
```

Checks the latest release on GitHub and replaces the binary in-place if a newer version is available. Also re-runs `ccr init` to refresh hooks with the new binary path.

```
Checking for updates (current: v0.5.3)...
Update available: v0.5.3 → v0.5.4
Downloading ccr-macos-arm64...
Updated to v0.5.4.
Re-registering hooks...
Done.

Checking for updates (current: v0.5.4)...
Already up to date (v0.5.4).
```

### ccr discover

Scans `~/.claude/projects/*/` JSONL history for Bash commands that ran without CCR. Reports estimated missed savings.

### ccr filter

```bash
cargo clippy 2>&1 | ccr filter --command cargo
cat big-log-file.txt | ccr filter
```

Reads stdin, runs the pipeline, writes to stdout. Useful outside of Claude Code.

### ccr run / ccr proxy

```bash
ccr run git status    # run through CCR handler
ccr proxy git status  # run raw (no filtering), record analytics baseline
```

When savings exceed 60%, the raw output is saved to `~/.local/share/ccr/tee/<ts>_<cmd>.log` and the path is appended to Claude's output so it can recover the full content.

---

## Handlers

38 handlers (50+ command aliases) in `ccr/src/handlers/`. Lookup cascade:

1. **Exact match** — direct command name
2. **Static alias table** — versioned binaries, wrappers, common aliases
3. **BERT similarity** — unknown commands matched to nearest handler (threshold 0.55)

**TypeScript / JavaScript**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **tsc** | `tsc` | ~90% | Groups errors by file. `Build OK` on clean. |
| **vitest** | `vitest` | ~88% | FAIL blocks + summary; drops `✓` lines. |
| **jest** | `jest`, `bun`, `deno`, `nx` | ~88% | `●` failure blocks + summary; drops `PASS` lines. |
| **eslint** | `eslint` | ~85% | Errors grouped by file, caps at 20 + `[+N more]`. |
| **next** | `next` | ~90% | `build`: route table collapsed, errors + page count. `dev`: errors + ready line. `lint`: ESLint-style. |
| **playwright** | `playwright` | ~88% | Failing test names + error messages; passing tests dropped. |
| **prettier** | `prettier` | ~80% | `--check`: files needing formatting + count. `--write`: file count. Mode auto-inferred. |

**Python**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **pytest** | `pytest`, `py.test` | ~87% | FAILED node IDs + AssertionError + short summary. |
| **pip** | `pip`, `pip3`, `uv`, `poetry`, `pdm`, `conda` | ~80% | `install`: `[complete — N packages]`. |
| **python** | `python`, `python3`, `python3.X` | ~60% | Traceback: keep block + final error. Long output: BERT. |

**DevOps / Cloud**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **kubectl** | `kubectl`, `k`, `minikube`, `kind` | ~85% | `get`: compact table. `logs`: BERT anomaly. `describe`: key sections. |
| **gh** | `gh` | ~90% | `pr list`/`issue list`: compact tables. `pr checks`: pass/fail counts. |
| **terraform** | `terraform`, `tofu` | ~88% | `plan`: `+`/`-`/`~` + summary. `apply`: resource lines + completion. |
| **aws** | `aws`, `gcloud`, `az` | ~85% | JSON → schema. `s3 ls`: grouped by prefix. |
| **make** | `make`, `gmake`, `ninja` | ~75% | Drops directory noise. Keeps errors + recipe failures. |
| **go** | `go` | ~82% | `build`/`vet`: errors only. `test`: FAIL blocks. |
| **golangci-lint** | `golangci-lint`, `golangci_lint` | ~88% | Diagnostics grouped by file; INFO/DEBUG runner noise dropped. |
| **prisma** | `prisma` | ~85% | `generate`: client summary. `migrate`: migration names. `db push`: sync status. |
| **mvn** | `mvn`, `mvnw`, `./mvnw` | ~80% | Drops `[INFO]` noise; keeps errors + reactor summary. |
| **gradle** | `gradle`, `gradlew`, `./gradlew` | ~80% | FAILED tasks, Kotlin errors, failure blocks. |
| **helm** | `helm`, `helm3` | ~85% | `list`: compact table. `status`/`diff`/`template`: structured. |

**System / Utility**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **cargo** | `cargo` | ~87% | `build`/`check`/`clippy`: JSON format, errors + warning count. `test`: failures + summary. |
| **git** | `git` | ~80% | `status` caps 20 files. `log` injects `--oneline`, caps 20. `diff`: `+`/`-`/`@@` only. |
| **curl** | `curl` | ~96% | JSON → type schema. Arrays: first-element schema + `[N items]`. |
| **docker** | `docker`, `docker-compose` | ~85% | `logs`: BERT anomaly. `ps`/`images`: compact table. |
| **npm/yarn** | `npm`, `yarn` | ~85% | `install`: package count. `test`: failures + summary. |
| **pnpm** | `pnpm`, `pnpx` | ~87% | `install`: `Packages:` + `Progress:` summary; drops progress bars. `run`/`exec`: errors + tail. |
| **clippy** | `clippy`, `cargo-clippy` | ~85% | Rustc-style diagnostics filtered; span lines dropped; duplicate warnings collapsed. |
| **journalctl** | `journalctl` | ~80% | Injects `--no-pager -n 200`. BERT anomaly scoring. |
| **psql** | `psql`, `pgcli` | ~88% | Strips borders, caps at 20 rows + `[+N more]`. |
| **brew** | `brew` | ~75% | `install`/`update`: status lines + Caveats. |
| **tree** | `tree` | ~70% | ≤30 lines pass through. >30: first 25 + summary. |
| **diff** | `diff` | ~75% | `+`/`-`/`@@`/header lines only. |
| **jq** | `jq` | ~80% | ≤20 lines pass through. Array: schema of first element + `[N items]`. |
| **env** | `env`, `printenv` | ~70% | Masks secrets, sorted, capped at 40. |
| **ls** | `ls` | ~80% | Dirs first, alphabetical, limit 40 + summary. |
| **cat** | `cat` | ~70% | ≤100 lines: pass through. 101–500: head/tail. >500: BERT. |
| **grep / rg** | `grep`, `rg` | ~80% | Groups by file, truncates lines, caps at 50 matches. |
| **find** | `find` | ~78% | Strips common prefix, groups by directory, caps at 50. |

---

## BERT Pipeline

CCR uses [`all-MiniLM-L6-v2`](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) (384-dim sentence embeddings, ~90 MB, downloaded once on first use via `fastembed`). Every line of output is embedded and scored; only the most informative lines are kept.

### Scoring

Base score: `1 - cosine_similarity(line_embedding, centroid)` — outliers score high, repetitive boilerplate scores low. Lines matching error/warning/panic patterns are **hard-kept** regardless of score.

The score is blended with a query when context is available:

```
final_score = 0.5 × anomaly_score + 0.5 × cosine_similarity(line, query_embedding)
```

The query comes from Claude's last assistant message (intent extraction) — so lines relevant to what Claude is currently working on rank higher than generic outliers.

### Passes

All seven passes run in sequence on every compressed output:

| Pass | What it does |
|------|-------------|
| **Noise pre-filter** | Removes project-specific boilerplate promoted by noise learning, before BERT runs |
| **Semantic clustering** | Near-identical lines (cosine > 0.85) collapse to one representative + `[N similar]` |
| **Entropy budget** | Samples embeddings to measure output diversity; uniform output (progress bars, install logs) gets a tight token budget automatically |
| **Anomaly scoring** | Scores each line against the centroid + intent query; keeps top-N by score |
| **Contextual anchors** | After selection, re-adds the nearest semantic neighbors of each kept line (e.g. the function signature above an error) |
| **Historical centroid** | Scores anomaly against a rolling mean of prior runs of the same command — genuinely new output stands out more than first-run spikes |
| **Delta compression** | Compares against previous run of same command; suppresses unchanged lines, surfaces new ones with `[Δ from turn N: +M new, K repeated]` |

### Fallback

If the model is unavailable or output is short (< `summarize_threshold_lines`), CCR falls back to head + tail. No crash, no empty output.

---

## Configuration

Config is loaded from the first file found: `./ccr.toml` → `~/.config/ccr/config.toml` → embedded default.

```toml
[global]
summarize_threshold_lines = 200  # trigger BERT summarization
head_lines = 30                  # head+tail fallback budget
tail_lines = 30
strip_ansi = true
normalize_whitespace = true
deduplicate_lines = true
# cost_per_million_tokens = 15.0  # override pricing in ccr gain

[tee]
enabled = true
mode = "aggressive"   # "aggressive" | "always" | "never"
max_files = 20

[commands.git]
patterns = [
  { regex = "^(Counting|Compressing|Receiving|Resolving) objects:.*", action = "Remove" },
  { regex = "^remote: (Counting|Compressing|Enumerating).*", action = "Remove" },
]

[commands.cargo]
patterns = [
  { regex = "^\\s+Compiling \\S+ v[\\d.]+", action = "Collapse" },
  { regex = "^\\s+Downloaded \\S+ v[\\d.]+", action = "Remove"   },
]
```

Pattern actions: `Remove` (delete line), `Collapse` (count → `[N lines collapsed]`), `ReplaceWith = "text"`.

To add a custom handler, implement the `Handler` trait and register it in `ccr/src/handlers/mod.rs`.

---

## Session Intelligence

CCR tracks state across turns within a session (identified by `CCR_SESSION_ID=$PPID`). State lives at `~/.local/share/ccr/sessions/<id>.json`.

**Cross-turn output cache** — Identical outputs (cosine > 0.92) across turns are collapsed to `[same output as turn 4 (3m ago) — 1.2k tokens saved]`.

**Semantic delta** — Repeated commands emit only new/changed lines: `[Δ from turn N: +M new, K repeated — ~T tokens saved]`. Subcommand-aware so `git status` and `git log` histories don't cross-contaminate.

**Elastic context** — As cumulative session tokens grow (25k → 80k), pipeline pressure scales 0 → 1, shrinking BERT budgets automatically. At >80% pressure a warning is appended.

**Pre-run cache** — git commands with identical HEAD+staged+unstaged state are served from cache (TTL 1h), skipping execution entirely.

**Intent-aware query** — Reads Claude's last assistant message from the live session JSONL and uses it as the BERT query, biasing compression toward what Claude is currently working on.

**Conversation compression** (ccr-sdk) — Tiered compression of old conversation turns: most-recent verbatim → tier 1 extractive (55%) → tier 2 generative via Ollama or extractive (20%). Compounding ~10–20% savings per turn across a long session.

---

## Hook Architecture

### PreToolUse

`ccr-rewrite.sh` calls `ccr rewrite "<cmd>"` before Bash executes:

- **Known handler** → rewrites to `ccr run <cmd>`, patches `tool_input.command`
- **Unknown** → exits 1, Claude Code uses original command
- **Compound commands** → each segment rewritten independently (`cargo build && git push` → `ccr run cargo build && ccr run git push`)
- **Already wrapped** → no double-wrap

### PostToolUse

Dispatches by `tool_name` — Bash, Read, or Glob:

- **Bash** — noise pre-filter → EC pressure → IX intent query → BERT pipeline → ZI blocks → delta compression → sentence dedup → session cache → analytics
- **Read** — files < 50 lines pass through; larger files go through BERT pipeline with intent query; session dedup by file path
- **Glob** — results ≤ 20 pass through; larger lists grouped by directory (max 60), session dedup by path-list hash

Never fails — returns nothing on error so Claude Code always sees a result.

---

## CCR vs RTK

| Feature | CCR | RTK |
|---------|-----|-----|
| Handler count | **31 (40+ aliases)** | 40+ |
| Unknown commands | BERT routing + fallback (~40%) | Pass through (0%) |
| Handler routing | Exact → alias → BERT similarity | Exact match only |
| Read tool compression | Yes (BERT pipeline ≥50 lines) | — |
| Glob tool compression | Yes (dir grouping + session dedup) | — |
| Intent-aware query | Yes (reads live session JSONL) | — |
| Project noise learning | Yes (auto-promotes at ≥90% suppression) | — |
| Pre-run structural cache | Yes (git by HEAD+staged+unstaged) | — |
| Cross-turn output cache | Yes (cosine > 0.92) | — |
| Elastic context | Yes (scales with session size) | — |
| Conversation compression | ccr-sdk: tiered + Ollama + dedup | — |
| Hooks preserved on init | Yes (merges arrays) | Overwrites |

---

## Crate Overview

```
ccr/            CLI binary — handlers, hooks, session state, commands
ccr-core/       Core library (no I/O) — pipeline, BERT summarizer, config, analytics
ccr-sdk/        Conversation compression — tiered compressor, deduplicator, Ollama
ccr-eval/       Evaluation suite — Q&A + conversation fixtures against Claude API
config/         Embedded default filter patterns (git, cargo, npm, docker)
```

---

## Uninstall

```bash
rm ~/.local/bin/ccr
rm ~/.claude/hooks/ccr-rewrite.sh
rm -rf ~/.local/share/ccr          # optional: cached data + analytics
# Remove CCR entries from ~/.claude/settings.json
```

---

## Contributing

Open an issue or PR on [GitHub](https://github.com/AssafWoo/Cool-Consumption-Reduction). To add a handler: implement the `Handler` trait and register it in `ccr/src/handlers/mod.rs` — see `git.rs` as a template.

---

## License

MIT — see [LICENSE](LICENSE).
