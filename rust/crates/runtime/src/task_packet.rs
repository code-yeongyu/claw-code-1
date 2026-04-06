use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScope {
    Workspace,
    Module,
    SingleFile,
    Custom,
}

impl Display for TaskScope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        serde_json::to_string(self)
            .map_err(|_| std::fmt::Error)
            .and_then(|value| write!(f, "{}", value.trim_matches('"')))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchPolicy {
    AutoRebase,
    AutoMergeForward,
    WarnOnly,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitPolicy {
    Forbid,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportingContract {
    SummaryOnly,
    AcceptanceTests,
    AcceptanceTestsAndCommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPacket {
    pub objective: String,
    pub scope: TaskScope,
    pub repo: Option<PathBuf>,
    pub worktree: Option<PathBuf>,
    pub branch_policy: BranchPolicy,
    pub acceptance_tests: Vec<String>,
    pub commit_policy: CommitPolicy,
    pub reporting_contract: ReportingContract,
    pub escalation_policy: crate::EscalationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPacketValidationError {
    errors: Vec<String>,
}

impl TaskPacketValidationError {
    #[must_use]
    pub fn new(errors: Vec<String>) -> Self {
        Self { errors }
    }

    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}

impl Display for TaskPacketValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.errors.join("; "))
    }
}

impl std::error::Error for TaskPacketValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPacket(TaskPacket);

impl ValidatedPacket {
    #[must_use]
    pub fn packet(&self) -> &TaskPacket {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> TaskPacket {
        self.0
    }
}

pub fn validate_packet(packet: TaskPacket) -> Result<ValidatedPacket, TaskPacketValidationError> {
    let mut errors = Vec::new();

    validate_required("objective", &packet.objective, &mut errors);
    validate_optional_path("repo", packet.repo.as_ref(), &mut errors);
    validate_optional_path("worktree", packet.worktree.as_ref(), &mut errors);

    for (index, test) in packet.acceptance_tests.iter().enumerate() {
        if test.trim().is_empty() {
            errors.push(format!(
                "acceptance_tests contains an empty value at index {index}"
            ));
        }
    }

    if errors.is_empty() {
        Ok(ValidatedPacket(packet))
    } else {
        Err(TaskPacketValidationError::new(errors))
    }
}

fn validate_required(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

fn validate_optional_path(field: &str, value: Option<&PathBuf>, errors: &mut Vec<String>) {
    if value.is_some_and(|path| path.as_os_str().is_empty()) {
        errors.push(format!("{field} must not be empty when provided"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_packet() -> TaskPacket {
        TaskPacket {
            objective: "Implement typed task packet format".to_string(),
            scope: TaskScope::Workspace,
            repo: Some(PathBuf::from("/tmp/claw-code")),
            worktree: Some(PathBuf::from("/tmp/claw-code/.worktrees/task-packet")),
            branch_policy: BranchPolicy::AutoRebase,
            acceptance_tests: vec![
                "cargo build --workspace".to_string(),
                "cargo test --workspace".to_string(),
            ],
            commit_policy: CommitPolicy::Required,
            reporting_contract: ReportingContract::AcceptanceTestsAndCommit,
            escalation_policy: crate::EscalationPolicy::AlertHuman,
        }
    }

    #[test]
    fn given_valid_packet_when_validating_then_packet_is_returned() {
        // given
        let packet = sample_packet();

        // when
        let validated = validate_packet(packet.clone()).expect("packet should validate");

        // then
        assert_eq!(validated.packet(), &packet);
        assert_eq!(validated.into_inner(), packet);
    }

    #[test]
    fn given_invalid_packet_when_validating_then_errors_accumulate() {
        // given
        let packet = TaskPacket {
            objective: " ".to_string(),
            scope: TaskScope::Custom,
            repo: Some(PathBuf::new()),
            worktree: Some(PathBuf::new()),
            branch_policy: BranchPolicy::Block,
            acceptance_tests: vec!["ok".to_string(), " ".to_string()],
            commit_policy: CommitPolicy::Optional,
            reporting_contract: ReportingContract::SummaryOnly,
            escalation_policy: crate::EscalationPolicy::Abort,
        };

        // when
        let error = validate_packet(packet).expect_err("packet should be rejected");

        // then
        assert_eq!(error.errors().len(), 4);
        assert!(error
            .errors()
            .contains(&"objective must not be empty".to_string()));
        assert!(error
            .errors()
            .contains(&"repo must not be empty when provided".to_string()));
        assert!(error
            .errors()
            .contains(&"worktree must not be empty when provided".to_string()));
        assert!(error
            .errors()
            .contains(&"acceptance_tests contains an empty value at index 1".to_string()));
    }

    #[test]
    fn given_typed_packet_when_serializing_then_roundtrip_preserves_fields() {
        // given
        let packet = sample_packet();

        // when
        let serialized = serde_json::to_string(&packet).expect("packet should serialize");
        let deserialized: TaskPacket =
            serde_json::from_str(&serialized).expect("packet should deserialize");

        // then
        assert_eq!(deserialized, packet);
        assert!(serialized.contains(r#""scope":"workspace""#));
        assert!(serialized.contains(r#""branch_policy":"auto_rebase""#));
        assert!(serialized.contains(r#""commit_policy":"required""#));
        assert!(serialized.contains(r#""reporting_contract":"acceptance_tests_and_commit""#));
        assert!(serialized.contains(r#""escalation_policy":"alert_human""#));
    }
}
