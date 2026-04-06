# ROADMAP.md

# Clawable Coding Harness Roadmap

## Goal

Turn claw-code into the most **clawable** coding harness:
- no human-first terminal assumptions
- no fragile prompt injection timing
- no opaque session state
- no hidden plugin or MCP failures
- no manual babysitting for routine recovery

This roadmap assumes the primary users are **claws wired through hooks, plugins, sessions, and channel events**.

## Definition of "clawable"

A clawable harness is:
- deterministic to start
- machine-readable in state and failure modes
- recoverable without a human watching the terminal
- branch/test/worktree aware
- plugin/MCP lifecycle aware
- event-first, not log-first
- capable of autonomous next-step execution

## Current Pain Points

### 1. Session boot is fragile
- trust prompts can block TUI startup
- prompts can land in the shell instead of the coding agent
- "session exists" does not mean "session is ready"

### 2. Truth is split across layers
- tmux state
- clawhip event stream
- git/worktree state
- test state
- gateway/plugin/MCP runtime state

### 3. Events are too log-shaped
- claws currently infer too much from noisy text
- important states are not normalized into machine-readable events

### 4. Recovery loops are too manual
- restart worker
- accept trust prompt
- re-inject prompt
- detect stale branch
- retry failed startup
- classify infra vs code failures manually

### 5. Branch freshness is not enforced enough
- side branches can miss already-landed main fixes
- broad test failures can be stale-branch noise instead of real regressions

### 6. Plugin/MCP failures are under-classified
- startup failures, handshake failures, config errors, partial startup, and degraded mode are not exposed cleanly enough

### 7. Human UX still leaks into claw workflows
- too much depends on terminal/TUI behavior instead of explicit agent state transitions and control APIs

## Product Principles

1. **State machine first** — every worker has explicit lifecycle states.
2. **Events over scraped prose** — channel output should be derived from typed events.
3. **Recovery before escalation** — known failure modes should auto-heal once before asking for help.
4. **Branch freshness before blame** — detect stale branches before treating red tests as new regressions.
5. **Partial success is first-class** — e.g. MCP startup can succeed for some servers and fail for others, with structured degraded-mode reporting.
6. **Terminal is transport, not truth** — tmux/TUI may remain implementation details, but orchestration state must live above them.
7. **Policy is executable** — merge, retry, rebase, stale cleanup, and escalation rules should be machine-enforced.

## Roadmap

## Phase 1 — Reliable Worker Boot

### 1. Ready-handshake lifecycle for coding workers
Add explicit states:
- `spawning`
- `trust_required`
- `ready_for_prompt`
- `prompt_accepted`
- `running`
- `blocked`
- `finished`
- `failed`

Acceptance:
- prompts are never sent before `ready_for_prompt`
- trust prompt state is detectable and emitted
- shell misdelivery becomes detectable as a first-class failure state

### 2. Trust prompt resolver — done
Add allowlisted auto-trust behavior for known repos/worktrees.

Acceptance:
- trusted repos auto-clear trust prompts
- events emitted for `trust_required` and `trust_resolved`
- non-allowlisted repos remain gated
- config-backed `trust.allowlist` / `trust.denylist` accumulate across discovered `.claw/settings*.json` entries with per-file relative path resolution

### 3. Structured session control API
Provide machine control above tmux:
- create worker
- await ready
- send task
- fetch state
- fetch last error
- restart worker
- terminate worker

Acceptance:
- a claw can operate a coding worker without raw send-keys as the primary control plane

## Phase 2 — Event-Native Clawhip Integration

### 4. Canonical lane event schema
Define typed events such as:
- `lane.started`
- `lane.ready`
- `lane.prompt_misdelivery`
- `lane.blocked`
- `lane.red`
- `lane.green`
- `lane.commit.created`
- `lane.pr.opened`
- `lane.merge.ready`
- `lane.finished`
- `lane.failed`
- `branch.stale_against_main`

Acceptance:
- clawhip consumes typed lane events
- Discord summaries are rendered from structured events instead of pane scraping alone

### 5. Failure taxonomy — **done**
Normalize failure classes:
- `prompt_delivery`
- `trust_gate`
- `branch_divergence`
- `compile`
- `test`
- `plugin_startup`
- `mcp_startup`
- `mcp_handshake`
- `gateway_routing`
- `tool_runtime`
- `infra`

Acceptance:
- blockers are machine-classified
- dashboards and retry policies can branch on failure type
- **done**: the canonical claw-level taxonomy now exists as `FailureClass`/`LaneFailureClass`, API errors map into it via `to_failure_class()`, doctor JSON checks expose `failure_class` where classification is meaningful, and session `turn_failed` traces retain the machine-readable class.

### 6. Actionable summary compression
Collapse noisy event streams into:
- current phase
- last successful checkpoint
- current blocker
- recommended next recovery action

Acceptance:
- channel status updates stay short and machine-grounded
- claws stop inferring state from raw build spam

## Phase 3 — Branch/Test Awareness and Auto-Recovery

### 7. Stale-branch detection before broad verification — **done**
Before broad test runs, compare current branch to `main` and detect if known fixes are missing.

Acceptance:
- emit `branch.stale_against_main`
- suggest or auto-run rebase/merge-forward according to policy
- avoid misclassifying stale-branch failures as new regressions
- **done**: reusable `resolve_main_ref`/`current_branch` helpers in `runtime::stale_branch`, broad-test preflight in `tools/src/lib.rs` delegates to runtime helpers, `claw status --output-format json` and `claw doctor --output-format json` expose `stale_against_main` and `missing_commits_count` on the workspace surface, text status/doctor output includes stale info, and doctor workspace check uses warning level for stale branches.

### 8. Recovery recipes for common failures — **done**
Encode known automatic recoveries for:
- trust prompt unresolved
- prompt delivered to shell
- stale branch
- compile red after cross-crate refactor
- MCP startup handshake failure
- partial plugin startup

Acceptance:
- one automatic recovery attempt occurs before escalation — **done**: `attempt_recovery()` enforces `max_attempts` then escalates
- the attempted recovery is itself emitted as structured event data — **done**: `RecoveryEvent::RecoveryAttempted` converts to canonical `lane.recovery_attempted` `LaneEvent` with scenario/recipe/result JSON payload; cross-module bridges from `BranchFreshness`, `McpDiscoveryFailure`, and `PluginState` into `FailureScenario` → `LaneFailureClass` → lane event

### 9. Green-ness contract
Workers should distinguish:
- targeted tests green
- package green
- workspace green
- merge-ready green

Acceptance:
- no more ambiguous "tests passed" messaging
- merge policy can require the correct green level for the lane type

## Phase 4 — Claws-First Task Execution

### 10. Typed task packet format — **done**
Define a structured task packet with fields like:
- objective
- scope
- repo/worktree
- branch policy
- acceptance tests
- commit policy
- reporting contract
- escalation policy

Acceptance:
- claws can dispatch work without relying on long natural-language prompt blobs alone
- task packets can be logged, retried, and transformed safely
- **Done:** `TaskPacket` now uses typed serde enums plus optional `repo`/`worktree` paths in `rust/crates/runtime/src/task_packet.rs`, `RunTaskPacket` exposes the typed schema, and `claw task create --from-json <path|->` / `claw task validate <path|->` provide direct local text/JSON packet handling.

### 11. Policy engine for autonomous coding
Encode automation rules such as:
- if green + scoped diff + review passed -> merge to dev
- if stale branch -> merge-forward before broad tests
- if startup blocked -> recover once, then escalate
- if lane completed -> emit closeout and cleanup session

Acceptance:
- doctrine moves from chat instructions into executable rules

### 12. Claw-native dashboards / lane board
Expose a machine-readable board of:
- repos
- active claws
- worktrees
- branch freshness
- red/green state
- current blocker
- merge readiness
- last meaningful event

Acceptance:
- claws can query status directly
- human-facing views become a rendering layer, not the source of truth

## Phase 5 — Plugin and MCP Lifecycle Maturity

### 13. First-class plugin/MCP lifecycle contract
Each plugin/MCP integration should expose:
- config validation contract
- startup healthcheck
- discovery result
- degraded-mode behavior
- shutdown/cleanup contract

Acceptance:
- partial-startup and per-server failures are reported structurally
- successful servers remain usable even when one server fails

### 14. MCP end-to-end lifecycle parity
Close gaps from:
- config load
- server registration
- spawn/connect
- initialize handshake
- tool/resource discovery
- invocation path
- error surfacing
- shutdown/cleanup

Acceptance:
- parity harness and runtime tests cover healthy and degraded startup cases
- broken servers are surfaced as structured failures, not opaque warnings

## Immediate Backlog (from current real pain)

Priority order: P0 = blocks CI/green state, P1 = blocks integration wiring, P2 = clawability hardening, P3 = swarm-efficiency improvements.

**P0 — Fix first (CI reliability)**
1. Isolate `render_diff_report` tests into tmpdir — **done**: `render_diff_report_for()` tests run in temp git repos instead of the live working tree, and targeted `cargo test -p rusty-claude-cli render_diff_report -- --nocapture` now stays green during branch/worktree activity
2. Expand GitHub CI from single-crate coverage to workspace-grade verification — **done**: `.github/workflows/rust-ci.yml` now runs `cargo test --workspace` plus fmt/clippy at the workspace level
3. Add release-grade binary workflow — **done**: `.github/workflows/release.yml` now builds tagged Rust release artifacts for the CLI
4. Add container-first test/run docs — **done**: `Containerfile` + `docs/container.md` document the canonical Docker/Podman workflow for build, bind-mount, and `cargo test --workspace` usage
5. Surface `doctor` / preflight diagnostics in onboarding docs and help — **done**: README + USAGE now put `claw doctor` / `/doctor` in the first-run path and point at the built-in preflight report
6. Automate branding/source-of-truth residue checks in CI — **done**: `.github/scripts/check_doc_source_of_truth.py` and the `doc-source-of-truth` CI job now block stale repo/org/invite residue in tracked docs and metadata
7. Eliminate warning spam from first-run help/build path — **done**: current `cargo run -q -p rusty-claude-cli -- --help` renders clean help output without a warning wall before the product surface
8. Promote `doctor` from slash-only to top-level CLI entrypoint — **done**: `claw doctor` is now a local shell entrypoint with regression coverage for direct help and health-report output
9. Make machine-readable status commands actually machine-readable — **done**: `claw --output-format json status` and `claw --output-format json sandbox` now emit structured JSON snapshots instead of prose tables
10. Unify legacy config/skill namespaces in user-facing output — **done**: skills/help JSON/text output now present `.claw` as the canonical namespace and collapse legacy roots behind `.claw`-shaped source ids/labels
11. Honor JSON output on inventory commands like `skills` and `mcp` — **done**: direct CLI inventory commands now honor `--output-format json` with structured payloads for both skills and MCP inventory
12. Audit `--output-format` contract across the whole CLI surface — **done**: direct CLI commands now honor deterministic JSON/text handling across help/version/status/sandbox/agents/mcp/skills/bootstrap-plan/system-prompt/init/doctor, with regression coverage in `output_format_contract.rs` and resumed `/status` JSON coverage

**P1 — Next (integration wiring, unblocks verification)**
2. Add cross-module integration tests — **done**: 12 integration tests covering worker→recovery→policy, stale_branch→policy, green_contract→policy, reconciliation flows
3. Wire lane-completion emitter — **done**: `lane_completion` module with `detect_lane_completion()` auto-sets `LaneContext::completed` from session-finished + tests-green + push-complete → policy closeout
4. Wire `SummaryCompressor` into the lane event pipeline — **done**: `compress_summary_text()` feeds into `LaneEvent::Finished` detail field in `tools/src/lib.rs`

**P2 — Clawability hardening (original backlog)**
5. Worker readiness handshake + trust resolution — **done**: `WorkerStatus` state machine with `Spawning` → `TrustRequired` → `ReadyForPrompt` → `PromptAccepted` → `Running` lifecycle, `trust_auto_resolve` + `trust_gate_cleared` gating
6. Prompt misdelivery detection and recovery — **done**: `prompt_delivery_attempts` counter, `PromptMisdelivery` event detection, `auto_recover_prompt_misdelivery` + `replay_prompt` recovery arm
7. Canonical lane event schema in clawhip — **done**: `runtime::lane_events` now owns the canonical typed lane event schema and serde wire names for `lane.started`, `lane.ready`, `lane.prompt_misdelivery`, `lane.blocked`, `lane.red`, `lane.green`, `lane.commit.created`, `lane.pr.opened`, `lane.merge.ready`, `lane.finished`, `lane.failed`, and `branch.stale_against_main`; `tools/src/lib.rs` emits typed manifest events for stale-branch, commit, finish, and high-confidence green/red/PR/merge-ready signals, and `worker_boot` exposes a narrow worker→lane bridge for ready + prompt-misdelivery lifecycle events with round-trip serialization coverage.
8. Failure taxonomy + blocker normalization — **done**: `WorkerFailureKind` enum (`TrustGate/PromptDelivery/Protocol/Provider`), `FailureScenario::from_worker_failure_kind()` bridge to recovery recipes
9. Stale-branch detection before workspace tests — **done**: `stale_branch.rs` module with freshness detection, behind/ahead metrics, policy integration
10. MCP structured degraded-startup reporting — **done**: `McpManager` degraded-startup reporting (+183 lines in `mcp_stdio.rs`), failed server classification (startup/handshake/config/partial), structured `failed_servers` + `recovery_recommendations` in tool output
11. Structured task packet format — **done**: `task_packet.rs` now uses typed serde enums (`scope`, `branch_policy`, `commit_policy`, `reporting_contract`, `escalation_policy`) plus optional `repo`/`worktree` paths, validation rejects blank objectives/tests, `tools/src/lib.rs` advertises the typed `RunTaskPacket` schema, and `claw task create|validate` exposes the packet format directly for local automation.
12. Lane board / machine-readable status API — **done**: Lane completion hardening + `LaneContext::completed` auto-detection + MCP degraded reporting surface machine-readable state
13. **Session completion failure classification** — **done**: `WorkerFailureKind::Provider` + `observe_completion()` + recovery recipe bridge landed
14. **Config merge validation gap** — **done**: `config.rs` hook validation before deep-merge (+56 lines), malformed entries fail with source-path context instead of merged parse errors
15. **MCP manager discovery flaky test** — **done**: `manager_discovery_report_keeps_healthy_servers_when_one_server_fails` now runs as a normal workspace test again after repeated stable passes, so degraded-startup coverage is no longer hidden behind `#[ignore]`

16. **Commit provenance / worktree-aware push events** — **done**: `LaneCommitProvenance` now carries branch/worktree/canonical-commit/supersession metadata in lane events, and `dedupe_superseded_commit_events()` is applied before agent manifests are written so superseded commit events collapse to the latest canonical lineage
17. **Orphaned module integration audit** — **done**: `runtime` now keeps `session_control` and `trust_resolver` behind `#[cfg(test)]` until they are wired into a real non-test execution path, so normal builds no longer advertise dead clawability surface area.
18. **Context-window preflight gap** — **done**: provider request sizing now emits `context_window_blocked` before oversized requests leave the process, using a model-context registry instead of the old naive max-token heuristic.
19. **Subcommand help falls through into runtime/API path** — **done**: `claw doctor --help`, `claw status --help`, `claw sandbox --help`, and nested `mcp`/`skills` help are now intercepted locally without runtime/provider startup, with regression tests covering the direct CLI paths.
20. **Session state classification gap (working vs blocked vs finished vs truly stale)** — **done**: agent manifests now derive machine states such as `working`, `blocked_background_job`, `blocked_merge_conflict`, `degraded_mcp`, `interrupted_transport`, `finished_pending_report`, and `finished_cleanable`, and terminal-state persistence records commit provenance plus derived state so downstream monitoring can distinguish quiet progress from truly idle sessions.
21. **Resumed `/status` JSON parity gap** — dogfooding shows fresh `claw status --output-format json` now emits structured JSON, but resumed slash-command status still leaks through a text-shaped path in at least one dispatch path. Local CI-equivalent repro fails `rust/crates/rusty-claude-cli/tests/resume_slash_commands.rs::resumed_status_command_emits_structured_json_when_requested` with `expected value at line 1 column 1`, so resumed automation can receive text where JSON was explicitly requested. **Action:** unify fresh vs resumed `/status` rendering through one output-format contract and add regression coverage so resumed JSON output is guaranteed valid.
22. **Opaque failure surface for session/runtime crashes** — repeated dogfood-facing failures can currently collapse to generic wrappers like `Something went wrong while processing your request. Please try again, or use /new to start a fresh session.` without exposing whether the fault was provider auth, session corruption, slash-command dispatch, render failure, or transport/runtime panic. This blocks fast self-recovery and turns actionable clawability bugs into blind retries. **Action:** preserve a short user-safe failure class (`provider_auth`, `session_load`, `command_dispatch`, `render`, `runtime_panic`, etc.), attach a local trace/session id, and ensure operators can jump from the chat-visible error to the exact failure log quickly.
23. **`doctor --output-format json` check-level structure gap** — **done**: `claw doctor --output-format json` now keeps the human-readable `message`/`report` while also emitting structured per-check diagnostics (`name`, `status`, `summary`, `details`, plus typed fields like workspace paths and sandbox fallback data), with regression coverage in `output_format_contract.rs`.
24. **Plugin lifecycle init/shutdown test flakes under workspace-parallel execution** — dogfooding surfaced that `build_runtime_runs_plugin_lifecycle_init_and_shutdown` can fail under `cargo test --workspace` while passing in isolation because sibling tests race on tempdir-backed shell init script paths. This is test brittleness rather than a code-path regression, but it still destabilizes CI confidence and wastes diagnosis cycles. **Action:** isolate temp resources per test robustly (unique dirs + no shared cwd assumptions), audit cleanup timing, and add a regression guard so the plugin lifecycle test remains stable under parallel workspace execution.
26. **Resumed local-command JSON parity gap** — **done**: direct `claw --output-format json` already had structured renderers for `sandbox`, `mcp`, `skills`, `version`, and `init`, but resumed `claw --output-format json --resume <session> /…` paths still fell back to prose because resumed slash dispatch only emitted JSON for `/status`. Resumed `/sandbox`, `/mcp`, `/skills`, `/version`, and `/init` now reuse the same JSON envelopes as their direct CLI counterparts, with regression coverage in `rust/crates/rusty-claude-cli/tests/resume_slash_commands.rs` and `rust/crates/rusty-claude-cli/tests/output_format_contract.rs`.
**P3 — Swarm efficiency**
13. Swarm branch-lock protocol — **done**: `branch_lock::detect_branch_lock_collisions()` now detects same-branch/same-scope and nested-module collisions before parallel lanes drift into duplicate implementation
14. Commit provenance / worktree-aware push events — **done**: lane event provenance now includes branch/worktree/superseded/canonical lineage metadata, and manifest persistence de-dupes superseded commit events before downstream consumers render them

## Suggested Session Split

### Session A — worker boot protocol
Focus:
- trust prompt detection
- ready-for-prompt handshake
- prompt misdelivery detection

### Session B — clawhip lane events
Focus:
- canonical lane event schema
- failure taxonomy
- summary compression

### Session C — branch/test intelligence
Focus:
- stale-branch detection
- green-level contract
- recovery recipes

### Session D — MCP lifecycle hardening
Focus:
- startup/handshake reliability
- structured failed server reporting
- degraded-mode runtime behavior
- lifecycle tests/harness coverage

### Session E — typed task packets + policy engine
Focus:
- structured task format
- retry/merge/escalation rules
- autonomous lane closure behavior

## MVP Success Criteria

We should consider claw-code materially more clawable when:
- a claw can start a worker and know with certainty when it is ready
- claws no longer accidentally type tasks into the shell
- stale-branch failures are identified before they waste debugging time
- clawhip reports machine states, not just tmux prose
- MCP/plugin startup failures are classified and surfaced cleanly
- a coding lane can self-recover from common startup and branch issues without human babysitting

## Short Version

claw-code should evolve from:
- a CLI a human can also drive

to:
- a **claw-native execution runtime**
- an **event-native orchestration substrate**
- a **plugin/hook-first autonomous coding harness**

## #27 — Active lane/session visibility gap
**Status:** Backlog
**Pinpoint:** To answer "what's active/blocked/cleanable", operator must scrape tmux + enumerate worktrees + inspect ROADMAP text. No single machine-readable lane board exists.
**Action:** Expose one machine-readable lane inventory endpoint/command:
  `claw lanes` → JSON array of `{ session_id, repo, worktree_path, branch, phase: exploring|planning|implementing|verifying|blocked, last_event_ms, blocker: string|null }`
**Acceptance:** `claw lanes --output-format json` returns current lane state in <100ms without requiring tmux or worktree enumeration.

## #28 — Workspace test discoverability gap
**Status:** Backlog
**Pinpoint:** `cargo test --workspace --manifest-path rust/Cargo.toml` silently skips the repo-root `tests/` integration suite, making "workspace green" ambiguous. Operators running from wrong directory see 0 tests pass and think it's green.
**Action:** Define one canonical repo-level verification entrypoint (e.g., `just test` or `scripts/verify.sh`) that:
  1. Runs from repo root
  2. Includes integration suite
  3. Fails loud if integration tests are missing or skipped
**Acceptance:** Running the canonical command from any directory produces identical results; missing integration suite causes non-zero exit.

## #29 — Orchestrator session longevity gap
**Status:** Backlog
**Pinpoint:** Long-running orchestration sessions (e.g., multi-hour UltraClaw batch coordination) accumulate context until exec becomes unusable — outputs get compacted before they can be read, making local verification impossible. Subagent spawning becomes the only viable workaround.
**Action:** Add orchestrator session affordances:
  1. Context pressure warning at ~70% capacity: "context filling, consider compacting"
  2. Auto-shedding of old tool outputs (keep last N per tool) before hitting the wall
  3. `claw compact --session <id>` to compact a running session's history without restarting
**Acceptance:** An orchestrator session running 4+ hours of parallel session coordination can still run `git log` and `cargo test` without output being compacted away.

## #30 — `claw lanes` real-time state (stub → live)
**Status:** Backlog
**Pinpoint:** `claw lanes` currently returns a hardcoded empty stub. During a real UltraClaw batch, there is no CLI surface to inspect which sessions are running, which are idle, which have a blocker, or what their last event was — forcing operators to either poll the Agentika topic or parse raw JSONL session files manually.
**Action:** Wire `claw lanes --output-format json` to read live session state:
  1. Enumerate active sessions from session JSONL files (or running opencode server API)
  2. Return `{ kind: "lanes", lanes: [{ session_id, repo, worktree_path, branch, phase, last_event_ms, blocker }] }` per lane
  3. Respond in <100ms without tmux/worktree scraping
**Acceptance:** `claw lanes --output-format json` during a 4-session batch returns one entry per session with correct phase and a non-null `last_event_ms`.

## #31 — Lane spawn wrapper hangs on second tmux create
**Status:** Done — `a3c643b` (`claw new <branch>` with 30s timeout, stale-branch attach-instead, REPL dispatch `50afdf9`)
**Pinpoint:** When launching multiple `claw-code-*` work lanes sequentially, the lane-spawn wrapper hangs after the first `tmux new-session` completes — the second invocation blocks indefinitely with no timeout, no error output, and no clear indication of what's stuck. Operators must kill the wrapper process manually and issue the second lane create separately.
**Observed:** 2026-04-06, reproducible on back-to-back lane spawns against existing worktrees.
**Action:**
  1. Add a spawn timeout (e.g., 30s) to each tmux/worktree create step with a non-zero exit and clear error message on expiry
  2. Add a pre-flight check: if the target branch/worktree already exists, skip creation and attach directly rather than erroring silently
  3. Log each lane-spawn step to stderr in real time so operators can see where it stalled
**Acceptance:** Spawning 3 lanes in sequence completes without manual intervention; stale-branch collision prints an actionable message instead of hanging.

## #32 — No pre-flight branch audit surface
**Status:** Backlog
**Pinpoint:** There is no `claw` command to distinguish "this branch has unique commits not on main" from "this branch is stale pre-merge history". During lane cleanup, operators must run `git log branch ^main --oneline` per branch manually. With 10+ active lanes this becomes a real source of data loss — actual unmerged work gets mistakenly pruned alongside dead branches.
**Observed:** 2026-04-06, during post-batch worktree reconciliation.
**Action:**
  1. Add `claw branches --status` that outputs per-branch: `{ branch, commits_ahead, last_commit_ms, merged_into_main: bool }`
  2. Flag branches with `commits_ahead > 0 && !merged_into_main` as "live unmerged" in both text and JSON output
  3. Optionally surface this in `claw lanes` so each lane entry includes `branch_status`
**Acceptance:** `claw branches --status --output-format json` completes in <500ms and correctly identifies which branches carry unmerged code vs which are purely historical.

## #33 — `/omc` slash command not registered outside REPL
**Status:** Backlog
**Pinpoint:** `claw /omc` exits with `unknown slash command outside the REPL: /omc`. The OMC interop path exists in the provider/tools layer but the CLI dispatch table has no `/omc` entry for non-REPL invocation.
**Observed:** 2026-04-06, direct dogfooding by gaebal-gajae.
**Action:**
  1. Find where slash commands are registered for CLI (non-REPL) mode in `main.rs`
  2. Add `/omc` entry that routes to the OMC interop handler
  3. Add test: `claw /omc --help` exits 0 and prints usage
**Acceptance:** `claw /omc <args>` works outside REPL without "unknown slash command" error.

## #34 — Real token count in context-window preflight
**Status:** Done — `be561bf` (Claude-family uses real Anthropic count_tokens API; non-Anthropic keeps explicit heuristic label)
**Pinpoint:** Preflight context-window check uses `serialized_json_bytes / 4 + 1` heuristic. The wording now says "estimate (heuristic)" (`c1883d0f`) but the count is still approximate. Operators hitting context limits get misleading numbers.
**API path:** `POST /v1/messages/count_tokens` + `anthropic-beta: token-counting-2024-11-01` (Anthropic). xAI/OpenAI-compat: no equivalent — heuristic stays for those providers.
**Tradeoffs:** Adds one network round-trip to preflight (~100–300ms). Fails offline. Consider: run exact count only when `--verbose` or when request is within 10% of limit; keep heuristic as fast path.
**Action:**
  1. Add `count_tokens(messages: &[Message]) -> Result<u32>` to `api` crate using the beta endpoint
  2. In preflight, call it when provider is Anthropic and estimated total > 80% of context window
  3. Surface exact count in error: `Input 352,102 tokens (exact)` vs `~352,332 tokens (heuristic)`
  4. For xAI/other providers, keep heuristic with explicit label
**Acceptance:** Anthropic context-window error shows exact token count; non-Anthropic shows explicit heuristic label.

## #35 — Session idle-without-progress not surfaced to operator
**Status:** Backlog
**Pinpoint:** A session can spin for 30+ minutes with 0 code changes and never signal that it is stuck. `claw lanes` shows `phase: running` based on last event timestamp, but has no concept of "running but not making progress." The context pressure warning (#29) fires on token pressure, not idle loops.
**Observed:** 2026-04-06, `ses_29dc4e62` ran ROADMAP #31 for 30min, 0 additions, had to be killed manually.
**Action:**
  1. Add `idle_without_progress_ms` to lane state: time since last file change or tool-use event
  2. If `idle_without_progress_ms > 10min`, surface `phase: stalled` in `claw lanes --output-format json`
  3. Optionally emit a `LaneEvent::StallDetected { idle_ms }` into the session JSONL
  4. CLI should print `WARN: session <id> has been idle for Xm — consider inspecting or restarting`
**Acceptance:** After 10min idle with no file edits, `claw lanes` shows `phase: stalled` and CLI emits a warning.

## #36 — No native push/delegation primitive for read-only agents
**Status:** Backlog
**Pinpoint:** Agents with local commit access but HTTP 403 on push have no `claw` primitive to hand off work upstream. Current workaround: git bundle + scp over Tailscale + manual relay by a privileged operator. This adds 5–30min latency to every dogfood cycle and creates merge debt accumulation.
**Observed:** 2026-04-06, Jobdori accumulated 15 local commits over ~3 hours that could not reach upstream without manual relay.
**Action:**
  1. Add `claw push-request` command: packages local commits not on `origin/main` as a bundle + opens a PR or posts the bundle URL to a configured channel
  2. OR: Add a `claw relay --to <agent-id>` that sends the local bundle to a privileged agent via Agentika topic for auto-push
  3. The receiving agent validates (build + test) then pushes
**Acceptance:** A read-only agent can run `claw push-request` and have commits reach upstream within 5 minutes without human intervention.

## #37 — CLI-REPL command surface parity has no compile-time enforcement
**Status:** Backlog
**Pinpoint:** CLI parse (`parse_args` match arm) and REPL dispatch (`try_parse_repl_subcommand` match arm) are two independent match blocks with no shared command registry. Adding a new subcommand requires manual edits to both, plus the help topic table, plus the `bare_slash_command_guidance` exclusion list, plus the main `print_help_to` listing — 5 separate sites, zero compile-time link between them. `claw new` proved this: it was wired into 3 of 5 sites on first commit, needed 3 follow-up commits to close the gaps.
**Observed:** 2026-04-06, `claw new` was missing from REPL dispatch (`50afdf9`), main help listing (`01ddc26`), and had no parity test.
**Action:**
  1. Extract a shared `SubcommandSpec` enum or registry that both CLI parse and REPL dispatch read from
  2. Add a compile-time or test-time assertion: every `SubcommandSpec` variant must appear in parse, REPL dispatch, help listing, and bare-command guidance
  3. Alternatively: a single `#[test]` that iterates known subcommands and asserts each one parses successfully in both `parse_args` and `try_parse_repl_subcommand`
**Acceptance:** Adding a new subcommand that compiles but is missing from REPL dispatch causes a test failure.
