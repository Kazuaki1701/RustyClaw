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
    /// 呼び出し元は返された (stdin, stdout) を McpClientHandler.connect() に渡す。
    pub fn spawn(workspace_root: &Path) -> Result<(Self, ChildStdin, ChildStdout)> {
        let storage_dir = workspace_root.join(".context-mode");
        let mut child = tokio::process::Command::new("context-mode")
            .env("CONTEXT_MODE_DIR", storage_dir.to_string_lossy().as_ref())
            .env("CONTEXT_MODE_PLATFORM", "custom-rustyclaw")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .context("ExternalMcpController: context-mode の起動に失敗")?;
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
