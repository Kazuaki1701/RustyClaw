use anyhow::Result;
use rig_core::completion::Message;
use std::path::{Path, PathBuf};

pub const SUMMARY_INTERVAL: u32 = 5;

pub struct ChatSession {
    pub raw_history: Vec<(String, String)>,
    pub recent_messages: Vec<Message>,
    pub current_summary: String,
    pub counter: u32,
    pub summary_path: PathBuf,
}

impl ChatSession {
    pub fn load(workspace_dir: &Path) -> Result<Self> {
        let summary_path = workspace_dir.join("summary.md");
        let current_summary = if summary_path.exists() {
            std::fs::read_to_string(&summary_path)?
        } else {
            String::new()
        };
        Ok(Self {
            raw_history: Vec::new(),
            recent_messages: Vec::new(),
            current_summary,
            counter: 0,
            summary_path,
        })
    }

    pub fn persist_summary(&self) -> Result<()> {
        if let Some(parent) = self.summary_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.summary_path, &self.current_summary)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tempdir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn load_creates_empty_session_when_no_file() {
        let dir = tempdir();
        let session = ChatSession::load(dir.path()).unwrap();
        assert!(session.current_summary.is_empty());
        assert_eq!(session.counter, 0);
        assert!(session.recent_messages.is_empty());
        assert!(session.raw_history.is_empty());
    }

    #[test]
    fn load_reads_existing_summary() {
        let dir = tempdir();
        fs::write(dir.path().join("summary.md"), "これまでの要約です").unwrap();
        let session = ChatSession::load(dir.path()).unwrap();
        assert_eq!(session.current_summary, "これまでの要約です");
    }

    #[test]
    fn persist_summary_creates_file() {
        let dir = tempdir();
        let mut session = ChatSession::load(dir.path()).unwrap();
        session.current_summary = "新しい要約".to_string();
        session.persist_summary().unwrap();
        let content = fs::read_to_string(dir.path().join("summary.md")).unwrap();
        assert_eq!(content, "新しい要約");
    }

    #[test]
    fn persist_summary_creates_parent_dirs() {
        let dir = tempdir();
        let nested = dir.path().join("sub").join("dir");
        let mut session = ChatSession::load(&nested).unwrap();
        session.current_summary = "ネスト".to_string();
        session.persist_summary().unwrap();
        let content = fs::read_to_string(nested.join("summary.md")).unwrap();
        assert_eq!(content, "ネスト");
    }
}
