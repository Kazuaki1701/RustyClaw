use std::path::Path;

/// workspace/skills/ 配下のスキルファイルを content に注入する。
/// ファイル名（拡張子なし）が content 中に現れたスキルを前置する。
/// スキルが見つからない場合は content をそのまま返す。
pub fn inject_skill_content(workspace_path: &Path, content: &str) -> String {
    let skills_dir = workspace_path.join("skills");
    if !skills_dir.exists() {
        return content.to_string();
    }
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return content.to_string();
    };
    let lower = content.to_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let skill_name = path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
        if lower.contains(skill_name.as_str()) {
            if let Ok(skill_md) = std::fs::read_to_string(&path) {
                tracing::info!("Skills: injecting '{}' into prompt", skill_name);
                return format!("{}\n\n---\n\n{}", skill_md.trim(), content);
            }
        }
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_skill_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(skills_dir.join("topic-patrol.md"), "# Topic Patrol\nDo the patrol.").unwrap();

        let result = inject_skill_content(dir.path(), "Run the topic-patrol skill.");
        assert!(result.contains("# Topic Patrol"));
        assert!(result.contains("Run the topic-patrol skill."));
        assert!(result.find("# Topic Patrol").unwrap() < result.find("Run the").unwrap());
    }

    #[test]
    fn test_no_inject_when_skill_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let content = "Run the unknown-skill.";
        let result = inject_skill_content(dir.path(), content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_no_inject_when_skills_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Run the topic-patrol skill.";
        let result = inject_skill_content(dir.path(), content);
        assert_eq!(result, content);
    }
}
