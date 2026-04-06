use serde::{Deserialize, Serialize};

use crate::{
    recipe_for, FailureScenario, LaneEvent, LaneEventBlocker, LaneEventName, LaneEventStatus,
    LaneFailureClass,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionableSummary {
    pub current_phase: String,
    pub last_successful_checkpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_blocker: Option<String>,
    pub recommended_next_action: String,
}

impl ActionableSummary {
    #[must_use]
    pub fn fallback(has_session: bool, has_workspace_changes: bool) -> Self {
        let current_phase = if has_workspace_changes {
            "implementing"
        } else {
            "done"
        };
        let recommended_next_action = if has_session {
            "resume or start a lane to populate actionable status"
        } else {
            "start a lane to populate actionable status"
        };

        Self {
            current_phase: current_phase.to_string(),
            last_successful_checkpoint: "workspace status snapshot collected".to_string(),
            current_blocker: None,
            recommended_next_action: recommended_next_action.to_string(),
        }
    }
}

#[must_use]
pub fn derive_actionable_summary(
    status: &str,
    derived_state: &str,
    subagent_type: Option<&str>,
    lane_events: &[LaneEvent],
    current_blocker: Option<&LaneEventBlocker>,
) -> ActionableSummary {
    let current_phase = determine_phase(
        status,
        derived_state,
        subagent_type,
        lane_events,
        current_blocker,
    );
    let blocker = blocker_text(current_blocker, lane_events);

    ActionableSummary {
        current_phase: current_phase.to_string(),
        last_successful_checkpoint: last_successful_checkpoint(lane_events),
        current_blocker: blocker,
        recommended_next_action: recommended_next_action(
            current_phase,
            derived_state,
            latest_failure_class(current_blocker, lane_events),
        ),
    }
}

fn determine_phase(
    status: &str,
    derived_state: &str,
    subagent_type: Option<&str>,
    lane_events: &[LaneEvent],
    current_blocker: Option<&LaneEventBlocker>,
) -> &'static str {
    let normalized_status = status.trim().to_ascii_lowercase();
    let normalized_state = derived_state.trim().to_ascii_lowercase();

    if current_blocker.is_some()
        || normalized_status == "failed"
        || matches!(
            normalized_state.as_str(),
            "blocked_background_job"
                | "blocked_merge_conflict"
                | "degraded_mcp"
                | "interrupted_transport"
                | "truly_idle"
        )
        || lane_events.iter().rev().any(|event| {
            matches!(
                event.status,
                LaneEventStatus::Blocked | LaneEventStatus::Failed
            )
        })
    {
        return "blocked";
    }

    if normalized_status == "completed"
        || normalized_status == "finished"
        || normalized_state.starts_with("finished_")
        || lane_events.iter().rev().any(|event| {
            matches!(
                event.status,
                LaneEventStatus::Completed
                    | LaneEventStatus::Reconciled
                    | LaneEventStatus::Merged
                    | LaneEventStatus::Superseded
                    | LaneEventStatus::Closed
            )
        })
    {
        return "done";
    }

    match subagent_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("explore") => "exploring",
        Some("plan") => "planning",
        Some("verification") => "verifying",
        _ => "implementing",
    }
}

fn blocker_text(
    current_blocker: Option<&LaneEventBlocker>,
    lane_events: &[LaneEvent],
) -> Option<String> {
    current_blocker
        .map(|blocker| blocker.detail.trim().to_string())
        .filter(|detail| !detail.is_empty())
        .or_else(|| {
            lane_events.iter().rev().find_map(|event| {
                matches!(
                    event.status,
                    LaneEventStatus::Blocked | LaneEventStatus::Failed
                )
                .then(|| event.detail.as_deref())
                .flatten()
                .map(str::trim)
                .filter(|detail| !detail.is_empty())
                .map(str::to_string)
            })
        })
}

fn latest_failure_class(
    current_blocker: Option<&LaneEventBlocker>,
    lane_events: &[LaneEvent],
) -> Option<LaneFailureClass> {
    current_blocker
        .map(|blocker| blocker.failure_class)
        .or_else(|| {
            lane_events
                .iter()
                .rev()
                .find_map(|event| event.failure_class)
        })
}

fn last_successful_checkpoint(lane_events: &[LaneEvent]) -> String {
    lane_events
        .iter()
        .rev()
        .find_map(successful_checkpoint_label)
        .unwrap_or_else(|| "no successful checkpoint recorded yet".to_string())
}

fn successful_checkpoint_label(event: &LaneEvent) -> Option<String> {
    match event.status {
        LaneEventStatus::Blocked
        | LaneEventStatus::Failed
        | LaneEventStatus::Red
        | LaneEventStatus::Superseded => return None,
        _ => {}
    }

    let detail = event
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    match event.event {
        LaneEventName::Started => detail.or_else(|| Some("lane started".to_string())),
        LaneEventName::Ready => detail.or_else(|| Some("lane ready".to_string())),
        LaneEventName::Green => detail.or_else(|| Some("verification passed".to_string())),
        LaneEventName::CommitCreated => detail.or_else(|| commit_checkpoint_label(event)),
        LaneEventName::PrOpened => detail.or_else(|| Some("pull request opened".to_string())),
        LaneEventName::MergeReady => {
            detail.or_else(|| Some("merge-ready state reached".to_string()))
        }
        LaneEventName::Finished => detail.or_else(|| Some("lane finished".to_string())),
        LaneEventName::Reconciled => detail.or_else(|| Some("lane reconciled".to_string())),
        LaneEventName::Merged => detail.or_else(|| Some("lane merged".to_string())),
        LaneEventName::Closed => detail.or_else(|| Some("lane closed".to_string())),
        LaneEventName::PromptMisdelivery
        | LaneEventName::Blocked
        | LaneEventName::Red
        | LaneEventName::Failed
        | LaneEventName::Superseded
        | LaneEventName::RecoveryAttempted
        | LaneEventName::BranchStaleAgainstMain => None,
    }
}

fn commit_checkpoint_label(event: &LaneEvent) -> Option<String> {
    event
        .data
        .as_ref()
        .and_then(|data| {
            data.get("canonicalCommit")
                .or_else(|| data.get("commit"))
                .and_then(serde_json::Value::as_str)
        })
        .map(|commit| format!("commit {commit} created"))
}

fn recommended_next_action(
    current_phase: &str,
    derived_state: &str,
    failure_class: Option<LaneFailureClass>,
) -> String {
    if current_phase == "blocked" {
        if let Some(failure_class) = failure_class {
            return recovery_action_for_failure_class(failure_class);
        }

        return match derived_state.trim().to_ascii_lowercase().as_str() {
            "blocked_background_job" => {
                "wait for the background job to finish or cancel it explicitly".to_string()
            }
            "blocked_merge_conflict" => {
                "resolve the merge conflict, then rerun verification".to_string()
            }
            "degraded_mcp" => "repair MCP startup or handshake, then retry the lane".to_string(),
            "interrupted_transport" => {
                "restart the interrupted lane and confirm transport health".to_string()
            }
            _ => "inspect the blocking failure and rerun the lane".to_string(),
        };
    }

    match current_phase {
        "exploring" => "turn exploration findings into a concrete implementation plan".to_string(),
        "planning" => "convert the plan into an implementation task".to_string(),
        "verifying" => "finish verification and report the outcome".to_string(),
        "done" => match derived_state.trim().to_ascii_lowercase().as_str() {
            "finished_pending_report" => {
                "publish the final report for the completed lane".to_string()
            }
            "finished_cleanable" => {
                "review the completed lane output and clean up the session".to_string()
            }
            _ => "review the completed lane output and decide whether to close it out".to_string(),
        },
        _ => "continue implementation and emit the next lane checkpoint".to_string(),
    }
}

fn recovery_action_for_failure_class(failure_class: LaneFailureClass) -> String {
    match failure_class {
        LaneFailureClass::PromptDelivery => recipe_action(FailureScenario::PromptMisdelivery),
        LaneFailureClass::TrustGate => recipe_action(FailureScenario::TrustPromptUnresolved),
        LaneFailureClass::BranchDivergence => recipe_action(FailureScenario::StaleBranch),
        LaneFailureClass::Compile => recipe_action(FailureScenario::CompileRedCrossCrate),
        LaneFailureClass::McpHandshake => recipe_action(FailureScenario::McpHandshakeFailure),
        LaneFailureClass::PluginStartup => {
            "restart the affected plugin, then retry startup".to_string()
        }
        LaneFailureClass::Test => "fix the failing tests, then rerun verification".to_string(),
        LaneFailureClass::McpStartup => "restart the MCP server, then retry discovery".to_string(),
        LaneFailureClass::GatewayRouting => {
            "check gateway routing and retry the request".to_string()
        }
        LaneFailureClass::ToolRuntime => {
            "inspect the tool runtime failure and rerun the lane".to_string()
        }
        LaneFailureClass::Infra => {
            "inspect the infrastructure failure and retry the lane".to_string()
        }
    }
}

fn recipe_action(scenario: FailureScenario) -> String {
    let recipe = recipe_for(&scenario);
    match scenario {
        FailureScenario::TrustPromptUnresolved => {
            "resolve the trust prompt, then resend the task".to_string()
        }
        FailureScenario::PromptMisdelivery => {
            "redirect the prompt to the agent lane, then retry delivery".to_string()
        }
        FailureScenario::StaleBranch => {
            "rebase or merge from main, then rerun verification".to_string()
        }
        FailureScenario::CompileRedCrossCrate => {
            format!(
                "run the recovery recipe starting with {:?}",
                recipe.steps[0]
            )
        }
        FailureScenario::McpHandshakeFailure => {
            "retry the MCP handshake or restart the server".to_string()
        }
        FailureScenario::PartialPluginStartup => {
            "restart the affected plugin, then retry MCP handshake".to_string()
        }
        FailureScenario::ProviderFailure => "restart the worker, then rerun the lane".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{derive_actionable_summary, ActionableSummary};
    use crate::{LaneEvent, LaneEventBlocker, LaneEventName, LaneEventStatus, LaneFailureClass};

    #[test]
    fn given_explore_lane_when_running_then_summary_marks_exploring() {
        // given
        let events = vec![LaneEvent::started("2026-04-06T00:00:00Z")];

        // when
        let summary =
            derive_actionable_summary("running", "working", Some("Explore"), &events, None);

        // then
        assert_eq!(
            summary,
            ActionableSummary {
                current_phase: "exploring".to_string(),
                last_successful_checkpoint: "lane started".to_string(),
                current_blocker: None,
                recommended_next_action:
                    "turn exploration findings into a concrete implementation plan".to_string(),
            }
        );
    }

    #[test]
    fn given_blocked_compile_lane_when_deriving_then_summary_uses_blocker_and_recovery() {
        // given
        let blocker = LaneEventBlocker {
            failure_class: LaneFailureClass::Compile,
            detail: "cargo test failed in runtime".to_string(),
        };
        let events = vec![
            LaneEvent::started("2026-04-06T00:00:00Z"),
            LaneEvent::finished(
                "2026-04-06T00:01:00Z",
                Some("compiled the runtime crate".to_string()),
            ),
            LaneEvent::blocked("2026-04-06T00:02:00Z", &blocker),
            LaneEvent::failed("2026-04-06T00:02:01Z", &blocker),
        ];

        // when
        let summary = derive_actionable_summary(
            "failed",
            "truly_idle",
            Some("Verification"),
            &events,
            Some(&blocker),
        );

        // then
        assert_eq!(summary.current_phase, "blocked");
        assert_eq!(
            summary.last_successful_checkpoint,
            "compiled the runtime crate"
        );
        assert_eq!(
            summary.current_blocker.as_deref(),
            Some("cargo test failed in runtime")
        );
        assert_eq!(
            summary.recommended_next_action,
            "run the recovery recipe starting with CleanBuild"
        );
    }

    #[test]
    fn given_finished_commit_lane_when_deriving_then_summary_marks_done_with_latest_checkpoint() {
        // given
        let events = vec![
            LaneEvent::started("2026-04-06T00:00:00Z"),
            LaneEvent::new(
                LaneEventName::CommitCreated,
                LaneEventStatus::Completed,
                "2026-04-06T00:01:00Z",
            )
            .with_data(json!({"commit": "abc1234"})),
            LaneEvent::finished(
                "2026-04-06T00:02:00Z",
                Some("finished successfully".to_string()),
            ),
        ];

        // when
        let summary =
            derive_actionable_summary("completed", "finished_cleanable", None, &events, None);

        // then
        assert_eq!(summary.current_phase, "done");
        assert_eq!(summary.last_successful_checkpoint, "finished successfully");
        assert!(summary.current_blocker.is_none());
        assert_eq!(
            summary.recommended_next_action,
            "review the completed lane output and clean up the session"
        );
    }

    #[test]
    fn given_no_lane_context_when_falling_back_then_summary_stays_grounded() {
        // given / when
        let summary = ActionableSummary::fallback(true, false);

        // then
        assert_eq!(summary.current_phase, "done");
        assert_eq!(
            summary.last_successful_checkpoint,
            "workspace status snapshot collected"
        );
        assert!(summary.current_blocker.is_none());
        assert_eq!(
            summary.recommended_next_action,
            "resume or start a lane to populate actionable status"
        );
    }
}
