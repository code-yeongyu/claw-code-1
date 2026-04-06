#![allow(clippy::similar_names)]
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaneEventName {
    #[serde(rename = "lane.started")]
    Started,
    #[serde(rename = "lane.ready")]
    Ready,
    #[serde(rename = "lane.prompt_misdelivery")]
    PromptMisdelivery,
    #[serde(rename = "lane.blocked")]
    Blocked,
    #[serde(rename = "lane.red")]
    Red,
    #[serde(rename = "lane.green")]
    Green,
    #[serde(rename = "lane.commit.created")]
    CommitCreated,
    #[serde(rename = "lane.pr.opened")]
    PrOpened,
    #[serde(rename = "lane.merge.ready")]
    MergeReady,
    #[serde(rename = "lane.finished")]
    Finished,
    #[serde(rename = "lane.failed")]
    Failed,
    #[serde(rename = "lane.reconciled")]
    Reconciled,
    #[serde(rename = "lane.merged")]
    Merged,
    #[serde(rename = "lane.superseded")]
    Superseded,
    #[serde(rename = "lane.closed")]
    Closed,
    #[serde(rename = "lane.recovery_attempted")]
    RecoveryAttempted,
    #[serde(rename = "branch.stale_against_main")]
    BranchStaleAgainstMain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneEventStatus {
    Running,
    Ready,
    Blocked,
    Red,
    Green,
    Recovering,
    Completed,
    Failed,
    Reconciled,
    Merged,
    Superseded,
    Closed,
}

impl std::fmt::Display for LaneEventStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Red => "red",
            Self::Green => "green",
            Self::Recovering => "recovering",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Reconciled => "reconciled",
            Self::Merged => "merged",
            Self::Superseded => "superseded",
            Self::Closed => "closed",
        };
        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    PromptDelivery,
    TrustGate,
    BranchDivergence,
    Compile,
    Test,
    PluginStartup,
    McpStartup,
    McpHandshake,
    GatewayRouting,
    ToolRuntime,
    Infra,
}

pub type LaneFailureClass = FailureClass;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneEventBlocker {
    #[serde(rename = "failureClass")]
    pub failure_class: LaneFailureClass,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneCommitProvenance {
    pub commit: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(rename = "canonicalCommit", skip_serializing_if = "Option::is_none")]
    pub canonical_commit: Option<String>,
    #[serde(rename = "supersededBy", skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneEvent {
    pub event: LaneEventName,
    pub status: LaneEventStatus,
    #[serde(rename = "emittedAt")]
    pub emitted_at: String,
    #[serde(rename = "failureClass", skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<LaneFailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl LaneEvent {
    #[must_use]
    pub fn new(
        event: LaneEventName,
        status: LaneEventStatus,
        emitted_at: impl Into<String>,
    ) -> Self {
        Self {
            event,
            status,
            emitted_at: emitted_at.into(),
            failure_class: None,
            detail: None,
            data: None,
        }
    }

    #[must_use]
    pub fn started(emitted_at: impl Into<String>) -> Self {
        Self::new(LaneEventName::Started, LaneEventStatus::Running, emitted_at)
    }

    #[must_use]
    pub fn ready(emitted_at: impl Into<String>) -> Self {
        Self::new(LaneEventName::Ready, LaneEventStatus::Ready, emitted_at)
    }

    #[must_use]
    pub fn finished(emitted_at: impl Into<String>, detail: Option<String>) -> Self {
        Self::new(
            LaneEventName::Finished,
            LaneEventStatus::Completed,
            emitted_at,
        )
        .with_optional_detail(detail)
    }

    #[must_use]
    pub fn commit_created(
        emitted_at: impl Into<String>,
        detail: Option<String>,
        provenance: LaneCommitProvenance,
    ) -> Self {
        Self::new(
            LaneEventName::CommitCreated,
            LaneEventStatus::Completed,
            emitted_at,
        )
        .with_optional_detail(detail)
        .with_data(serde_json::to_value(provenance).expect("commit provenance should serialize"))
    }

    #[must_use]
    pub fn pr_opened(
        emitted_at: impl Into<String>,
        detail: Option<String>,
        data: Option<Value>,
    ) -> Self {
        Self::new(
            LaneEventName::PrOpened,
            LaneEventStatus::Completed,
            emitted_at,
        )
        .with_optional_detail(detail)
        .with_optional_data(data)
    }

    #[must_use]
    pub fn merge_ready(emitted_at: impl Into<String>, detail: Option<String>) -> Self {
        Self::new(
            LaneEventName::MergeReady,
            LaneEventStatus::Green,
            emitted_at,
        )
        .with_optional_detail(detail)
    }

    #[must_use]
    pub fn superseded(
        emitted_at: impl Into<String>,
        detail: Option<String>,
        provenance: LaneCommitProvenance,
    ) -> Self {
        Self::new(
            LaneEventName::Superseded,
            LaneEventStatus::Superseded,
            emitted_at,
        )
        .with_optional_detail(detail)
        .with_data(serde_json::to_value(provenance).expect("commit provenance should serialize"))
    }

    #[must_use]
    pub fn recovery_attempted(
        emitted_at: impl Into<String>,
        failure_class: LaneFailureClass,
        data: Value,
    ) -> Self {
        Self::new(
            LaneEventName::RecoveryAttempted,
            LaneEventStatus::Recovering,
            emitted_at,
        )
        .with_failure_class(failure_class)
        .with_data(data)
    }

    #[must_use]
    pub fn blocked(emitted_at: impl Into<String>, blocker: &LaneEventBlocker) -> Self {
        Self::new(LaneEventName::Blocked, LaneEventStatus::Blocked, emitted_at)
            .with_failure_class(blocker.failure_class)
            .with_detail(blocker.detail.clone())
    }

    #[must_use]
    pub fn prompt_misdelivery(
        emitted_at: impl Into<String>,
        detail: Option<String>,
        data: Option<Value>,
    ) -> Self {
        Self::new(
            LaneEventName::PromptMisdelivery,
            LaneEventStatus::Blocked,
            emitted_at,
        )
        .with_failure_class(LaneFailureClass::PromptDelivery)
        .with_optional_detail(detail)
        .with_optional_data(data)
    }

    #[must_use]
    pub fn failed(emitted_at: impl Into<String>, blocker: &LaneEventBlocker) -> Self {
        Self::new(LaneEventName::Failed, LaneEventStatus::Failed, emitted_at)
            .with_failure_class(blocker.failure_class)
            .with_detail(blocker.detail.clone())
    }

    #[must_use]
    pub fn red(emitted_at: impl Into<String>, detail: Option<String>) -> Self {
        Self::new(LaneEventName::Red, LaneEventStatus::Red, emitted_at).with_optional_detail(detail)
    }

    #[must_use]
    pub fn green(emitted_at: impl Into<String>, detail: Option<String>) -> Self {
        Self::new(LaneEventName::Green, LaneEventStatus::Green, emitted_at)
            .with_optional_detail(detail)
    }

    #[must_use]
    pub fn branch_stale_against_main(
        emitted_at: impl Into<String>,
        detail: impl Into<String>,
        data: Value,
    ) -> Self {
        Self::new(
            LaneEventName::BranchStaleAgainstMain,
            LaneEventStatus::Blocked,
            emitted_at,
        )
        .with_failure_class(LaneFailureClass::BranchDivergence)
        .with_detail(detail)
        .with_data(data)
    }

    #[must_use]
    pub fn with_failure_class(mut self, failure_class: LaneFailureClass) -> Self {
        self.failure_class = Some(failure_class);
        self
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn with_optional_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }

    #[must_use]
    pub fn with_optional_data(mut self, data: Option<Value>) -> Self {
        self.data = data;
        self
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

#[must_use]
pub fn dedupe_superseded_commit_events(events: &[LaneEvent]) -> Vec<LaneEvent> {
    let mut keep = vec![true; events.len()];
    let mut latest_by_key = std::collections::BTreeMap::<String, usize>::new();

    for (index, event) in events.iter().enumerate() {
        if event.event != LaneEventName::CommitCreated {
            continue;
        }
        let Some(data) = event.data.as_ref() else {
            continue;
        };
        let key = data
            .get("canonicalCommit")
            .or_else(|| data.get("commit"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let superseded = data
            .get("supersededBy")
            .and_then(serde_json::Value::as_str)
            .is_some();
        if superseded {
            keep[index] = false;
            continue;
        }
        if let Some(key) = key {
            if let Some(previous) = latest_by_key.insert(key, index) {
                keep[previous] = false;
            }
        }
    }

    events
        .iter()
        .cloned()
        .zip(keep)
        .filter_map(|(event, retain)| retain.then_some(event))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        dedupe_superseded_commit_events, LaneCommitProvenance, LaneEvent, LaneEventBlocker,
        LaneEventName, LaneEventStatus, LaneFailureClass,
    };

    #[test]
    fn canonical_lane_event_names_serialize_to_expected_wire_values() {
        let cases = [
            (LaneEventName::Started, "lane.started"),
            (LaneEventName::Ready, "lane.ready"),
            (LaneEventName::PromptMisdelivery, "lane.prompt_misdelivery"),
            (LaneEventName::Blocked, "lane.blocked"),
            (LaneEventName::Red, "lane.red"),
            (LaneEventName::Green, "lane.green"),
            (LaneEventName::CommitCreated, "lane.commit.created"),
            (LaneEventName::PrOpened, "lane.pr.opened"),
            (LaneEventName::MergeReady, "lane.merge.ready"),
            (LaneEventName::Finished, "lane.finished"),
            (LaneEventName::Failed, "lane.failed"),
            (LaneEventName::Reconciled, "lane.reconciled"),
            (LaneEventName::Merged, "lane.merged"),
            (LaneEventName::Superseded, "lane.superseded"),
            (LaneEventName::Closed, "lane.closed"),
            (LaneEventName::RecoveryAttempted, "lane.recovery_attempted"),
            (
                LaneEventName::BranchStaleAgainstMain,
                "branch.stale_against_main",
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(
                serde_json::to_value(event).expect("serialize event"),
                json!(expected)
            );
        }
    }

    #[test]
    fn failure_classes_cover_canonical_taxonomy_wire_values() {
        let cases = [
            (LaneFailureClass::PromptDelivery, "prompt_delivery"),
            (LaneFailureClass::TrustGate, "trust_gate"),
            (LaneFailureClass::BranchDivergence, "branch_divergence"),
            (LaneFailureClass::Compile, "compile"),
            (LaneFailureClass::Test, "test"),
            (LaneFailureClass::PluginStartup, "plugin_startup"),
            (LaneFailureClass::McpStartup, "mcp_startup"),
            (LaneFailureClass::McpHandshake, "mcp_handshake"),
            (LaneFailureClass::GatewayRouting, "gateway_routing"),
            (LaneFailureClass::ToolRuntime, "tool_runtime"),
            (LaneFailureClass::Infra, "infra"),
        ];

        for (failure_class, expected) in cases {
            assert_eq!(
                serde_json::to_value(failure_class).expect("serialize failure class"),
                json!(expected)
            );
        }
    }

    #[test]
    fn blocked_and_failed_events_reuse_blocker_details() {
        let blocker = LaneEventBlocker {
            failure_class: LaneFailureClass::McpStartup,
            detail: "broken server".to_string(),
        };

        let blocked = LaneEvent::blocked("2026-04-04T00:00:00Z", &blocker);
        let failed = LaneEvent::failed("2026-04-04T00:00:01Z", &blocker);

        assert_eq!(blocked.event, LaneEventName::Blocked);
        assert_eq!(blocked.status, LaneEventStatus::Blocked);
        assert_eq!(blocked.failure_class, Some(LaneFailureClass::McpStartup));
        assert_eq!(failed.event, LaneEventName::Failed);
        assert_eq!(failed.status, LaneEventStatus::Failed);
        assert_eq!(failed.detail.as_deref(), Some("broken server"));
    }

    #[test]
    fn commit_events_can_carry_worktree_and_supersession_metadata() {
        let event = LaneEvent::commit_created(
            "2026-04-04T00:00:00Z",
            Some("commit created".to_string()),
            LaneCommitProvenance {
                commit: "abc123".to_string(),
                branch: "feature/provenance".to_string(),
                worktree: Some("wt-a".to_string()),
                canonical_commit: Some("abc123".to_string()),
                superseded_by: None,
                lineage: vec!["abc123".to_string()],
            },
        );
        let event_json = serde_json::to_value(&event).expect("lane event should serialize");
        assert_eq!(event_json["event"], "lane.commit.created");
        assert_eq!(event_json["data"]["branch"], "feature/provenance");
        assert_eq!(event_json["data"]["worktree"], "wt-a");
    }

    #[test]
    fn recovering_status_serializes_to_wire_value() {
        assert_eq!(
            serde_json::to_value(LaneEventStatus::Recovering).expect("serialize"),
            json!("recovering")
        );
    }

    #[test]
    fn recovery_attempted_event_carries_failure_class_and_data() {
        // given
        let data = json!({"scenario": "stale_branch", "steps_taken": 2});

        // when
        let event = LaneEvent::recovery_attempted(
            "2026-05-01T00:00:00Z",
            LaneFailureClass::BranchDivergence,
            data.clone(),
        );

        // then
        assert_eq!(event.event, LaneEventName::RecoveryAttempted);
        assert_eq!(event.status, LaneEventStatus::Recovering);
        assert_eq!(
            event.failure_class,
            Some(LaneFailureClass::BranchDivergence)
        );
        let event_json = serde_json::to_value(&event).expect("serialize");
        assert_eq!(event_json["event"], "lane.recovery_attempted");
        assert_eq!(event_json["status"], "recovering");
        assert_eq!(event_json["data"]["scenario"], "stale_branch");
    }

    #[test]
    fn dedupes_superseded_commit_events_by_canonical_commit() {
        let retained = dedupe_superseded_commit_events(&[
            LaneEvent::commit_created(
                "2026-04-04T00:00:00Z",
                Some("old".to_string()),
                LaneCommitProvenance {
                    commit: "old123".to_string(),
                    branch: "feature/provenance".to_string(),
                    worktree: Some("wt-a".to_string()),
                    canonical_commit: Some("canon123".to_string()),
                    superseded_by: Some("new123".to_string()),
                    lineage: vec!["old123".to_string(), "new123".to_string()],
                },
            ),
            LaneEvent::commit_created(
                "2026-04-04T00:00:01Z",
                Some("new".to_string()),
                LaneCommitProvenance {
                    commit: "new123".to_string(),
                    branch: "feature/provenance".to_string(),
                    worktree: Some("wt-b".to_string()),
                    canonical_commit: Some("canon123".to_string()),
                    superseded_by: None,
                    lineage: vec!["old123".to_string(), "new123".to_string()],
                },
            ),
        ]);
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].detail.as_deref(), Some("new"));
    }

    #[test]
    fn canonical_lane_event_constructors_preserve_expected_statuses() {
        // given
        let prompt_payload = json!({
            "observed_target": "shell",
            "recovery_armed": true,
        });
        let pr_payload = json!({
            "url": "https://github.com/example/repo/pull/42",
        });
        let stale_payload = json!({
            "branch": "feature/stale",
            "mainRef": "main",
            "commitsBehind": 2,
            "missingCommits": ["fix missing"],
        });

        // when
        let ready = LaneEvent::ready("100");
        let prompt_misdelivery = LaneEvent::prompt_misdelivery(
            "101",
            Some("prompt landed in shell".to_string()),
            Some(prompt_payload.clone()),
        );
        let red = LaneEvent::red("102", Some("tests failed".to_string()));
        let green = LaneEvent::green("103", Some("tests passed".to_string()));
        let pr_opened = LaneEvent::pr_opened(
            "104",
            Some("opened PR #42".to_string()),
            Some(pr_payload.clone()),
        );
        let merge_ready = LaneEvent::merge_ready("105", Some("ready to merge".to_string()));
        let stale = LaneEvent::branch_stale_against_main(
            "106",
            "branch stale against main",
            stale_payload.clone(),
        );

        // then
        assert_eq!(ready.event, LaneEventName::Ready);
        assert_eq!(ready.status, LaneEventStatus::Ready);
        assert_eq!(prompt_misdelivery.event, LaneEventName::PromptMisdelivery);
        assert_eq!(prompt_misdelivery.status, LaneEventStatus::Blocked);
        assert_eq!(
            prompt_misdelivery.failure_class,
            Some(LaneFailureClass::PromptDelivery)
        );
        assert_eq!(prompt_misdelivery.data, Some(prompt_payload));
        assert_eq!(red.status, LaneEventStatus::Red);
        assert_eq!(green.status, LaneEventStatus::Green);
        assert_eq!(pr_opened.event, LaneEventName::PrOpened);
        assert_eq!(pr_opened.data, Some(pr_payload));
        assert_eq!(merge_ready.event, LaneEventName::MergeReady);
        assert_eq!(merge_ready.status, LaneEventStatus::Green);
        assert_eq!(stale.event, LaneEventName::BranchStaleAgainstMain);
        assert_eq!(
            stale.failure_class,
            Some(LaneFailureClass::BranchDivergence)
        );
        assert_eq!(stale.data, Some(stale_payload));
    }

    #[test]
    fn lane_event_serialization_roundtrip_preserves_canonical_variants() {
        // given
        let blocker = LaneEventBlocker {
            failure_class: LaneFailureClass::ToolRuntime,
            detail: "tool failed".to_string(),
        };
        let events = vec![
            LaneEvent::started("100"),
            LaneEvent::ready("101"),
            LaneEvent::prompt_misdelivery(
                "102",
                Some("prompt landed in shell".to_string()),
                Some(json!({ "observed_target": "shell" })),
            ),
            LaneEvent::blocked("103", &blocker),
            LaneEvent::red("104", Some("tests failed".to_string())),
            LaneEvent::green("105", Some("tests passed".to_string())),
            LaneEvent::commit_created(
                "106",
                Some("commit abc1234".to_string()),
                LaneCommitProvenance {
                    commit: "abc1234".to_string(),
                    branch: "feature/events".to_string(),
                    worktree: Some("wt-a".to_string()),
                    canonical_commit: Some("abc1234".to_string()),
                    superseded_by: None,
                    lineage: vec!["abc1234".to_string()],
                },
            ),
            LaneEvent::pr_opened(
                "107",
                Some("opened PR #42".to_string()),
                Some(json!({ "url": "https://github.com/example/repo/pull/42" })),
            ),
            LaneEvent::merge_ready("108", Some("ready to merge".to_string())),
            LaneEvent::finished("109", Some("lane finished cleanly".to_string())),
            LaneEvent::failed("110", &blocker),
            LaneEvent::branch_stale_against_main(
                "111",
                "branch stale against main",
                json!({
                    "branch": "feature/events",
                    "mainRef": "main",
                    "commitsBehind": 3,
                    "missingCommits": ["fix: unblock tests"],
                }),
            ),
        ];

        // when
        let serialized = serde_json::to_string(&events).expect("events should serialize");
        let deserialized: Vec<LaneEvent> =
            serde_json::from_str(&serialized).expect("events should deserialize");

        // then
        assert_eq!(deserialized, events);
    }

    #[test]
    fn branch_stale_event_serializes_to_clawhip_wire_shape() {
        // given
        let event = LaneEvent::branch_stale_against_main(
            "200",
            "branch divergence detected",
            json!({
                "branch": "feature/stale",
                "mainRef": "main",
                "commitsBehind": 5,
                "commitsAhead": 1,
                "missingCommits": ["fix: unblock tests"],
                "blockedCommand": "cargo test --workspace",
                "recommendedAction": "merge or rebase main before workspace tests",
            }),
        );

        // when
        let event_json = serde_json::to_value(&event).expect("lane event should serialize");

        // then
        assert_eq!(event_json["event"], "branch.stale_against_main");
        assert_eq!(event_json["status"], "blocked");
        assert_eq!(event_json["failureClass"], "branch_divergence");
        assert_eq!(event_json["data"]["mainRef"], "main");
        assert_eq!(
            event_json["data"]["blockedCommand"],
            "cargo test --workspace"
        );
    }
}
