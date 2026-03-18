# CCR — Cool Cost Reduction

> **60–95% token savings on Claude Code tool outputs.** CCR intercepts shell commands before Claude reads their output, routes them through specialized handlers, and returns compact summaries.

---

## Contents

- [How It Works](#how-it-works)
- [Installation](#installation)
- [Commands](#commands)
- [Handlers](#handlers)
- [Pipeline (Unknown Commands)](#pipeline-unknown-commands)
- [Session Intelligence](#session-intelligence)
- [Configuration](#configuration)
- [Analytics](#analytics)
- [Tee: Raw Output Recovery](#tee-raw-output-recovery)
- [CCR-SDK: Conversation Compression](#ccr-sdk-conversation-compression)
- [Hook Architecture](#hook-architecture)
- [CCR vs RTK](#ccr-vs-rtk)
- [Crate Overview](#crate-overview)

---

## How It Works

```
Claude issues: git status
    ↓ PreToolUse hook (ccr-rewrite.sh)
      git is a known handler → patches command to: ccr run git status
    ↓ ccr run executes git status, filters output, writes tee file
    ↓ Claude reads: compact changed-file list (80% fewer tokens)

Claude issues: some-unknown-tool
    ↓ PreToolUse: no handler match (exact → alias table → BERT similarity)
    ↓ PostToolUse hook (ccr hook)
      query-biased BERT compression (~40% savings on anything)
      + session dedup: "[same output as turn N (3m ago) — 1.2k tokens saved]"
    ↓ Claude reads: compressed, deduplicated output
```

**CCR's edge over rule-based proxies:**

- **31 handlers (40+ aliases)** — covers the full surface area of common dev tools
- **BERT semantic routing** — unknown commands fuzzy-matched to nearest handler via sentence embeddings
- **Query-biased BERT** — anomaly scoring blended with command-string relevance for smarter summarization
- **Session output cache** — identical tool outputs across turns replaced with a single reference line
- **Session-aware compression** — budget tightens as context fills; sentence-level cross-turn dedup via ccr-sdk
- **Log anomaly scoring** — docker/kubectl/journalctl use centroid-distance BERT scoring instead of exact-match dedup
- **Smart `cat`** — files >500 lines use BERT importance scoring, not head+tail
- **Conversation compression** (ccr-sdk) — 10–20% savings per turn that compound across a long session

---

## Installation

```bash
git clone <repo> && cd ccr
cargo build --release
cp target/release/ccr ~/.local/bin/   # or any directory on PATH
ccr init                               # registers hooks in ~/.claude/settings.json
```

`ccr init` writes `~/.claude/hooks/ccr-rewrite.sh` (PreToolUse) and merges both hook entries into `settings.json` **without removing existing hooks** from other tools.

Verify:

```bash
ccr run git status    # should print compact output
ccr gain              # should show a run recorded
```

---

## Commands

### ccr run

Execute a command through CCR's handler pipeline.

```
ccr run <command> [args...]
```

1. Looks up handler for `argv[0]`
2. `handler.rewrite_args()` — optionally injects flags (e.g. `--message-format json` for cargo)
3. Executes, capturing stdout + stderr combined
4. Writes raw output to `~/.local/share/ccr/tee/<ts>_<cmd>.log`
5. `handler.filter()` → compact output; falls back to BERT pipeline if no handler
6. Appends `[full output: <path>]` when savings exceed 60%
7. Records `{ command, subcommand, input_tokens, output_tokens, duration_ms }` to analytics
8. Propagates original exit code

### ccr gain

```
ccr gain [--history] [--days N]
```

**Default view:**
```
CCR Token Savings
═════════════════════════════════════════════════
  Runs:           142
  Tokens saved:   182.2k  (77.7%)
  Cost saved:     ~$0.547  (at $3.00/1M input tokens)
  Today:          23 runs · 31.4k saved · 74.3%

Per-Command Breakdown
─────────────────────────────────────────────────────────────
COMMAND        RUNS       SAVED   SAVINGS   AVG ms  IMPACT
─────────────────────────────────────────────────────────────
cargo            45       89.2k     87.2%      420  ████████████████████
git              31       41.1k     79.1%       82  ████████████████
curl             12       31.2k     94.3%      210  ██████████████████
(pipeline)       18       12.4k     42.1%        —  ████████
```

**History view:**
```bash
ccr gain --history          # last 14 days (default)
ccr gain --history --days 7
```
```
CCR Daily History  (last 14 days)
────────────────────────────────────────────────────────────
DATE          RUNS        SAVED   SAVINGS   COST SAVED
2026-03-17      23        31.4k     74.3%       $0.094
2026-03-16      41        58.1k     78.1%       $0.174
```

### ccr discover

Scan `~/.claude/projects/*/` JSONL history for Bash calls not yet wrapped in `ccr run`. Reports estimated savings per command and suggests running `ccr init`.

### ccr init

Installs both hooks into `~/.claude/settings.json`. Safe to re-run — merges into existing arrays so other tools' hooks are preserved. Uses the absolute path of the running binary in all hook commands.

### ccr filter

```
ccr filter [--command <hint>]
```

Reads stdin, runs the four-stage pipeline, writes to stdout. Useful for piping arbitrary output: `cargo clippy 2>&1 | ccr filter --command cargo`.

### ccr proxy

Execute raw (no filtering), record analytics as a baseline. Writes a `_proxy.log` tee file.

---

## Handlers

31 handlers (40+ command aliases) in `ccr/src/handlers/`. Handler lookup is a 3-level cascade:

1. **Exact match** — direct command name lookup
2. **Static alias table** — versioned binaries (`python3.11`→`python`), wrappers (`./gradlew`→`gradle`), common aliases (`bun`→`jest`, `az`→`aws`, etc.)
3. **BERT similarity** — unknown commands embedded and compared to 15 handler representative sentences; threshold 0.55

Each handler implements:

```rust
fn rewrite_args(&self, args: &[String]) -> Vec<String>  // inject flags before execution
fn filter(&self, output: &str, args: &[String]) -> String
```

**TypeScript / JavaScript**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **tsc** | `tsc` | ~90% | Groups `error TS\d+` lines by file. `Build OK` on clean. |
| **vitest** | `vitest` | ~88% | FAIL blocks + summary; drops `✓` passing lines. |
| **jest** | `jest`, `bun`, `deno`, `nx` | ~88% | `●` failure blocks + summary; drops `PASS` lines. |
| **eslint** | `eslint` | ~85% | Errors grouped by file, caps at 20 + `[+N more]`. |

**Python**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **pytest** | `pytest`, `py.test` | ~87% | FAILED node IDs + AssertionError block + short summary. |
| **pip** | `pip`, `pip3`, `uv`, `poetry`, `pdm`, `conda` | ~80% | `install`: `[complete — N packages]`. `uv`: parses resolved/installed counts. |
| **python** | `python`, `python3`, `python3.X` | ~60% | Traceback: keep block + final error. Long output: BERT summarize. |

**DevOps / Cloud**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **kubectl** | `kubectl`, `k`, `minikube`, `kind` | ~85% | `get`: compact table (NAME/READY/STATUS/AGE). `logs`: BERT anomaly scoring. `describe`: key sections only. |
| **gh** | `gh` | ~90% | `pr list`/`issue list`: compact tables. `pr checks`: `✓ N passed, ✗ M failed`. `run view`: failed steps only. |
| **terraform** | `terraform`, `tofu` | ~88% | `plan`: `+`/`-`/`~` lines + summary. `apply`: resource lines + completion. `init`/`validate`: errors or success. |
| **aws** | `aws`, `gcloud`, `az` | ~85% | JSON → schema (allowlisted subcommands only). `s3 ls`: grouped by prefix. |
| **make** | `make`, `gmake`, `ninja` | ~75% | Drops directory noise. Keeps compiler errors + recipe failures. |
| **go** | `go` | ~82% | `build`/`vet`: errors only. `test`: FAIL blocks. `run`: traceback or BERT. |
| **mvn** | `mvn`, `mvnw`, `./mvnw` | ~80% | Drops `[INFO]` noise; keeps errors, warnings, reactor summary. |
| **gradle** | `gradle`, `gradlew`, `./gradlew` | ~80% | Keeps FAILED tasks, Kotlin errors, failure blocks. |
| **helm** | `helm`, `helm3` | ~85% | `list`: compact table. `status`/`diff`/`template`: structured output. |

**System / Utility**

| Handler | Keys | Savings | Key behavior |
|---------|------|---------|-------------|
| **cargo** | `cargo` | ~87% | `build`/`check`/`clippy`: injects `--message-format json`, keeps errors + warning count. `test`: failure names + detail + summary. |
| **git** | `git` | ~80% | `status` caps 20 files. `log` injects `--oneline`, caps 20. `diff` keeps `+`/`-`/`@@` only. |
| **curl** | `curl` | ~96% | JSON → type schema. Arrays: first-element schema + `[N items total]`. |
| **docker** | `docker`, `docker-compose` | ~85% | `logs`: BERT anomaly scoring (centroid distance). `ps`/`images`: compact table. |
| **npm/pnpm/yarn** | `npm`, `pnpm`, `yarn` | ~85% | `install`: package count. `test`: failures + summary. |
| **journalctl** | `journalctl` | ~80% | Injects `--no-pager -n 200`. BERT anomaly scoring. |
| **psql** | `psql`, `pgcli` | ~88% | Strips table borders, caps at 20 rows + `[+N more]`. |
| **brew** | `brew` | ~75% | `install`/`update`: status lines + Caveats. `list`/`info`: compact. |
| **tree** | `tree` | ~70% | ≤30 lines pass through. >30: first 25 + `[... N more]` + summary. |
| **diff** | `diff` | ~75% | Keeps `+`/`-`/`@@`/header lines only (same as git diff). |
| **jq** | `jq` | ~80% | ≤20 lines pass through. Array: schema of first element + `[N items]`. |
| **env** | `env`, `printenv` | ~70% | Masks secrets (`KEY`, `TOKEN`, `PASSWORD`, …). Sorted, capped at 40. |
| **ls** | `ls` | ~80% | Dirs first, alphabetical, limit 40, `[N dirs, M files]` summary. |
| **cat** | `cat` | ~70% | ≤100 lines: pass through. 101–500: head/tail. >500: BERT importance scoring. |
| **grep / rg** | `grep`, `rg` | ~80% | Groups by file, truncates to 120 chars, caps at 50 matches. |
| **find** | `find` | ~78% | Strips common prefix, groups by directory, 5 files/dir, caps at 50. |

---

## Pipeline (Unknown Commands)

Any command without a handler goes through four stages:

1. **Strip ANSI** — removes color/cursor escape sequences
2. **Normalize whitespace** — trim trailing spaces, deduplicate consecutive identical lines, collapse multiple blanks
3. **Apply regex patterns** — per-command rules from config (`Remove` / `Collapse` / `ReplaceWith`)
4. **BERT semantic summarization** — triggered when line count > `summarize_threshold_lines` (default 200)

**BERT scoring:** Each line is scored as `1 - cosine_similarity(embedding, centroid)`. High score = outlier = informative. Lines matching error/warning patterns are hard-kept regardless of score. Falls back to head+tail if the model is unavailable.

---

## Session Intelligence

CCR tracks state across tool-use turns within a Claude Code session. The session is identified by `CCR_SESSION_ID=$PPID` (Claude Code's PID, injected by the hook script) — stable across all hook invocations in one session. State is persisted at `~/.local/share/ccr/sessions/<id>.json`.

### B3: Cross-turn output cache

When `ccr run` produces output it embeds it (384-dim BERT) and checks against recent entries with cosine similarity > 0.92. On a hit, the output is replaced with:

```
[same output as turn 4 (3m ago) — 1.2k tokens saved]
```

On a miss, the embedding and a 600-char content preview are recorded (ring buffer, max 30 entries).

### B2: Query-biased BERT summarization

The PostToolUse hook passes the command string as a query to the summarizer. Scoring becomes:

```
score = 0.5 × anomaly_score + 0.5 × cosine_similarity(line, query_embedding)
```

This surfaces lines that are both informative (outliers) and relevant to what Claude asked for.

### C1: Sentence-level cross-turn deduplication

Before emitting output, the hook runs ccr-sdk's sentence deduplicator against the 8 most recent session entries. Sentences that repeat earlier content are replaced with `[covered in turn N]`.

### C2: Session-aware compression budget

As cumulative session tokens grow, `ccr hook` tightens the line budget passed to the BERT summarizer:

| Session tokens | Compression factor | Effect |
|---------------|-------------------|--------|
| < 50k | 1.0 | No extra compression |
| 50k–100k | 1.0 → 0.5 | Budget scales linearly |
| > 100k | 0.5 | Max compression (50% of lines) |

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

Pattern actions: `Remove` (delete line), `Collapse` (count consecutive matches → `[N lines collapsed]`), `ReplaceWith = "text"`.

To add a custom handler, implement the `Handler` trait and register it in `get_handler()` in `ccr/src/handlers/mod.rs`.

---

## Analytics

Every CCR operation appends a record to `~/.local/share/ccr/analytics.jsonl`:

```json
{
  "input_tokens": 4821,  "output_tokens": 612,  "savings_pct": 87.3,
  "command": "cargo",    "subcommand": "build",
  "timestamp_secs": 1742198400,  "duration_ms": 3420
}
```

All fields added after the initial release use `#[serde(default)]` for backward compatibility with old records.

---

## Tee: Raw Output Recovery

`ccr run` saves raw output to `~/.local/share/ccr/tee/<ts>_<cmd>.log` before filtering. When savings exceed 60%, the filtered output includes a recovery hint:

```
error: mismatched types [src/main.rs:42]
[full output: ~/.local/share/ccr/tee/1742198400_cargo.log]
```

Claude can `cat` that path without re-running the command. Max 20 files kept; oldest rotated out.

| Mode | Behavior |
|------|----------|
| `aggressive` | Write only when savings > 60% (default) |
| `always` | Write on every `ccr run` |
| `never` | Disabled |

---

## CCR-SDK: Conversation Compression

The `ccr-sdk` crate compresses old turns in the conversation history — orthogonal to per-command savings, and compounding across the session (~10–20% per turn).

```
messages (oldest → newest):
  [tier 2][tier 2][tier 1][tier 1][verbatim][verbatim][verbatim]
```

| Tier | Default age | Compression |
|------|-------------|-------------|
| Verbatim | most recent 3 | unchanged |
| Tier 1 | next 5 | extractive: keep 55% of sentences |
| Tier 2 | older | generative (Ollama) or extractive 20% |

**Sentence selection:** BERT centroid scoring. Hard-kept by role:
- *User:* questions, code (backticks/`::`), snake_case identifiers, constraint language (`must`, `never`, `always`, `ensure`, …)
- *Assistant:* code, list items, numbers/dates/currency, constraint language

**Generative tier 2 (Ollama):** Prompts `mistral:instruct` to compress to ~60% word count. BERT quality gate rejects output with cosine similarity < 0.80 vs original, falling back to extractive.

**Semantic deduplication:** Sentences with cosine similarity > 0.92 to content in older turns are replaced with `[covered in turn N]`. Assistant messages never modified.

**Budget enforcement:** If `max_context_tokens` is set, a second pass compresses user then assistant messages oldest-first until under budget.

```rust
let result = Compressor::new(CompressionConfig::default()).compress(messages)?;
println!("Saved {} tokens", result.tokens_in - result.tokens_out);
```

---

## Hook Architecture

### PreToolUse

Runs before Bash executes. `ccr-rewrite.sh` calls `ccr rewrite "<cmd>"`:

- **Known handler** → prints `ccr run <cmd>`, exits 0; hook patches `tool_input.command`
- **Unknown** → exits 1; hook emits nothing; Claude Code uses original command
- **Compound commands** (`&&`, `||`, `;`) → each segment rewritten independently: `cargo build && git push` → `ccr run cargo build && ccr run git push`
- **Already wrapped** → exits 1 (no double-wrap)

Multiple PreToolUse hooks run in order — CCR merges into the existing array, preserving RTK's hook.

### PostToolUse

`ccr hook` receives output JSON after any Bash call. Pipeline:

1. Extract `tool_response.output` (or error)
2. Run 4-stage BERT pipeline with command string as query (B2 biasing)
3. Apply sentence-level cross-turn dedup via ccr-sdk (C1)
4. If session is token-heavy, apply extra BERT compression (C2)
5. Embed output, record to session cache (B3)
6. Return `{ "output": "<filtered>" }`

Never fails — returns nothing on any error so Claude Code always sees a result.

### Hook JSON contract

```json
// PreToolUse output (when rewriting):
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "permissionDecisionReason": "CCR auto-rewrite",
    "updatedInput": { "command": "ccr run git status" }
  }
}

// PostToolUse output:
{ "output": "filtered output" }
```

---

## CCR vs RTK

| Feature | CCR | RTK |
|---------|-----|-----|
| Handler count | **31 (40+ aliases)** | 40+ |
| Unknown commands | BERT routing + fallback (~40%) | Pass through (0%) |
| Handler routing | Exact → alias table → BERT similarity | Exact match only |
| Log handlers (docker/kubectl/journal) | BERT anomaly scoring (centroid distance) | Exact-match dedup |
| `cat` large files | BERT importance scoring | head+tail |
| Cross-turn output cache | Yes (cosine > 0.92, turn reference) | — |
| Query-biased summarization | Yes (anomaly + command relevance blend) | — |
| Session-aware compression | Yes (scales to 50% at 100k tokens) | — |
| Sentence-level cross-turn dedup | Yes (ccr-sdk, marks `[covered in turn N]`) | — |
| Conversation history compression | ccr-sdk: tiered + Ollama + dedup | — |
| Evaluation suite | ccr-eval (Q&A + conv fixtures) | — |
| Hooks preserved on init | Yes (merges arrays) | Overwrites |

---

## Crate Overview

```
ccr/                     CLI binary
  src/main.rs            Commands enum, init() with merge_hook()
  src/hook.rs            PostToolUse: B2 query-biased BERT, C1 sentence dedup,
                         C2 session budget, B3 cache record (JSON in → JSON out)
  src/session.rs         Per-session state: output cache, compression budget,
                         cross-turn dedup context (CCR_SESSION_ID=$PPID)
  src/cmd/               filter, run (B3 cache check), proxy, rewrite, gain, discover
  src/handlers/          31 handlers: cargo, git, curl, docker, npm, ls, read,
                         grep, find, tsc, vitest, jest, eslint, pytest, pip,
                         python, kubectl, gh, terraform, aws, make, go, maven,
                         brew, helm, journalctl, psql, tree, diff, jq, env
                         + util.rs (compact_table, test_failures, is_hard_keep,
                                    json_to_schema, cosine_similarity)

ccr-core/                Core library (no I/O)
  src/pipeline.rs        ANSI strip → normalize → patterns → BERT summarize
                         (process() accepts optional query for B2 biasing)
  src/summarizer.rs      fastembed AllMiniLML6V2, OnceCell model cache (A1),
                         anomaly scoring, summarize_with_query() for B2
  src/analytics.rs       Analytics struct (command, subcommand, duration_ms)
  src/config.rs          CcrConfig, GlobalConfig, TeeConfig, FilterAction
  src/tokens.rs          tiktoken cl100k_base

ccr-sdk/                 Conversation compression
  src/compressor.rs      Tiered compression + budget enforcement
  src/deduplicator.rs    Cross-turn semantic dedup (0.92 threshold)
  src/ollama.rs          Generative summarization + BERT quality gate

ccr-eval/                Evaluation suite
  fixtures/              .qa.toml (Q&A) + .conv.toml (conversation) test data
  src/runner.rs          Fixture execution against Claude API

config/
  default_filters.toml   Embedded default config (git, cargo, npm, docker patterns)
```
