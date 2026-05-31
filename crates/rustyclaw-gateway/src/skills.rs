use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use gray_matter::{Matter, engine::YAML};

/// `SKILL.md` の先頭にある YAML フロントマターを表すメタデータ構造体
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// メモリ上にキャッシュされるスキルオブジェクト
#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub instructions: String,    // SKILL.md の本文部分 (Markdown)
    pub path: PathBuf,           // [skill-name]/ ディレクトリの絶対パス
}

/// workspace/skills/ 配下から標準スキルおよび互換スキルをロードする
pub fn load_skills(workspace_path: &Path) -> Vec<Skill> {
    let skills_dir = workspace_path.join("skills");
    let mut skills = Vec::new();

    if !skills_dir.exists() {
        return skills;
    }

    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return skills;
    };

    let matter = Matter::<YAML>::new();

    for entry in entries.flatten() {
        let path = entry.path();
        
        // パターン1: ディレクトリ構造 [skill-name]/SKILL.md
        if path.is_dir() {
            let skill_md_path = path.join("SKILL.md");
            if skill_md_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&skill_md_path) {
                    if let Some(skill) = parse_standard_skill(&content, &path, &matter) {
                        skills.push(skill);
                        continue;
                    }
                }
            }
        }
        
        // パターン2: 従来互換フラットファイル [skill-name].md (フォールバック)
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let skill_name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let skill = parse_fallback_skill(&content, &skill_name, &path.parent().unwrap().to_path_buf(), &matter);
                skills.push(skill);
            }
        }
    }
    skills
}

/// 標準 SKILL.md ファイルのパース
fn parse_standard_skill(content: &str, dir_path: &Path, matter: &Matter<YAML>) -> Option<Skill> {
    let result = matter.parse(content);
    let manifest: SkillManifest = result.data?.deserialize().ok()?;
    
    Some(Skill {
        manifest,
        instructions: result.content,
        path: dir_path.to_path_buf(),
    })
}

/// 従来のフラットマークダウンを疑似 manifest にラップして下位互換
fn parse_fallback_skill(content: &str, file_name: &str, base_path: &Path, matter: &Matter<YAML>) -> Skill {
    // 既にYAMLが含まれているかチェック
    if let Some(skill) = parse_standard_skill(content, base_path, matter) {
        return skill;
    }

    // YAMLが含まれていないプレーンなマークダウンの場合
    let lines: Vec<&str> = content.lines().collect();
    let description = lines.iter()
        .find(|l| !l.is_empty() && !l.starts_with("#"))
        .copied()
        .unwrap_or("RustyClaw Fallback Skill")
        .to_string();

    Skill {
        manifest: SkillManifest {
            name: file_name.to_lowercase(),
            description,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,
        },
        instructions: content.to_string(),
        path: base_path.join(file_name),
    }
}

/// LLMのシステムプロンプトにインジェクトするための Skills Directory (発見情報) を生成
pub fn generate_skills_directory(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut dir_str = String::from("\n\n## 🛠️ Available Agent Skills (Discovery)\n");
    dir_str.push_str("You have access to the following specialized capabilities. To activate detailed instructions for a skill, include the skill's identifier (e.g. `[use-skill: vitals-coach]`) in your internal chain of thought.\n\n");

    for skill in skills {
        dir_str.push_str(&format!(
            "- **`{}`**: {}\n",
            skill.manifest.name,
            skill.manifest.description
        ));
    }
    dir_str
}

/// ゲートウェイ L530 で呼ばれるメインエントリーポイント。
/// 新しい段階的開示（Discovery & Activation）をハイブリッドに適用する。
pub fn inject_skill_content(workspace_path: &Path, content: &str) -> String {
    let skills = load_skills(workspace_path);
    if skills.is_empty() {
        return content.to_string();
    }

    // 1. Discovery (レベル1) の自動構築
    let skills_directory = generate_skills_directory(&skills);

    // 2. Activation (レベル2) の動的ロード
    let search_target = content.to_lowercase();
    let mut injected_instructions = String::new();

    for skill in &skills {
        let trigger_tag = format!("use-skill: {}", skill.manifest.name);
        let name_match = format!("skill:{}", skill.manifest.name);
        
        if search_target.contains(&trigger_tag) 
            || search_target.contains(&name_match) 
            || search_target.contains(&skill.manifest.name) 
        {
            tracing::info!("Activation: Dynamic loading of skill '{}' into prompt", skill.manifest.name);
            injected_instructions.push_str(&format!(
                "\n\n--- [ACTIVE SKILL: {}] ---\n{}\n",
                skill.manifest.name,
                skill.instructions.trim()
            ));
        }
    }

    // 元のプロンプトに Discovery と Activation を付与して返す
    let mut final_content = content.to_string();
    if !skills_directory.is_empty() {
        final_content = format!("{}{}", final_content, skills_directory);
    }
    if !injected_instructions.is_empty() {
        final_content = format!("{}\n\n---\n\n{}", injected_instructions.trim(), final_content);
    }
    final_content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fallback_skill() {
        let dir = tempfile::tempdir().unwrap();
        let content = "# Fallback Skill\nThis is a fallback skill description.\nMore details here.";
        let matter = Matter::<YAML>::new();
        let skill = parse_fallback_skill(content, "test-skill", dir.path(), &matter);
        
        assert_eq!(skill.manifest.name, "test-skill");
        assert_eq!(skill.manifest.description, "This is a fallback skill description.");
        assert!(skill.instructions.contains("More details here."));
    }

    #[test]
    fn test_parse_standard_skill() {
        let dir = tempfile::tempdir().unwrap();
        let content = "---\nname: standard-skill\ndescription: A standard skill description.\nallowed-tools:\n  - run_workspace_script\n---\n# Instructions\nFollow these steps.";
        let matter = Matter::<YAML>::new();
        let skill = parse_standard_skill(content, dir.path(), &matter).unwrap();
        
        assert_eq!(skill.manifest.name, "standard-skill");
        assert_eq!(skill.manifest.description, "A standard skill description.");
        assert_eq!(skill.manifest.allowed_tools.unwrap(), vec!["run_workspace_script".to_string()]);
        assert_eq!(skill.instructions.trim(), "# Instructions\nFollow these steps.");
    }

    #[test]
    fn test_inject_skill_dynamic_activation() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        // 1. 標準スキルディレクトリの作成
        let vitals_dir = skills_dir.join("vitals-coach");
        std::fs::create_dir_all(&vitals_dir).unwrap();
        std::fs::write(vitals_dir.join("SKILL.md"), "---\nname: vitals-coach\ndescription: Garmin coach.\n---\n# Garmin instructions").unwrap();

        // 2. 従来互換フラットスキルの作成
        std::fs::write(skills_dir.join("topic-patrol.md"), "# Topic Patrol\nPatrol description.").unwrap();

        // テストA: トリガーワードが含まれていない場合 (Discoveryのみ追加される)
        let prompt = "How is the weather today?";
        let result = inject_skill_content(dir.path(), prompt);
        assert!(result.contains("Available Agent Skills"));
        assert!(result.contains("vitals-coach"));
        assert!(result.contains("topic-patrol"));
        assert!(!result.contains("Garmin instructions"));
        assert!(!result.contains("# Topic Patrol"));

        // テストB: スキル名が含まれている場合 (Activationされ本文がマージされる)
        let prompt_vitals = "I want to check my vitals-coach data.";
        let result_vitals = inject_skill_content(dir.path(), prompt_vitals);
        assert!(result_vitals.contains("ACTIVE SKILL: vitals-coach"));
        assert!(result_vitals.contains("Garmin instructions"));

        // テストC: トリガータグが含まれている場合 (Activationされ本文がマージされる)
        let prompt_patrol = "Include [use-skill: topic-patrol] in your run.";
        let result_patrol = inject_skill_content(dir.path(), prompt_patrol);
        assert!(result_patrol.contains("ACTIVE SKILL: topic-patrol"));
        assert!(result_patrol.contains("Patrol description"));
    }
}
