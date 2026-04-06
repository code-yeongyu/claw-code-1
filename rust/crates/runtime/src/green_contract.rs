use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GreenLevel {
    #[serde(rename = "targeted_tests_green")]
    TargetedTests,
    #[serde(rename = "package_green")]
    Package,
    #[serde(rename = "workspace_green")]
    Workspace,
    #[serde(rename = "merge_ready_green")]
    MergeReady,
}

impl GreenLevel {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetedTests => "targeted_tests_green",
            Self::Package => "package_green",
            Self::Workspace => "workspace_green",
            Self::MergeReady => "merge_ready_green",
        }
    }
}

#[must_use]
pub fn classify_test_command(command: &str) -> Option<GreenLevel> {
    let normalized = normalize_command(command);
    if is_merge_ready_command(&normalized) {
        return Some(GreenLevel::MergeReady);
    }
    if is_workspace_test_command(&normalized) {
        return Some(GreenLevel::Workspace);
    }
    if is_package_test_command(&normalized) {
        return Some(GreenLevel::Package);
    }
    if is_targeted_test_command(&normalized) {
        return Some(GreenLevel::TargetedTests);
    }
    None
}

#[must_use]
pub fn determine_achieved_green_level(
    command: &str,
    command_succeeded: bool,
) -> Option<GreenLevel> {
    classify_test_command(command).filter(|_| command_succeeded)
}

fn normalize_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn is_merge_ready_command(command: &str) -> bool {
    is_workspace_test_command(command)
        && command.contains("cargo clippy --workspace")
        && (command.contains("cargo fmt --check") || command.contains("cargo fmt --all --check"))
}

fn is_workspace_test_command(command: &str) -> bool {
    [
        "cargo test --workspace",
        "cargo test --all",
        "cargo nextest run --workspace",
        "cargo nextest run --all",
    ]
    .iter()
    .any(|needle| command.contains(needle))
}

fn is_package_test_command(command: &str) -> bool {
    let has_package_flag = [
        "cargo test -p ",
        "cargo test --package ",
        "cargo nextest run -p ",
        "cargo nextest run --package ",
    ]
    .iter()
    .any(|needle| command.contains(needle));
    has_package_flag || is_plain_cargo_test(command)
}

fn is_plain_cargo_test(command: &str) -> bool {
    ["cargo test", "cargo nextest run"]
        .iter()
        .any(|needle| command.contains(needle))
        && !is_workspace_test_command(command)
        && !is_targeted_test_command(command)
}

fn is_targeted_test_command(command: &str) -> bool {
    if !command.contains("cargo test") && !command.contains("cargo nextest run") {
        return false;
    }
    [
        "cargo test --test ",
        "cargo test --bench ",
        "cargo test --example ",
        "cargo test --bin ",
        "cargo test --lib",
        "cargo test ",
        "cargo nextest run --test ",
        "cargo nextest run --bench ",
        "cargo nextest run --example ",
        "cargo nextest run --bin ",
        "cargo nextest run --lib",
        "cargo nextest run ",
    ]
    .iter()
    .any(|needle| {
        command.contains(needle)
            && !is_workspace_test_command(command)
            && !is_package_test_command_from_selector(command, needle)
    })
}

fn is_package_test_command_from_selector(command: &str, needle: &str) -> bool {
    (needle == "cargo test " || needle == "cargo nextest run ")
        && [
            "cargo test -p ",
            "cargo test --package ",
            "cargo nextest run -p ",
            "cargo nextest run --package ",
        ]
        .iter()
        .any(|package_needle| command.contains(package_needle))
}

impl std::fmt::Display for GreenLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GreenContract {
    pub required_level: GreenLevel,
}

impl GreenContract {
    #[must_use]
    pub fn new(required_level: GreenLevel) -> Self {
        Self { required_level }
    }

    #[must_use]
    pub fn evaluate(self, observed_level: Option<GreenLevel>) -> GreenContractOutcome {
        match observed_level {
            Some(level) if level >= self.required_level => GreenContractOutcome::Satisfied {
                required_level: self.required_level,
                observed_level: level,
            },
            _ => GreenContractOutcome::Unsatisfied {
                required_level: self.required_level,
                observed_level,
            },
        }
    }

    #[must_use]
    pub fn is_satisfied_by(self, observed_level: GreenLevel) -> bool {
        observed_level >= self.required_level
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GreenContractOutcome {
    Satisfied {
        required_level: GreenLevel,
        observed_level: GreenLevel,
    },
    Unsatisfied {
        required_level: GreenLevel,
        observed_level: Option<GreenLevel>,
    },
}

impl GreenContractOutcome {
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        matches!(self, Self::Satisfied { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_matching_level_when_evaluating_contract_then_it_is_satisfied() {
        // given
        let contract = GreenContract::new(GreenLevel::Package);

        // when
        let outcome = contract.evaluate(Some(GreenLevel::Package));

        // then
        assert_eq!(
            outcome,
            GreenContractOutcome::Satisfied {
                required_level: GreenLevel::Package,
                observed_level: GreenLevel::Package,
            }
        );
        assert!(outcome.is_satisfied());
    }

    #[test]
    fn given_higher_level_when_checking_requirement_then_it_still_satisfies_contract() {
        // given
        let contract = GreenContract::new(GreenLevel::TargetedTests);

        // when
        let is_satisfied = contract.is_satisfied_by(GreenLevel::Workspace);

        // then
        assert!(is_satisfied);
    }

    #[test]
    fn given_lower_level_when_evaluating_contract_then_it_is_unsatisfied() {
        // given
        let contract = GreenContract::new(GreenLevel::Workspace);

        // when
        let outcome = contract.evaluate(Some(GreenLevel::Package));

        // then
        assert_eq!(
            outcome,
            GreenContractOutcome::Unsatisfied {
                required_level: GreenLevel::Workspace,
                observed_level: Some(GreenLevel::Package),
            }
        );
        assert!(!outcome.is_satisfied());
    }

    #[test]
    fn given_no_green_level_when_evaluating_contract_then_contract_is_unsatisfied() {
        // given
        let contract = GreenContract::new(GreenLevel::MergeReady);

        // when
        let outcome = contract.evaluate(None);

        // then
        assert_eq!(
            outcome,
            GreenContractOutcome::Unsatisfied {
                required_level: GreenLevel::MergeReady,
                observed_level: None,
            }
        );
    }

    #[test]
    fn given_targeted_test_command_when_classifying_then_it_returns_targeted_tests_green() {
        // given
        let command = "cargo test stale_branch";

        // when
        let level = classify_test_command(command);

        // then
        assert_eq!(level, Some(GreenLevel::TargetedTests));
    }

    #[test]
    fn given_package_test_command_when_classifying_then_it_returns_package_green() {
        // given
        let command = "cargo test -p runtime";

        // when
        let level = classify_test_command(command);

        // then
        assert_eq!(level, Some(GreenLevel::Package));
    }

    #[test]
    fn given_workspace_test_command_when_classifying_then_it_returns_workspace_green() {
        // given
        let command = "cargo test --workspace";

        // when
        let level = classify_test_command(command);

        // then
        assert_eq!(level, Some(GreenLevel::Workspace));
    }

    #[test]
    fn given_workspace_verification_command_when_classifying_then_it_returns_merge_ready_green() {
        // given
        let command =
            "cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace";

        // when
        let level = classify_test_command(command);

        // then
        assert_eq!(level, Some(GreenLevel::MergeReady));
    }

    #[test]
    fn given_failed_test_run_when_determining_achieved_level_then_it_returns_none() {
        // given
        let command = "cargo test --workspace";

        // when
        let level = determine_achieved_green_level(command, false);

        // then
        assert_eq!(level, None);
    }

    #[test]
    fn given_successful_test_run_when_determining_achieved_level_then_it_returns_attempted_level() {
        // given
        let command = "cargo test -p runtime";

        // when
        let level = determine_achieved_green_level(command, true);

        // then
        assert_eq!(level, Some(GreenLevel::Package));
    }

    #[test]
    fn given_green_level_when_serializing_then_it_uses_green_suffix_wire_values() {
        // given
        let level = GreenLevel::Workspace;

        // when
        let serialized = serde_json::to_value(level).expect("green level should serialize");

        // then
        assert_eq!(serialized, json!("workspace_green"));
    }
}
