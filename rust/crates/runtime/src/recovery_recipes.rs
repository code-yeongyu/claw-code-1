#![allow(clippy::cast_possible_truncation, clippy::uninlined_format_args)]
//! Recovery recipes for common failure scenarios.
//!
//! Encodes known automatic recoveries for the six failure scenarios
//! listed in ROADMAP item 8, and enforces one automatic recovery
//! attempt before escalation. Each attempt is emitted as a structured
//! recovery event.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::lane_events::{LaneEvent, LaneFailureClass};
use crate::mcp_lifecycle_hardened::McpLifecyclePhase;
use crate::mcp_stdio::McpDiscoveryFailure;
use crate::plugin_lifecycle::PluginState;
use crate::stale_branch::BranchFreshness;
use crate::worker_boot::WorkerFailureKind;

/// The six failure scenarios that have known recovery recipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureScenario {
    TrustPromptUnresolved,
    PromptMisdelivery,
    StaleBranch,
    CompileRedCrossCrate,
    McpHandshakeFailure,
    PartialPluginStartup,
    ProviderFailure,
}

impl FailureScenario {
    /// Returns all known failure scenarios.
    #[must_use]
    pub fn all() -> &'static [FailureScenario] {
        &[
            Self::TrustPromptUnresolved,
            Self::PromptMisdelivery,
            Self::StaleBranch,
            Self::CompileRedCrossCrate,
            Self::McpHandshakeFailure,
            Self::PartialPluginStartup,
            Self::ProviderFailure,
        ]
    }

    /// Map a `WorkerFailureKind` to the corresponding `FailureScenario`.
    /// This is the bridge that lets recovery policy consume worker boot events.
    #[must_use]
    pub fn from_worker_failure_kind(kind: WorkerFailureKind) -> Self {
        match kind {
            WorkerFailureKind::TrustGate => Self::TrustPromptUnresolved,
            WorkerFailureKind::PromptDelivery => Self::PromptMisdelivery,
            WorkerFailureKind::Protocol => Self::McpHandshakeFailure,
            WorkerFailureKind::Provider => Self::ProviderFailure,
        }
    }

    /// Derive a `FailureScenario` from branch freshness.
    /// Returns `None` when the branch is fresh.
    #[must_use]
    pub fn from_branch_freshness(freshness: &BranchFreshness) -> Option<Self> {
        match freshness {
            BranchFreshness::Fresh => None,
            BranchFreshness::Stale { .. } | BranchFreshness::Diverged { .. } => {
                Some(Self::StaleBranch)
            }
        }
    }

    /// Derive a `FailureScenario` from an MCP discovery failure.
    #[must_use]
    pub fn from_mcp_discovery_failure(failure: &McpDiscoveryFailure) -> Self {
        match failure.phase {
            McpLifecyclePhase::InitializeHandshake => Self::McpHandshakeFailure,
            _ => Self::McpHandshakeFailure,
        }
    }

    /// Derive a `FailureScenario` from plugin state.
    /// Returns `Some` for `Degraded` and `Failed` states.
    #[must_use]
    pub fn from_plugin_state(state: &PluginState) -> Option<Self> {
        match state {
            PluginState::Degraded { .. } | PluginState::Failed { .. } => {
                Some(Self::PartialPluginStartup)
            }
            _ => None,
        }
    }

    /// Map this scenario to the canonical `LaneFailureClass` for lane events.
    #[must_use]
    pub fn to_lane_failure_class(self) -> LaneFailureClass {
        match self {
            Self::TrustPromptUnresolved => LaneFailureClass::TrustGate,
            Self::PromptMisdelivery => LaneFailureClass::PromptDelivery,
            Self::StaleBranch => LaneFailureClass::BranchDivergence,
            Self::CompileRedCrossCrate => LaneFailureClass::Compile,
            Self::McpHandshakeFailure => LaneFailureClass::McpHandshake,
            Self::PartialPluginStartup => LaneFailureClass::PluginStartup,
            Self::ProviderFailure => LaneFailureClass::GatewayRouting,
        }
    }
}

impl std::fmt::Display for FailureScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrustPromptUnresolved => write!(f, "trust_prompt_unresolved"),
            Self::PromptMisdelivery => write!(f, "prompt_misdelivery"),
            Self::StaleBranch => write!(f, "stale_branch"),
            Self::CompileRedCrossCrate => write!(f, "compile_red_cross_crate"),
            Self::McpHandshakeFailure => write!(f, "mcp_handshake_failure"),
            Self::PartialPluginStartup => write!(f, "partial_plugin_startup"),
            Self::ProviderFailure => write!(f, "provider_failure"),
        }
    }
}

/// Individual step that can be executed as part of a recovery recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStep {
    AcceptTrustPrompt,
    RedirectPromptToAgent,
    RebaseBranch,
    CleanBuild,
    RetryMcpHandshake { timeout: u64 },
    RestartPlugin { name: String },
    RestartWorker,
    EscalateToHuman { reason: String },
}

/// Policy governing what happens when automatic recovery is exhausted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationPolicy {
    AlertHuman,
    LogAndContinue,
    Abort,
}

/// A recovery recipe encodes the sequence of steps to attempt for a
/// given failure scenario, along with the maximum number of automatic
/// attempts and the escalation policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRecipe {
    pub scenario: FailureScenario,
    pub steps: Vec<RecoveryStep>,
    pub max_attempts: u32,
    pub escalation_policy: EscalationPolicy,
}

/// Outcome of a recovery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryResult {
    Recovered {
        steps_taken: u32,
    },
    PartialRecovery {
        recovered: Vec<RecoveryStep>,
        remaining: Vec<RecoveryStep>,
    },
    EscalationRequired {
        reason: String,
    },
}

/// Structured event emitted during recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryEvent {
    RecoveryAttempted {
        scenario: FailureScenario,
        recipe: RecoveryRecipe,
        result: RecoveryResult,
    },
    RecoverySucceeded,
    RecoveryFailed,
    Escalated,
}

impl RecoveryEvent {
    /// Convert a `RecoveryAttempted` variant into a canonical `LaneEvent`.
    /// Returns `None` for other variants.
    #[must_use]
    pub fn into_lane_event(self, emitted_at: impl Into<String>) -> Option<LaneEvent> {
        match self {
            Self::RecoveryAttempted {
                scenario,
                recipe,
                result,
            } => {
                let failure_class = scenario.to_lane_failure_class();
                let data = serde_json::json!({
                    "scenario": scenario,
                    "recipe": recipe,
                    "result": result,
                });
                Some(LaneEvent::recovery_attempted(
                    emitted_at,
                    failure_class,
                    data,
                ))
            }
            _ => None,
        }
    }
}

/// Minimal context for tracking recovery state and emitting events.
///
/// Holds per-scenario attempt counts, a structured event log, and an
/// optional simulation knob for controlling step outcomes during tests.
#[derive(Debug, Clone, Default)]
pub struct RecoveryContext {
    attempts: HashMap<FailureScenario, u32>,
    events: Vec<RecoveryEvent>,
    /// Optional step index at which simulated execution fails.
    /// `None` means all steps succeed.
    fail_at_step: Option<usize>,
}

impl RecoveryContext {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure a step index at which simulated execution will fail.
    #[must_use]
    pub fn with_fail_at_step(mut self, index: usize) -> Self {
        self.fail_at_step = Some(index);
        self
    }

    /// Returns the structured event log populated during recovery.
    #[must_use]
    pub fn events(&self) -> &[RecoveryEvent] {
        &self.events
    }

    /// Returns the number of recovery attempts made for a scenario.
    #[must_use]
    pub fn attempt_count(&self, scenario: &FailureScenario) -> u32 {
        self.attempts.get(scenario).copied().unwrap_or(0)
    }
}

/// Returns the known recovery recipe for the given failure scenario.
#[must_use]
pub fn recipe_for(scenario: &FailureScenario) -> RecoveryRecipe {
    match scenario {
        FailureScenario::TrustPromptUnresolved => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::AcceptTrustPrompt],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::AlertHuman,
        },
        FailureScenario::PromptMisdelivery => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::RedirectPromptToAgent],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::AlertHuman,
        },
        FailureScenario::StaleBranch => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::RebaseBranch, RecoveryStep::CleanBuild],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::AlertHuman,
        },
        FailureScenario::CompileRedCrossCrate => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::CleanBuild],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::AlertHuman,
        },
        FailureScenario::McpHandshakeFailure => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::RetryMcpHandshake { timeout: 5000 }],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::Abort,
        },
        FailureScenario::PartialPluginStartup => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![
                RecoveryStep::RestartPlugin {
                    name: "stalled".to_string(),
                },
                RecoveryStep::RetryMcpHandshake { timeout: 3000 },
            ],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::LogAndContinue,
        },
        FailureScenario::ProviderFailure => RecoveryRecipe {
            scenario: *scenario,
            steps: vec![RecoveryStep::RestartWorker],
            max_attempts: 1,
            escalation_policy: EscalationPolicy::AlertHuman,
        },
    }
}

/// Attempts automatic recovery for the given failure scenario.
///
/// Looks up the recipe, enforces the one-attempt-before-escalation
/// policy, simulates step execution (controlled by the context), and
/// emits structured [`RecoveryEvent`]s for every attempt.
pub fn attempt_recovery(scenario: &FailureScenario, ctx: &mut RecoveryContext) -> RecoveryResult {
    let recipe = recipe_for(scenario);
    let attempt_count = ctx.attempts.entry(*scenario).or_insert(0);

    // Enforce one automatic recovery attempt before escalation.
    if *attempt_count >= recipe.max_attempts {
        let result = RecoveryResult::EscalationRequired {
            reason: format!(
                "max recovery attempts ({}) exceeded for {}",
                recipe.max_attempts, scenario
            ),
        };
        ctx.events.push(RecoveryEvent::RecoveryAttempted {
            scenario: *scenario,
            recipe,
            result: result.clone(),
        });
        ctx.events.push(RecoveryEvent::Escalated);
        return result;
    }

    *attempt_count += 1;

    // Execute steps, honoring the optional fail_at_step simulation.
    let fail_index = ctx.fail_at_step;
    let mut executed = Vec::new();
    let mut failed = false;

    for (i, step) in recipe.steps.iter().enumerate() {
        if fail_index == Some(i) {
            failed = true;
            break;
        }
        executed.push(step.clone());
    }

    let result = if failed {
        let remaining: Vec<RecoveryStep> = recipe.steps[executed.len()..].to_vec();
        if executed.is_empty() {
            RecoveryResult::EscalationRequired {
                reason: format!("recovery failed at first step for {}", scenario),
            }
        } else {
            RecoveryResult::PartialRecovery {
                recovered: executed,
                remaining,
            }
        }
    } else {
        RecoveryResult::Recovered {
            steps_taken: recipe.steps.len() as u32,
        }
    };

    // Emit the attempt as structured event data.
    ctx.events.push(RecoveryEvent::RecoveryAttempted {
        scenario: *scenario,
        recipe,
        result: result.clone(),
    });

    match &result {
        RecoveryResult::Recovered { .. } => {
            ctx.events.push(RecoveryEvent::RecoverySucceeded);
        }
        RecoveryResult::PartialRecovery { .. } => {
            ctx.events.push(RecoveryEvent::RecoveryFailed);
        }
        RecoveryResult::EscalationRequired { .. } => {
            ctx.events.push(RecoveryEvent::Escalated);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_scenario_has_a_matching_recipe() {
        // given
        let scenarios = FailureScenario::all();

        // when / then
        for scenario in scenarios {
            let recipe = recipe_for(scenario);
            assert_eq!(
                recipe.scenario, *scenario,
                "recipe scenario should match requested scenario"
            );
            assert!(
                !recipe.steps.is_empty(),
                "recipe for {} should have at least one step",
                scenario
            );
            assert!(
                recipe.max_attempts >= 1,
                "recipe for {} should allow at least one attempt",
                scenario
            );
        }
    }

    #[test]
    fn successful_recovery_returns_recovered_and_emits_events() {
        // given
        let mut ctx = RecoveryContext::new();
        let scenario = FailureScenario::TrustPromptUnresolved;

        // when
        let result = attempt_recovery(&scenario, &mut ctx);

        // then
        assert_eq!(result, RecoveryResult::Recovered { steps_taken: 1 });
        assert_eq!(ctx.events().len(), 2);
        assert!(matches!(
            &ctx.events()[0],
            RecoveryEvent::RecoveryAttempted {
                scenario: s,
                result: r,
                ..
            } if *s == FailureScenario::TrustPromptUnresolved
              && matches!(r, RecoveryResult::Recovered { steps_taken: 1 })
        ));
        assert_eq!(ctx.events()[1], RecoveryEvent::RecoverySucceeded);
    }

    #[test]
    fn escalation_after_max_attempts_exceeded() {
        // given
        let mut ctx = RecoveryContext::new();
        let scenario = FailureScenario::PromptMisdelivery;

        // when — first attempt succeeds
        let first = attempt_recovery(&scenario, &mut ctx);
        assert!(matches!(first, RecoveryResult::Recovered { .. }));

        // when — second attempt should escalate
        let second = attempt_recovery(&scenario, &mut ctx);

        // then
        assert!(
            matches!(
                &second,
                RecoveryResult::EscalationRequired { reason }
                    if reason.contains("max recovery attempts")
            ),
            "second attempt should require escalation, got: {second:?}"
        );
        assert_eq!(ctx.attempt_count(&scenario), 1);
        assert!(ctx
            .events()
            .iter()
            .any(|e| matches!(e, RecoveryEvent::Escalated)));
    }

    #[test]
    fn partial_recovery_when_step_fails_midway() {
        // given — PartialPluginStartup has two steps; fail at step index 1
        let mut ctx = RecoveryContext::new().with_fail_at_step(1);
        let scenario = FailureScenario::PartialPluginStartup;

        // when
        let result = attempt_recovery(&scenario, &mut ctx);

        // then
        match &result {
            RecoveryResult::PartialRecovery {
                recovered,
                remaining,
            } => {
                assert_eq!(recovered.len(), 1, "one step should have succeeded");
                assert_eq!(remaining.len(), 1, "one step should remain");
                assert!(matches!(recovered[0], RecoveryStep::RestartPlugin { .. }));
                assert!(matches!(
                    remaining[0],
                    RecoveryStep::RetryMcpHandshake { .. }
                ));
            }
            other => panic!("expected PartialRecovery, got {other:?}"),
        }
        assert!(ctx
            .events()
            .iter()
            .any(|e| matches!(e, RecoveryEvent::RecoveryFailed)));
    }

    #[test]
    fn first_step_failure_escalates_immediately() {
        // given — fail at step index 0
        let mut ctx = RecoveryContext::new().with_fail_at_step(0);
        let scenario = FailureScenario::CompileRedCrossCrate;

        // when
        let result = attempt_recovery(&scenario, &mut ctx);

        // then
        assert!(
            matches!(
                &result,
                RecoveryResult::EscalationRequired { reason }
                    if reason.contains("failed at first step")
            ),
            "zero-step failure should escalate, got: {result:?}"
        );
        assert!(ctx
            .events()
            .iter()
            .any(|e| matches!(e, RecoveryEvent::Escalated)));
    }

    #[test]
    fn emitted_events_include_structured_attempt_data() {
        // given
        let mut ctx = RecoveryContext::new();
        let scenario = FailureScenario::McpHandshakeFailure;

        // when
        let _ = attempt_recovery(&scenario, &mut ctx);

        // then — verify the RecoveryAttempted event carries full context
        let attempted = ctx
            .events()
            .iter()
            .find(|e| matches!(e, RecoveryEvent::RecoveryAttempted { .. }))
            .expect("should have emitted RecoveryAttempted event");

        match attempted {
            RecoveryEvent::RecoveryAttempted {
                scenario: s,
                recipe,
                result,
            } => {
                assert_eq!(*s, scenario);
                assert_eq!(recipe.scenario, scenario);
                assert!(!recipe.steps.is_empty());
                assert!(matches!(result, RecoveryResult::Recovered { .. }));
            }
            _ => unreachable!(),
        }

        // Verify the event is serializable as structured JSON
        let json = serde_json::to_string(&ctx.events()[0])
            .expect("recovery event should be serializable to JSON");
        assert!(
            json.contains("mcp_handshake_failure"),
            "serialized event should contain scenario name"
        );
    }

    #[test]
    fn recovery_context_tracks_attempts_per_scenario() {
        // given
        let mut ctx = RecoveryContext::new();

        // when
        assert_eq!(ctx.attempt_count(&FailureScenario::StaleBranch), 0);
        attempt_recovery(&FailureScenario::StaleBranch, &mut ctx);

        // then
        assert_eq!(ctx.attempt_count(&FailureScenario::StaleBranch), 1);
        assert_eq!(ctx.attempt_count(&FailureScenario::PromptMisdelivery), 0);
    }

    #[test]
    fn stale_branch_recipe_has_rebase_then_clean_build() {
        // given
        let recipe = recipe_for(&FailureScenario::StaleBranch);

        // then
        assert_eq!(recipe.steps.len(), 2);
        assert_eq!(recipe.steps[0], RecoveryStep::RebaseBranch);
        assert_eq!(recipe.steps[1], RecoveryStep::CleanBuild);
    }

    #[test]
    fn partial_plugin_startup_recipe_has_restart_then_handshake() {
        // given
        let recipe = recipe_for(&FailureScenario::PartialPluginStartup);

        // then
        assert_eq!(recipe.steps.len(), 2);
        assert!(matches!(
            recipe.steps[0],
            RecoveryStep::RestartPlugin { .. }
        ));
        assert!(matches!(
            recipe.steps[1],
            RecoveryStep::RetryMcpHandshake { timeout: 3000 }
        ));
        assert_eq!(recipe.escalation_policy, EscalationPolicy::LogAndContinue);
    }

    #[test]
    fn failure_scenario_display_all_variants() {
        // given
        let cases = [
            (
                FailureScenario::TrustPromptUnresolved,
                "trust_prompt_unresolved",
            ),
            (FailureScenario::PromptMisdelivery, "prompt_misdelivery"),
            (FailureScenario::StaleBranch, "stale_branch"),
            (
                FailureScenario::CompileRedCrossCrate,
                "compile_red_cross_crate",
            ),
            (
                FailureScenario::McpHandshakeFailure,
                "mcp_handshake_failure",
            ),
            (
                FailureScenario::PartialPluginStartup,
                "partial_plugin_startup",
            ),
        ];

        // when / then
        for (scenario, expected) in &cases {
            assert_eq!(scenario.to_string(), *expected);
        }
    }

    #[test]
    fn multi_step_success_reports_correct_steps_taken() {
        // given — StaleBranch has 2 steps, no simulated failure
        let mut ctx = RecoveryContext::new();
        let scenario = FailureScenario::StaleBranch;

        // when
        let result = attempt_recovery(&scenario, &mut ctx);

        // then
        assert_eq!(result, RecoveryResult::Recovered { steps_taken: 2 });
    }

    #[test]
    fn mcp_handshake_recipe_uses_abort_escalation_policy() {
        // given
        let recipe = recipe_for(&FailureScenario::McpHandshakeFailure);

        // then
        assert_eq!(recipe.escalation_policy, EscalationPolicy::Abort);
        assert_eq!(recipe.max_attempts, 1);
    }

    #[test]
    fn worker_failure_kind_maps_to_failure_scenario() {
        // given / when / then — verify the bridge is correct
        assert_eq!(
            FailureScenario::from_worker_failure_kind(WorkerFailureKind::TrustGate),
            FailureScenario::TrustPromptUnresolved,
        );
        assert_eq!(
            FailureScenario::from_worker_failure_kind(WorkerFailureKind::PromptDelivery),
            FailureScenario::PromptMisdelivery,
        );
        assert_eq!(
            FailureScenario::from_worker_failure_kind(WorkerFailureKind::Protocol),
            FailureScenario::McpHandshakeFailure,
        );
        assert_eq!(
            FailureScenario::from_worker_failure_kind(WorkerFailureKind::Provider),
            FailureScenario::ProviderFailure,
        );
    }

    #[test]
    fn provider_failure_recipe_uses_restart_worker_step() {
        // given
        let recipe = recipe_for(&FailureScenario::ProviderFailure);

        // then
        assert_eq!(recipe.scenario, FailureScenario::ProviderFailure);
        assert!(recipe.steps.contains(&RecoveryStep::RestartWorker));
        assert_eq!(recipe.escalation_policy, EscalationPolicy::AlertHuman);
        assert_eq!(recipe.max_attempts, 1);
    }

    #[test]
    fn provider_failure_recovery_attempt_succeeds_then_escalates() {
        // given
        let mut ctx = RecoveryContext::new();
        let scenario = FailureScenario::ProviderFailure;

        // when — first attempt
        let first = attempt_recovery(&scenario, &mut ctx);
        assert!(matches!(first, RecoveryResult::Recovered { .. }));

        // when — second attempt should escalate (max_attempts=1)
        let second = attempt_recovery(&scenario, &mut ctx);
        assert!(matches!(second, RecoveryResult::EscalationRequired { .. }));
        assert!(ctx
            .events()
            .iter()
            .any(|e| matches!(e, RecoveryEvent::Escalated)));
    }

    #[test]
    fn branch_freshness_stale_maps_to_stale_branch_scenario() {
        // given
        let freshness = BranchFreshness::Stale {
            commits_behind: 3,
            missing_fixes: vec!["fix-timeout".to_string()],
        };

        // when
        let scenario = FailureScenario::from_branch_freshness(&freshness);

        // then
        assert_eq!(scenario, Some(FailureScenario::StaleBranch));
    }

    #[test]
    fn branch_freshness_diverged_maps_to_stale_branch_scenario() {
        // given
        let freshness = BranchFreshness::Diverged {
            ahead: 2,
            behind: 5,
            missing_fixes: vec![],
        };

        // when
        let scenario = FailureScenario::from_branch_freshness(&freshness);

        // then
        assert_eq!(scenario, Some(FailureScenario::StaleBranch));
    }

    #[test]
    fn branch_freshness_fresh_returns_none() {
        // given
        let freshness = BranchFreshness::Fresh;

        // when
        let scenario = FailureScenario::from_branch_freshness(&freshness);

        // then
        assert_eq!(scenario, None);
    }

    #[test]
    fn mcp_discovery_failure_maps_to_mcp_handshake_scenario() {
        // given
        let failure = McpDiscoveryFailure {
            server_name: "broken-server".to_string(),
            phase: crate::mcp_lifecycle_hardened::McpLifecyclePhase::InitializeHandshake,
            error: "connection refused".to_string(),
            recoverable: true,
            context: std::collections::BTreeMap::new(),
        };

        // when
        let scenario = FailureScenario::from_mcp_discovery_failure(&failure);

        // then
        assert_eq!(scenario, FailureScenario::McpHandshakeFailure);
    }

    #[test]
    fn mcp_discovery_failure_non_handshake_phase_maps_to_mcp_handshake_scenario() {
        // given
        let failure = McpDiscoveryFailure {
            server_name: "slow-server".to_string(),
            phase: crate::mcp_lifecycle_hardened::McpLifecyclePhase::ToolDiscovery,
            error: "timeout".to_string(),
            recoverable: true,
            context: std::collections::BTreeMap::new(),
        };

        // when
        let scenario = FailureScenario::from_mcp_discovery_failure(&failure);

        // then
        assert_eq!(scenario, FailureScenario::McpHandshakeFailure);
    }

    #[test]
    fn plugin_state_degraded_maps_to_partial_plugin_startup() {
        // given
        let state = PluginState::Degraded {
            healthy_servers: vec!["alpha".to_string()],
            failed_servers: vec![],
        };

        // when
        let scenario = FailureScenario::from_plugin_state(&state);

        // then
        assert_eq!(scenario, Some(FailureScenario::PartialPluginStartup));
    }

    #[test]
    fn plugin_state_failed_maps_to_partial_plugin_startup() {
        // given
        let state = PluginState::Failed {
            reason: "all servers down".to_string(),
        };

        // when
        let scenario = FailureScenario::from_plugin_state(&state);

        // then
        assert_eq!(scenario, Some(FailureScenario::PartialPluginStartup));
    }

    #[test]
    fn plugin_state_healthy_returns_none() {
        // given / when
        let scenario = FailureScenario::from_plugin_state(&PluginState::Healthy);

        // then
        assert_eq!(scenario, None);
    }

    #[test]
    fn failure_scenario_maps_to_lane_failure_class() {
        // given / when / then
        assert_eq!(
            FailureScenario::TrustPromptUnresolved.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::TrustGate,
        );
        assert_eq!(
            FailureScenario::PromptMisdelivery.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::PromptDelivery,
        );
        assert_eq!(
            FailureScenario::StaleBranch.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::BranchDivergence,
        );
        assert_eq!(
            FailureScenario::CompileRedCrossCrate.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::Compile,
        );
        assert_eq!(
            FailureScenario::McpHandshakeFailure.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::McpHandshake,
        );
        assert_eq!(
            FailureScenario::PartialPluginStartup.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::PluginStartup,
        );
        assert_eq!(
            FailureScenario::ProviderFailure.to_lane_failure_class(),
            crate::lane_events::LaneFailureClass::GatewayRouting,
        );
    }

    #[test]
    fn recovery_attempted_converts_to_lane_event_with_structured_data() {
        // given
        let mut ctx = RecoveryContext::new();
        attempt_recovery(&FailureScenario::StaleBranch, &mut ctx);
        let event = ctx
            .events()
            .iter()
            .find(|e| matches!(e, RecoveryEvent::RecoveryAttempted { .. }))
            .expect("should have RecoveryAttempted event")
            .clone();

        // when
        let lane_event = event
            .into_lane_event("2026-05-01T00:00:00Z")
            .expect("RecoveryAttempted should produce a LaneEvent");

        // then
        assert_eq!(
            lane_event.event,
            crate::lane_events::LaneEventName::RecoveryAttempted
        );
        assert_eq!(
            lane_event.status,
            crate::lane_events::LaneEventStatus::Recovering
        );
        assert_eq!(
            lane_event.failure_class,
            Some(crate::lane_events::LaneFailureClass::BranchDivergence)
        );
        let data = lane_event.data.expect("should carry structured data");
        assert_eq!(data["scenario"], "stale_branch");
        assert!(!data["recipe"]["steps"].as_array().unwrap().is_empty());
        assert!(data["result"]["recovered"].is_object());
    }

    #[test]
    fn recovery_succeeded_does_not_convert_to_lane_event() {
        // given
        let event = RecoveryEvent::RecoverySucceeded;

        // when
        let lane_event = event.into_lane_event("2026-05-01T00:00:00Z");

        // then
        assert!(lane_event.is_none());
    }
}
