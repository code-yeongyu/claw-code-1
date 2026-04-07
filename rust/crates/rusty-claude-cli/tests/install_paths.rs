use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn cargo_install_and_install_script_create_runnable_claw_binaries() {
    // given
    let repo_root = repo_root();
    let cargo_install_root = unique_temp_dir("cargo-install-root");
    let script_bin_dir = unique_temp_dir("install-script-bin");
    fs::create_dir_all(&cargo_install_root).expect("cargo install root should exist");
    fs::create_dir_all(&script_bin_dir).expect("script bin dir should exist");

    // when
    let cargo_install_output = Command::new("cargo")
        .current_dir(&repo_root)
        .args([
            "install",
            "--path",
            "rust/crates/rusty-claude-cli",
            "--root",
            cargo_install_root
                .to_str()
                .expect("utf8 cargo install root"),
        ])
        .output()
        .expect("cargo install should launch");
    let install_script_output = Command::new("sh")
        .current_dir(&repo_root)
        .arg("./install.sh")
        .args([
            "--bin-dir",
            script_bin_dir.to_str().expect("utf8 script bin dir"),
        ])
        .output()
        .expect("install.sh should launch");

    // then
    assert_success(&cargo_install_output);
    let cargo_installed_binary = cargo_install_root.join("bin").join("claw");
    assert!(
        cargo_installed_binary.exists(),
        "expected installed binary at {}",
        cargo_installed_binary.display()
    );
    assert_version_command_succeeds(&cargo_installed_binary);

    assert_success(&install_script_output);
    let script_installed_binary = script_bin_dir.join("claw");
    assert!(
        script_installed_binary.exists(),
        "expected installed binary at {}",
        script_installed_binary.display()
    );
    assert_version_command_succeeds(&script_installed_binary);

    fs::remove_dir_all(cargo_install_root).expect("cleanup cargo install root");
    fs::remove_dir_all(script_bin_dir).expect("cleanup script bin dir");
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root should exist")
        .to_path_buf()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_version_command_succeeds(binary_path: &Path) {
    let output = Command::new(binary_path)
        .arg("--version")
        .output()
        .expect("installed claw should launch");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_millis();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claw-install-paths-{label}-{}-{millis}-{counter}",
        std::process::id()
    ))
}
