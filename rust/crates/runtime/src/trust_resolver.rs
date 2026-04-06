use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const TRUST_PROMPT_CUES: &[&str] = &[
    "do you trust the files in this folder",
    "trust the files in this folder",
    "trust this folder",
    "allow and continue",
    "yes, proceed",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustPolicy {
    AutoTrust,
    RequireApproval,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Pending,
    Required,
    Resolved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustEvent {
    TrustRequired {
        cwd: String,
        state: TrustState,
    },
    TrustResolved {
        cwd: String,
        policy: TrustPolicy,
        state: TrustState,
    },
    TrustDenied {
        cwd: String,
        reason: String,
        state: TrustState,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustConfig {
    allowlisted: Vec<PathBuf>,
    denied: Vec<PathBuf>,
}

impl TrustConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_allowlisted(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowlisted.push(path.into());
        self
    }

    #[must_use]
    pub fn with_denied(mut self, path: impl Into<PathBuf>) -> Self {
        self.denied.push(path.into());
        self
    }

    #[must_use]
    pub fn allowlisted(&self) -> &[PathBuf] {
        &self.allowlisted
    }

    #[must_use]
    pub fn denied(&self) -> &[PathBuf] {
        &self.denied
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustDecision {
    NotRequired {
        state: TrustState,
    },
    Required {
        policy: TrustPolicy,
        state: TrustState,
        events: Vec<TrustEvent>,
    },
}

impl TrustDecision {
    #[must_use]
    pub fn policy(&self) -> Option<TrustPolicy> {
        match self {
            Self::NotRequired { .. } => None,
            Self::Required { policy, .. } => Some(*policy),
        }
    }

    #[must_use]
    pub fn state(&self) -> TrustState {
        match self {
            Self::NotRequired { state } | Self::Required { state, .. } => *state,
        }
    }

    #[must_use]
    pub fn events(&self) -> &[TrustEvent] {
        match self {
            Self::NotRequired { .. } => &[],
            Self::Required { events, .. } => events,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrustResolver {
    config: TrustConfig,
}

impl TrustResolver {
    #[must_use]
    pub fn new(config: TrustConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn resolve(&self, cwd: &str, screen_text: &str) -> TrustDecision {
        if !detect_trust_prompt(screen_text) {
            return TrustDecision::NotRequired {
                state: TrustState::Pending,
            };
        }

        let mut events = vec![TrustEvent::TrustRequired {
            cwd: cwd.to_owned(),
            state: TrustState::Required,
        }];

        if let Some(matched_root) = self
            .config
            .denied
            .iter()
            .find(|root| path_matches(cwd, root))
        {
            let reason = format!("cwd matches denied trust root: {}", matched_root.display());
            events.push(TrustEvent::TrustDenied {
                cwd: cwd.to_owned(),
                reason,
                state: TrustState::Denied,
            });
            return TrustDecision::Required {
                policy: TrustPolicy::Deny,
                state: TrustState::Denied,
                events,
            };
        }

        if self
            .config
            .allowlisted
            .iter()
            .any(|root| path_matches(cwd, root))
        {
            events.push(TrustEvent::TrustResolved {
                cwd: cwd.to_owned(),
                policy: TrustPolicy::AutoTrust,
                state: TrustState::Resolved,
            });
            return TrustDecision::Required {
                policy: TrustPolicy::AutoTrust,
                state: TrustState::Resolved,
                events,
            };
        }

        TrustDecision::Required {
            policy: TrustPolicy::RequireApproval,
            state: TrustState::Required,
            events,
        }
    }

    #[must_use]
    pub fn policy_for_cwd(&self, cwd: &str) -> TrustPolicy {
        if self
            .config
            .denied
            .iter()
            .any(|root| path_matches(cwd, root))
        {
            return TrustPolicy::Deny;
        }
        if self
            .config
            .allowlisted
            .iter()
            .any(|root| path_matches(cwd, root))
        {
            return TrustPolicy::AutoTrust;
        }
        TrustPolicy::RequireApproval
    }

    #[must_use]
    pub fn trusts(&self, cwd: &str) -> bool {
        self.policy_for_cwd(cwd) == TrustPolicy::AutoTrust
    }
}

#[must_use]
pub fn detect_trust_prompt(screen_text: &str) -> bool {
    let lowered = screen_text.to_ascii_lowercase();
    TRUST_PROMPT_CUES
        .iter()
        .any(|needle| lowered.contains(needle))
}

#[cfg(test)]
#[must_use]
pub fn path_matches_trusted_root(cwd: &str, trusted_root: &str) -> bool {
    path_matches(cwd, &normalize_path(Path::new(trusted_root)))
}

fn path_matches(candidate: &str, root: &Path) -> bool {
    let candidate = normalize_path(Path::new(candidate));
    let root = normalize_path(root);
    candidate == root || candidate.starts_with(&root)
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{
        detect_trust_prompt, path_matches_trusted_root, TrustConfig, TrustDecision, TrustEvent,
        TrustPolicy, TrustResolver, TrustState,
    };

    #[test]
    fn detects_known_trust_prompt_copy() {
        // given
        let screen_text = "Do you trust the files in this folder?\n1. Yes, proceed\n2. No";

        // when
        let detected = detect_trust_prompt(screen_text);

        // then
        assert!(detected);
    }

    #[test]
    fn does_not_emit_events_when_prompt_is_absent() {
        // given
        let resolver = TrustResolver::new(TrustConfig::new().with_allowlisted("/tmp/worktrees"));

        // when
        let decision = resolver.resolve("/tmp/worktrees/repo-a", "Ready for your input\n>");

        // then
        assert_eq!(
            decision,
            TrustDecision::NotRequired {
                state: TrustState::Pending,
            }
        );
        assert_eq!(decision.events(), &[]);
        assert_eq!(decision.policy(), None);
        assert_eq!(decision.state(), TrustState::Pending);
    }

    #[test]
    fn auto_trusts_allowlisted_cwd_after_prompt_detection() {
        // given
        let resolver = TrustResolver::new(TrustConfig::new().with_allowlisted("/tmp/worktrees"));

        // when
        let decision = resolver.resolve(
            "/tmp/worktrees/repo-a",
            "Do you trust the files in this folder?\n1. Yes, proceed\n2. No",
        );

        // then
        assert_eq!(decision.policy(), Some(TrustPolicy::AutoTrust));
        assert_eq!(decision.state(), TrustState::Resolved);
        assert_eq!(
            decision.events(),
            &[
                TrustEvent::TrustRequired {
                    cwd: "/tmp/worktrees/repo-a".to_string(),
                    state: TrustState::Required,
                },
                TrustEvent::TrustResolved {
                    cwd: "/tmp/worktrees/repo-a".to_string(),
                    policy: TrustPolicy::AutoTrust,
                    state: TrustState::Resolved,
                },
            ]
        );
    }

    #[test]
    fn requires_approval_for_unknown_cwd_after_prompt_detection() {
        // given
        let resolver = TrustResolver::new(TrustConfig::new().with_allowlisted("/tmp/worktrees"));

        // when
        let decision = resolver.resolve(
            "/tmp/other/repo-b",
            "Do you trust the files in this folder?\n1. Yes, proceed\n2. No",
        );

        // then
        assert_eq!(decision.policy(), Some(TrustPolicy::RequireApproval));
        assert_eq!(decision.state(), TrustState::Required);
        assert_eq!(
            decision.events(),
            &[TrustEvent::TrustRequired {
                cwd: "/tmp/other/repo-b".to_string(),
                state: TrustState::Required,
            }]
        );
    }

    #[test]
    fn denied_root_takes_precedence_over_allowlist() {
        // given
        let resolver = TrustResolver::new(
            TrustConfig::new()
                .with_allowlisted("/tmp/worktrees")
                .with_denied("/tmp/worktrees/repo-c"),
        );

        // when
        let decision = resolver.resolve(
            "/tmp/worktrees/repo-c",
            "Do you trust the files in this folder?\n1. Yes, proceed\n2. No",
        );

        // then
        assert_eq!(decision.policy(), Some(TrustPolicy::Deny));
        assert_eq!(decision.state(), TrustState::Denied);
        assert_eq!(
            decision.events(),
            &[
                TrustEvent::TrustRequired {
                    cwd: "/tmp/worktrees/repo-c".to_string(),
                    state: TrustState::Required,
                },
                TrustEvent::TrustDenied {
                    cwd: "/tmp/worktrees/repo-c".to_string(),
                    reason: "cwd matches denied trust root: /tmp/worktrees/repo-c".to_string(),
                    state: TrustState::Denied,
                },
            ]
        );
    }

    #[test]
    fn policy_for_cwd_prefers_denylist_over_allowlist() {
        // given
        let resolver = TrustResolver::new(
            TrustConfig::new()
                .with_allowlisted("/tmp/worktrees")
                .with_denied("/tmp/worktrees/repo-c"),
        );

        // when
        let denied_policy = resolver.policy_for_cwd("/tmp/worktrees/repo-c");
        let allowlisted_policy = resolver.policy_for_cwd("/tmp/worktrees/repo-d");
        let unknown_policy = resolver.policy_for_cwd("/tmp/other/repo-e");

        // then
        assert_eq!(denied_policy, TrustPolicy::Deny);
        assert_eq!(allowlisted_policy, TrustPolicy::AutoTrust);
        assert_eq!(unknown_policy, TrustPolicy::RequireApproval);
    }

    #[test]
    fn sibling_prefix_does_not_match_trusted_root() {
        // given
        let trusted_root = "/tmp/worktrees";
        let sibling_path = "/tmp/worktrees-other/repo-d";

        // when
        let matched = path_matches_trusted_root(sibling_path, trusted_root);

        // then
        assert!(!matched);
    }
}
