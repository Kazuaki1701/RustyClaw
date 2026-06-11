use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::{Child, ChildStdin, ChildStdout};

pub struct ExternalMcpController {
    child: Child,
    #[allow(dead_code)]
    workspace_root: PathBuf,
}

impl ExternalMcpController {
    /// context-mode 子プロセスを起動し stdin/stdout を返す。
    /// bun が見つかれば `bun <bundle>` で起動（bun:sqlite を使用）、
    /// なければ `context-mode` コマンドで起動（node が必要）。
    pub fn spawn(workspace_root: &Path) -> Result<(Self, ChildStdin, ChildStdout)> {
        let storage_dir = workspace_root.join(".context-mode");
        let (cmd, args) = resolve_context_mode_command();
        tracing::info!("context-mode 起動: {} {:?}", cmd, args);

        let mut child = tokio::process::Command::new(&cmd)
            .args(&args)
            .env("CONTEXT_MODE_DIR", storage_dir.to_string_lossy().as_ref())
            .env("CONTEXT_MODE_PLATFORM", "custom-rustyclaw")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| format!("ExternalMcpController: `{cmd}` の起動に失敗"))?;

        let stdin = child.stdin.take().context("stdin が取得できない")?;
        let stdout = child.stdout.take().context("stdout が取得できない")?;
        Ok((
            Self {
                child,
                workspace_root: workspace_root.to_path_buf(),
            },
            stdin,
            stdout,
        ))
    }

    /// 子プロセスの終了を待機する（プロセスが死んだことの検知に使用）。
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    #[allow(dead_code)]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

impl Drop for ExternalMcpController {
    fn drop(&mut self) {
        // SIGTERM を送って子プロセスをクリーンに終了させる
        let _ = self.child.start_kill();
    }
}

/// bun + bundle パスを自動検出する。
/// 優先順位: bun ($HOME/.bun/bin/bun) + npm global bundle > context-mode (PATH)
fn resolve_context_mode_command() -> (String, Vec<String>) {
    if let (Some(bun), Some(bundle)) = (find_bun(), find_context_mode_bundle()) {
        return (
            bun.to_string_lossy().into_owned(),
            vec![bundle.to_string_lossy().into_owned()],
        );
    }
    ("context-mode".to_string(), vec![])
}

/// $HOME/.bun/bin/bun を優先し、なければ PATH から bun を探す。
fn find_bun() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".bun/bin/bun");
        if p.exists() {
            return Some(p);
        }
    }
    // PATH fallback
    let Ok(out) = std::process::Command::new("which").arg("bun").output() else {
        return None;
    };
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// npm global root から context-mode/cli.bundle.mjs を探す。
fn find_context_mode_bundle() -> Option<PathBuf> {
    let Ok(out) = std::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
    else {
        return None;
    };
    if out.status.success() {
        let root = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
        let bundle = root.join("context-mode/cli.bundle.mjs");
        if bundle.exists() {
            return Some(bundle);
        }
    }
    None
}
