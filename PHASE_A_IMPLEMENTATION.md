# Phase A: Provider Infrastructure (Implementation Kickoff)

**Scope:** Formalize multi-provider routing and declarative config architecture. Critical path for Phases B-F.

**Pinpoints in scope:** #245, #246, #285
**Blocked by:** Phase 0 merge (GitHub OAuth, cargo fmt, clawcode-human approval)
**Estimated effort:** 2-3 cycles
**Target:** Merge-ready immediately post-Phase 0

## #245 — Providers are hard-coded enum; no backend-swap capability

**Acceptance Criteria:**
- [ ] Providers defined as trait (not enum)
- [ ] Factory/registry pattern allows runtime provider selection
- [ ] Existing providers (Anthropic, OpenAI) are re-implemented as trait impls
- [ ] Tests pass for all existing behavior
- [ ] Zero breaking changes to public API

**Implementation sequence:**
1. Define `Provider` trait with core methods (chat completion, streaming, model listing)
2. Implement trait for existing providers
3. Add provider registry/factory
4. Update CLI to accept `--provider` flag
5. Regression tests

## #246 — Provider selection logic is CLI-parsing only; no config source integration

**Acceptance Criteria:**
- [ ] Provider selection checks: 1) CLI flag, 2) env var, 3) settings.json, 4) default
- [ ] settings.json schema includes `provider` field with subconfig
- [ ] Env vars like `OPENAI_API_KEY` trigger automatic provider selection
- [ ] Conflict resolution documented (CLI > env > config file > default)
- [ ] Config merging tested

**Implementation sequence:**
1. Extend settings.json schema (add provider field, subconfig structure)
2. Implement config-merge logic (priority order)
3. Update `claw doctor` to validate provider config (#293 prerequisite)
4. Integration tests

## #285 — No declarative provider fallback; can't swap backends mid-session

**Acceptance Criteria:**
- [ ] `settings.json` supports `providers: [primary, secondary, fallback]` array
- [ ] Streaming failures trigger automatic fallback to next provider
- [ ] Session state is preserved across provider swap
- [ ] User is notified of fallback event
- [ ] `claw doctor --providers` shows fallback chain health

**Implementation sequence:**
1. Extend settings.json schema (providers array)
2. Implement fallback logic in streaming handler
3. Add state-preservation during swap
4. User notification (log + maybe `--verbose` output)
5. Integration tests with dual-provider setup

## Dependency Graph

```
Phase 0 merge ──→ #245 (trait + registry) ──→ #246 (config integration) ──→ #285 (fallback)
                           │                        │
                           └────────────────────────┘
                                  (parallel possible)
```

## Success Criteria (Phase A complete)

- [ ] All three pinpoints (#245, #246, #285) have passing tests
- [ ] `claw --provider openai` works
- [ ] `claw --provider openai --fallback anthropic` works
- [ ] settings.json with `{ "provider": "openai", ... }` is read correctly
- [ ] `claw doctor --providers` validates all configured backends
- [ ] Zero regression on existing Anthropic-only workflows
- [ ] PR merges with zero cargo fmt warnings
- [ ] clawcode-human approval granted

## Next: Phase B (transport-layer + resilience)

Once Phase A merges, Phase B begins with auto-compaction (#287, #288, #289) and streaming resilience (#223, #225, #229, #230, #232, #283, #287, #288, #289, #290, #291, #292).
