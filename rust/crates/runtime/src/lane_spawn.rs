use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{validate_packet, TaskPacket};

pub const DEFAULT_LANE_SPAWN_TIMEOUT: Duration = Duration::from_secs(30);
const TMUX_SESSION_PREFIX: &str = "claw-code-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneSpawnTransport {
    Tmux,
    InProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneSpawnMode {
    Created,
    Attached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSpawnRequest {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub branch: String,
    pub session_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneSpawnResult {
    pub mode: LaneSpawnMode,
    pub transport: LaneSpawnTransport,
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub created_worktree: bool,
    pub created_session: bool,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSpawnCommandSpec {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSpawnCommandOutput {
    pub status_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait LaneSpawnCommandRunner {
    fn run(
        &self,
        spec: &LaneSpawnCommandSpec,
        timeout: Duration,
    ) -> Result<LaneSpawnCommandOutput, String>;
}

pub fn lane_spawn_request_from_packet(
    packet: &TaskPacket,
    cwd: &Path,
) -> Result<LaneSpawnRequest, String> {
    validate_packet(packet.clone()).map_err(|error| error.to_string())?;

    let repo = packet
        .repo
        .as_ref()
        .map_or_else(|| cwd.to_path_buf(), |path| absolutize_from(cwd, path));

    let worktree_input = packet
        .worktree
        .as_ref()
        .ok_or_else(|| String::from("task packet worktree is required for lane spawn"))?;
    let worktree = absolutize_from(&repo, worktree_input);
    let branch = infer_branch_name(&worktree)?;
    let session_name = build_tmux_session_name(&branch);

    Ok(LaneSpawnRequest {
        repo,
        worktree,
        branch,
        session_name,
    })
}

pub fn spawn_lane_from_packet(
    packet: &TaskPacket,
    cwd: &Path,
    transport: LaneSpawnTransport,
    log: &mut dyn FnMut(&str),
) -> Result<LaneSpawnResult, String> {
    let request = lane_spawn_request_from_packet(packet, cwd)?;
    let runner = SystemLaneSpawnCommandRunner;
    spawn_lane_with_runner(
        &request,
        transport,
        DEFAULT_LANE_SPAWN_TIMEOUT,
        &runner,
        log,
    )
}

pub fn spawn_lane_with_runner(
    request: &LaneSpawnRequest,
    transport: LaneSpawnTransport,
    timeout: Duration,
    runner: &dyn LaneSpawnCommandRunner,
    log: &mut dyn FnMut(&str),
) -> Result<LaneSpawnResult, String> {
    let mut logs = Vec::new();
    let mut emit = |message: String| {
        log(&message);
        logs.push(message);
    };

    emit(format!(
        "lane spawn: repo={} branch={} worktree={} transport={}",
        request.repo.display(),
        request.branch,
        request.worktree.display(),
        render_transport(transport)
    ));

    let mut created_worktree = false;
    let mut created_session = false;
    let mut resolved_worktree =
        existing_worktree_for_branch(&request.repo, &request.branch, runner, timeout)?;

    if let Some(existing_worktree) = &resolved_worktree {
        emit(format!(
            "lane spawn: reusing existing worktree {} for branch {}",
            existing_worktree.display(),
            request.branch
        ));
    } else if request.worktree.exists() {
        let worktree_branch = current_branch_for_worktree(&request.worktree, runner, timeout)?;
        if worktree_branch != request.branch {
            return Err(format!(
                "lane spawn: worktree {} already exists on branch {}. Expected {}. Reuse the existing lane/session or choose a different worktree path.",
                request.worktree.display(),
                worktree_branch,
                request.branch
            ));
        }

        emit(format!(
            "lane spawn: reusing existing worktree path {}",
            request.worktree.display()
        ));
        resolved_worktree = Some(request.worktree.clone());
    } else if local_branch_exists(&request.repo, &request.branch, runner, timeout)? {
        emit(format!(
            "lane spawn: branch {} already exists locally; reusing it instead of creating a new branch",
            request.branch
        ));
        run_checked_step(
            runner,
            timeout,
            &LaneSpawnCommandSpec {
                label: format!("create worktree for existing branch {}", request.branch),
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("add"),
                    request.worktree.display().to_string(),
                    request.branch.clone(),
                ],
                cwd: request.repo.clone(),
            },
            &mut emit,
        )?;
        created_worktree = true;
        resolved_worktree = Some(request.worktree.clone());
    } else {
        run_checked_step(
            runner,
            timeout,
            &LaneSpawnCommandSpec {
                label: format!("create worktree and branch {}", request.branch),
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("add"),
                    String::from("-b"),
                    request.branch.clone(),
                    request.worktree.display().to_string(),
                ],
                cwd: request.repo.clone(),
            },
            &mut emit,
        )?;
        created_worktree = true;
        resolved_worktree = Some(request.worktree.clone());
    }

    let worktree = resolved_worktree.expect("lane spawn should resolve worktree");

    if transport == LaneSpawnTransport::Tmux {
        if tmux_session_exists(&request.repo, &request.session_name, runner, timeout)? {
            emit(format!(
                "lane spawn: attaching to existing tmux session {}",
                request.session_name
            ));
        } else {
            run_checked_step(
                runner,
                timeout,
                &LaneSpawnCommandSpec {
                    label: format!("create tmux session {}", request.session_name),
                    program: String::from("tmux"),
                    args: vec![
                        String::from("new-session"),
                        String::from("-d"),
                        String::from("-s"),
                        request.session_name.clone(),
                        String::from("-c"),
                        worktree.display().to_string(),
                    ],
                    cwd: request.repo.clone(),
                },
                &mut emit,
            )?;
            created_session = true;
        }
    } else {
        emit(String::from(
            "lane spawn: in-process transport selected; skipping tmux session creation",
        ));
    }

    Ok(LaneSpawnResult {
        mode: if created_worktree || created_session {
            LaneSpawnMode::Created
        } else {
            LaneSpawnMode::Attached
        },
        transport,
        repo: request.repo.clone(),
        worktree,
        branch: request.branch.clone(),
        session_name: (transport == LaneSpawnTransport::Tmux)
            .then_some(request.session_name.clone()),
        created_worktree,
        created_session,
        logs,
    })
}

struct SystemLaneSpawnCommandRunner;

impl LaneSpawnCommandRunner for SystemLaneSpawnCommandRunner {
    fn run(
        &self,
        spec: &LaneSpawnCommandSpec,
        timeout: Duration,
    ) -> Result<LaneSpawnCommandOutput, String> {
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                format!("lane spawn step '{}' failed to start: {error}", spec.label)
            })?;

        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|error| {
                    format!(
                        "lane spawn step '{}' could not be polled: {error}",
                        spec.label
                    )
                })?
                .is_some()
            {
                let output = child.wait_with_output().map_err(|error| {
                    format!(
                        "lane spawn step '{}' could not collect output: {error}",
                        spec.label
                    )
                })?;
                return Ok(LaneSpawnCommandOutput {
                    status_code: output.status.code().unwrap_or(1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }

            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err(format!(
                    "lane spawn step '{}' exceeded timeout of {}s",
                    spec.label,
                    timeout.as_secs()
                ));
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn run_checked_step(
    runner: &dyn LaneSpawnCommandRunner,
    timeout: Duration,
    spec: &LaneSpawnCommandSpec,
    emit: &mut dyn FnMut(String),
) -> Result<LaneSpawnCommandOutput, String> {
    emit(format!(
        "lane spawn: {} [{} {}]",
        spec.label,
        spec.program,
        spec.args.join(" ")
    ));
    let output = runner.run(spec, timeout)?;
    if output.status_code == 0 {
        return Ok(output);
    }

    let detail = first_non_empty_line(&output.stderr)
        .or_else(|| first_non_empty_line(&output.stdout))
        .unwrap_or_else(|| String::from("command produced no output"));
    Err(format!(
        "lane spawn step '{}' failed with exit code {}: {}",
        spec.label, output.status_code, detail
    ))
}

fn tmux_session_exists(
    repo: &Path,
    session_name: &str,
    runner: &dyn LaneSpawnCommandRunner,
    timeout: Duration,
) -> Result<bool, String> {
    let spec = LaneSpawnCommandSpec {
        label: format!("check tmux session {}", session_name),
        program: String::from("tmux"),
        args: vec![
            String::from("has-session"),
            String::from("-t"),
            session_name.to_string(),
        ],
        cwd: repo.to_path_buf(),
    };
    Ok(runner.run(&spec, timeout)?.status_code == 0)
}

fn local_branch_exists(
    repo: &Path,
    branch: &str,
    runner: &dyn LaneSpawnCommandRunner,
    timeout: Duration,
) -> Result<bool, String> {
    let spec = LaneSpawnCommandSpec {
        label: format!("check local branch {}", branch),
        program: String::from("git"),
        args: vec![
            String::from("show-ref"),
            String::from("--verify"),
            String::from("--quiet"),
            format!("refs/heads/{branch}"),
        ],
        cwd: repo.to_path_buf(),
    };
    Ok(runner.run(&spec, timeout)?.status_code == 0)
}

fn existing_worktree_for_branch(
    repo: &Path,
    branch: &str,
    runner: &dyn LaneSpawnCommandRunner,
    timeout: Duration,
) -> Result<Option<PathBuf>, String> {
    let spec = LaneSpawnCommandSpec {
        label: String::from("inspect git worktrees"),
        program: String::from("git"),
        args: vec![
            String::from("worktree"),
            String::from("list"),
            String::from("--porcelain"),
        ],
        cwd: repo.to_path_buf(),
    };
    let output = run_checked_step(runner, timeout, &spec, &mut |_| {})?;
    Ok(parse_worktree_list(&output.stdout)
        .into_iter()
        .find_map(|entry| {
            if entry.branch.as_deref() == Some(branch) {
                entry.worktree
            } else {
                None
            }
        }))
}

fn current_branch_for_worktree(
    worktree: &Path,
    runner: &dyn LaneSpawnCommandRunner,
    timeout: Duration,
) -> Result<String, String> {
    let spec = LaneSpawnCommandSpec {
        label: format!("inspect existing worktree {}", worktree.display()),
        program: String::from("git"),
        args: vec![String::from("branch"), String::from("--show-current")],
        cwd: worktree.to_path_buf(),
    };
    let output = run_checked_step(runner, timeout, &spec, &mut |_| {})?;
    let branch = output.stdout.trim();
    if branch.is_empty() {
        return Err(format!(
            "lane spawn: could not resolve branch for existing worktree {}",
            worktree.display()
        ));
    }
    Ok(branch.to_string())
}

fn parse_worktree_list(source: &str) -> Vec<ParsedWorktree> {
    let mut entries = Vec::new();
    let mut current = ParsedWorktree::default();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if current.worktree.is_some() {
                entries.push(current);
                current = ParsedWorktree::default();
            }
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("worktree ") {
            current.worktree = Some(PathBuf::from(value));
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("branch refs/heads/") {
            current.branch = Some(value.to_string());
        }
    }

    if current.worktree.is_some() {
        entries.push(current);
    }

    entries
}

#[derive(Debug, Default)]
struct ParsedWorktree {
    worktree: Option<PathBuf>,
    branch: Option<String>,
}

fn infer_branch_name(worktree: &Path) -> Result<String, String> {
    worktree
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "lane spawn: could not infer branch name from worktree path {}",
                worktree.display()
            )
        })
}

fn build_tmux_session_name(branch: &str) -> String {
    let slug = branch
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!(
        "{TMUX_SESSION_PREFIX}{}",
        if slug.is_empty() { "lane" } else { &slug }
    )
}

fn absolutize_from(base: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        base.join(value)
    }
}

fn first_non_empty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

const fn render_transport(transport: LaneSpawnTransport) -> &'static str {
    match transport {
        LaneSpawnTransport::Tmux => "tmux",
        LaneSpawnTransport::InProcess => "in_process",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone)]
    struct ExpectedCall {
        program: String,
        args: Vec<String>,
        timeout: Duration,
        output: LaneSpawnCommandOutput,
    }

    #[derive(Clone)]
    struct FakeRunner {
        calls: Arc<Mutex<Vec<LaneSpawnCommandSpec>>>,
        expected: Arc<Mutex<VecDeque<ExpectedCall>>>,
    }

    impl FakeRunner {
        fn new(expected: Vec<ExpectedCall>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                expected: Arc::new(Mutex::new(expected.into())),
            }
        }

        fn calls(&self) -> Vec<LaneSpawnCommandSpec> {
            self.calls.lock().expect("calls lock").clone()
        }
    }

    impl LaneSpawnCommandRunner for FakeRunner {
        fn run(
            &self,
            spec: &LaneSpawnCommandSpec,
            timeout: Duration,
        ) -> Result<LaneSpawnCommandOutput, String> {
            self.calls.lock().expect("calls lock").push(spec.clone());
            let expected = self
                .expected
                .lock()
                .expect("expected lock")
                .pop_front()
                .expect("unexpected command");
            assert_eq!(spec.program, expected.program);
            assert_eq!(spec.args, expected.args);
            assert_eq!(timeout, expected.timeout);
            Ok(expected.output)
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("runtime-lane-spawn-{label}-{nanos}"))
    }

    fn request(root: &Path, branch: &str) -> LaneSpawnRequest {
        let repo = root.join("repo");
        fs::create_dir_all(&repo).expect("repo dir should exist");
        let worktree = repo.join(".worktrees").join(branch);
        LaneSpawnRequest {
            repo,
            worktree,
            branch: branch.to_string(),
            session_name: build_tmux_session_name(branch),
        }
    }

    #[test]
    fn given_new_branch_when_spawning_then_worktree_and_tmux_session_are_created() {
        // given
        let root = temp_dir("new-branch");
        let request = request(&root, "feature-a");
        let runner = FakeRunner::new(vec![
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("list"),
                    String::from("--porcelain"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("show-ref"),
                    String::from("--verify"),
                    String::from("--quiet"),
                    String::from("refs/heads/feature-a"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("add"),
                    String::from("-b"),
                    String::from("feature-a"),
                    request.worktree.display().to_string(),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("tmux"),
                args: vec![
                    String::from("has-session"),
                    String::from("-t"),
                    String::from("claw-code-feature-a"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("tmux"),
                args: vec![
                    String::from("new-session"),
                    String::from("-d"),
                    String::from("-s"),
                    String::from("claw-code-feature-a"),
                    String::from("-c"),
                    request.worktree.display().to_string(),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
        ]);
        let mut logs = Vec::new();

        // when
        let result = spawn_lane_with_runner(
            &request,
            LaneSpawnTransport::Tmux,
            DEFAULT_LANE_SPAWN_TIMEOUT,
            &runner,
            &mut |message| logs.push(message.to_string()),
        )
        .expect("spawn should succeed");

        // then
        assert_eq!(result.mode, LaneSpawnMode::Created);
        assert!(result.created_worktree);
        assert!(result.created_session);
        assert_eq!(result.session_name.as_deref(), Some("claw-code-feature-a"));
        assert!(logs
            .iter()
            .any(|message| message.contains("create tmux session claw-code-feature-a")));
        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn given_existing_branch_worktree_and_session_when_spawning_then_existing_lane_is_attached() {
        // given
        let root = temp_dir("attach-existing");
        let request = request(&root, "feature-b");
        let existing_worktree = request.repo.join(".worktrees").join("feature-b-existing");
        let runner = FakeRunner::new(vec![
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("list"),
                    String::from("--porcelain"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: format!(
                        "worktree {}\nHEAD deadbee\nbranch refs/heads/feature-b\n\n",
                        existing_worktree.display()
                    ),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("tmux"),
                args: vec![
                    String::from("has-session"),
                    String::from("-t"),
                    String::from("claw-code-feature-b"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
        ]);

        // when
        let result = spawn_lane_with_runner(
            &request,
            LaneSpawnTransport::Tmux,
            DEFAULT_LANE_SPAWN_TIMEOUT,
            &runner,
            &mut |_| {},
        )
        .expect("spawn should succeed");

        // then
        assert_eq!(result.mode, LaneSpawnMode::Attached);
        assert_eq!(result.worktree, existing_worktree);
        assert!(!result.created_worktree);
        assert!(!result.created_session);
        assert_eq!(runner.calls().len(), 2);
        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn given_existing_branch_without_worktree_when_spawning_then_worktree_reuses_branch_without_dash_b(
    ) {
        // given
        let root = temp_dir("branch-reuse");
        let request = request(&root, "feature-c");
        let runner = FakeRunner::new(vec![
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("list"),
                    String::from("--porcelain"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("show-ref"),
                    String::from("--verify"),
                    String::from("--quiet"),
                    String::from("refs/heads/feature-c"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("add"),
                    request.worktree.display().to_string(),
                    String::from("feature-c"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("tmux"),
                args: vec![
                    String::from("has-session"),
                    String::from("-t"),
                    String::from("claw-code-feature-c"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("tmux"),
                args: vec![
                    String::from("new-session"),
                    String::from("-d"),
                    String::from("-s"),
                    String::from("claw-code-feature-c"),
                    String::from("-c"),
                    request.worktree.display().to_string(),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
        ]);

        // when
        let result = spawn_lane_with_runner(
            &request,
            LaneSpawnTransport::Tmux,
            DEFAULT_LANE_SPAWN_TIMEOUT,
            &runner,
            &mut |_| {},
        )
        .expect("spawn should succeed");

        // then
        assert_eq!(result.mode, LaneSpawnMode::Created);
        assert!(runner.calls().iter().any(|call| call.program == "git"
            && call.args
                == vec![
                    String::from("worktree"),
                    String::from("add"),
                    request.worktree.display().to_string(),
                    String::from("feature-c"),
                ]));
        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn given_conflicting_existing_worktree_when_spawning_then_actionable_error_is_returned() {
        // given
        let root = temp_dir("conflict");
        let request = request(&root, "feature-d");
        fs::create_dir_all(&request.worktree).expect("worktree dir should exist");
        let runner = FakeRunner::new(vec![
            ExpectedCall {
                program: String::from("git"),
                args: vec![
                    String::from("worktree"),
                    String::from("list"),
                    String::from("--porcelain"),
                ],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            },
            ExpectedCall {
                program: String::from("git"),
                args: vec![String::from("branch"), String::from("--show-current")],
                timeout: DEFAULT_LANE_SPAWN_TIMEOUT,
                output: LaneSpawnCommandOutput {
                    status_code: 0,
                    stdout: String::from("other-branch\n"),
                    stderr: String::new(),
                },
            },
        ]);

        // when
        let error = spawn_lane_with_runner(
            &request,
            LaneSpawnTransport::Tmux,
            DEFAULT_LANE_SPAWN_TIMEOUT,
            &runner,
            &mut |_| {},
        )
        .expect_err("spawn should fail");

        // then
        assert!(error.contains("already exists on branch other-branch"));
        assert!(error.contains("choose a different worktree path"));
        fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
