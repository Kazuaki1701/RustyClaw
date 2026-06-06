use gray_matter::{Matter, engine::YAML};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// YAMLの allowed-tools を文字列（スペース区切り）と配列の両方に対応するためのデシリアライザ
fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct AllowedToolsVisitor;
    impl<'de> serde::de::Visitor<'de> for AllowedToolsVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or sequence of strings")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum RawAllowedTools {
                Single(String),
                List(Vec<String>),
            }

            match RawAllowedTools::deserialize(deserializer)? {
                RawAllowedTools::Single(s) => {
                    let tools = s
                        .split_whitespace()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>();
                    if tools.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(tools))
                    }
                }
                RawAllowedTools::List(l) => {
                    if l.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(l))
                    }
                }
            }
        }
    }
    deserializer.deserialize_option(AllowedToolsVisitor)
}

/// `SKILL.md` の先頭にある YAML フロントマターを表すメタデータ構造体
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(
        rename = "allowed-tools",
        default,
        deserialize_with = "deserialize_allowed_tools"
    )]
    pub allowed_tools: Option<Vec<String>>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// メモリ上にキャッシュされるスキルオブジェクト
#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub instructions: String, // SKILL.md の本文部分 (Markdown)
    pub path: PathBuf,        // [skill-name]/ ディレクトリの絶対パス
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
                let skill_name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let skill = parse_fallback_skill(
                    &content,
                    &skill_name,
                    &path.parent().unwrap().to_path_buf(),
                    &matter,
                );
                skills.push(skill);
            }
        }
    }
    skills
}

fn validate_manifest(manifest: &SkillManifest, dir_path: &Path) -> Result<(), String> {
    // 1. name の検証
    let name = &manifest.name;
    if name.is_empty() || name.len() > 64 {
        return Err(format!(
            "Skill name '{}' length must be between 1 and 64 characters",
            name
        ));
    }

    // 使用可能文字のチェック
    let mut prev_is_hyphen = false;
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_is_hyphen = false;
        } else if c == '-' {
            if i == 0 {
                return Err(format!("Skill name '{}' cannot start with a hyphen", name));
            }
            if prev_is_hyphen {
                return Err(format!(
                    "Skill name '{}' cannot contain consecutive hyphens",
                    name
                ));
            }
            prev_is_hyphen = true;
        } else {
            return Err(format!(
                "Skill name '{}' contains invalid character '{}' (only lowercase alphanumeric and hyphens are allowed)",
                name, c
            ));
        }
    }
    if name.ends_with('-') {
        return Err(format!("Skill name '{}' cannot end with a hyphen", name));
    }

    // 親ディレクトリ名との一致検証
    if let Some(dir_name) = dir_path.file_name().and_then(|n| n.to_str()) {
        if name != dir_name {
            return Err(format!(
                "Skill name '{}' does not match its parent directory name '{}'",
                name, dir_name
            ));
        }
    }

    // 2. description の検証
    if manifest.description.is_empty() || manifest.description.len() > 1024 {
        return Err(format!(
            "Skill description length must be between 1 and 1024 characters"
        ));
    }

    // 3. compatibility の検証
    if let Some(ref compat) = manifest.compatibility {
        if compat.is_empty() || compat.len() > 500 {
            return Err(format!(
                "Skill compatibility length must be between 1 and 500 characters"
            ));
        }
    }

    Ok(())
}

fn rewrite_relative_links(instructions: &str, skill_name: &str) -> String {
    let re = match regex::Regex::new(r"(!?\[[^\]]+\])\(([^)]+)\)") {
        Ok(r) => r,
        Err(_) => return instructions.to_string(),
    };

    re.replace_all(instructions, |caps: &regex::Captures| {
        let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let url = caps.get(2).map(|m| m.as_str()).unwrap_or("");

        if url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("file://")
            || url.starts_with("/")
            || url.starts_with("#")
        {
            format!("{}({})", prefix, url)
        } else {
            format!("{}(skills/{}/{})", prefix, skill_name, url)
        }
    })
    .into_owned()
}

/// 標準 SKILL.md ファイルのパース
fn parse_standard_skill(content: &str, dir_path: &Path, matter: &Matter<YAML>) -> Option<Skill> {
    let result = matter.parse(content);
    let raw_data = result.data?;
    let manifest: SkillManifest = match raw_data.deserialize() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Failed to deserialize skill manifest in {:?}: {}",
                dir_path,
                e
            );
            return None;
        }
    };

    if let Err(e) = validate_manifest(&manifest, dir_path) {
        tracing::warn!("Validation failed for skill in {:?}: {}", dir_path, e);
        return None;
    }

    Some(Skill {
        manifest,
        instructions: result.content,
        path: dir_path.to_path_buf(),
    })
}

/// 従来のフラットマークダウンを疑似 manifest にラップして下位互換
fn parse_fallback_skill(
    content: &str,
    file_name: &str,
    base_path: &Path,
    matter: &Matter<YAML>,
) -> Skill {
    // 既にYAMLが含まれているかチェック
    if let Some(skill) = parse_standard_skill(content, base_path, matter) {
        return skill;
    }

    // YAMLが含まれていないプレーンなマークダウンの場合
    let lines: Vec<&str> = content.lines().collect();
    let description = lines
        .iter()
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

    let mut dir_str = String::from("\n\n## Available Skills\n");
    dir_str.push_str(
        "Skills are NOT callable as tool names. \
         To execute a skill that has a script, call the `run_workspace_script` tool \
         with the script path shown below. Do NOT generate a tool call named after the skill itself.\n\n"
    );

    for skill in skills {
        // scripts/ 配下の .sh ファイルを列挙（ソート済み）
        let scripts_dir = skill.path.join("scripts");
        let mut script_paths: Vec<String> = std::fs::read_dir(&scripts_dir)
            .map(|rd| {
                let mut names: Vec<String> = rd
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("sh"))
                    .map(|e| {
                        format!(
                            "skills/{}/scripts/{}",
                            skill.manifest.name,
                            e.file_name().to_string_lossy()
                        )
                    })
                    .collect();
                names.sort();
                names
            })
            .unwrap_or_default();

        if script_paths.is_empty() {
            // スクリプトなし: LLM が直接処理するタイプ
            dir_str.push_str(&format!(
                "- **{}**: {} (instruction-based, no script)\n",
                skill.manifest.name, skill.manifest.description
            ));
        } else {
            dir_str.push_str(&format!(
                "- **{}**: {}\n",
                skill.manifest.name, skill.manifest.description
            ));
            for path in &script_paths {
                dir_str.push_str(&format!("  → run_workspace_script: \"{}\"\n", path));
            }
        }
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
            tracing::info!(
                "Activation: Dynamic loading of skill '{}' into prompt",
                skill.manifest.name
            );
            injected_instructions.push_str(&format!(
                "\n\n--- [ACTIVE SKILL: {}] ---\n{}\n",
                skill.manifest.name,
                rewrite_relative_links(skill.instructions.trim(), &skill.manifest.name)
            ));
        }
    }

    // 元のプロンプトに Discovery と Activation を付与して返す
    let mut final_content = content.to_string();
    if !skills_directory.is_empty() {
        final_content = format!("{}{}", final_content, skills_directory);
    }
    if !injected_instructions.is_empty() {
        final_content = format!(
            "{}\n\n---\n\n{}",
            injected_instructions.trim(),
            final_content
        );
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
        assert_eq!(
            skill.manifest.description,
            "This is a fallback skill description."
        );
        assert!(skill.instructions.contains("More details here."));
    }

    #[test]
    fn test_parse_standard_skill() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("standard-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let content = "---\nname: standard-skill\ndescription: A standard skill description.\nallowed-tools:\n  - run_workspace_script\n---\n# Instructions\nFollow these steps.";
        let matter = Matter::<YAML>::new();
        let skill = parse_standard_skill(content, &skill_dir, &matter).unwrap();

        assert_eq!(skill.manifest.name, "standard-skill");
        assert_eq!(skill.manifest.description, "A standard skill description.");
        assert_eq!(
            skill.manifest.allowed_tools.unwrap(),
            vec!["run_workspace_script".to_string()]
        );
        assert_eq!(
            skill.instructions.trim(),
            "# Instructions\nFollow these steps."
        );
    }

    #[test]
    fn test_parse_standard_skill_space_separated_tools() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("space-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let content = "---\nname: space-skill\ndescription: Space description.\nallowed-tools: tool1 tool2 tool3\n---\n# Instructions";
        let matter = Matter::<YAML>::new();
        let skill = parse_standard_skill(content, &skill_dir, &matter).unwrap();

        assert_eq!(
            skill.manifest.allowed_tools.unwrap(),
            vec![
                "tool1".to_string(),
                "tool2".to_string(),
                "tool3".to_string()
            ]
        );
    }

    #[test]
    fn test_parse_standard_skill_validation_fails() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("standard-skill"); // name mismatch
        std::fs::create_dir_all(&skill_dir).unwrap();

        let matter = Matter::<YAML>::new();

        // ケースA: ディレクトリ名不一致
        let content_mismatch = "---\nname: other-name\ndescription: Valid desc.\n---";
        assert!(parse_standard_skill(content_mismatch, &skill_dir, &matter).is_none());

        // ケースB: 大文字含む名前
        let skill_dir_capital = dir.path().join("CapitalSkill");
        std::fs::create_dir_all(&skill_dir_capital).unwrap();
        let content_capital = "---\nname: CapitalSkill\ndescription: Valid desc.\n---";
        assert!(parse_standard_skill(content_capital, &skill_dir_capital, &matter).is_none());

        // ケースC: 連続ハイフン
        let skill_dir_hyphens = dir.path().join("foo--bar");
        std::fs::create_dir_all(&skill_dir_hyphens).unwrap();
        let content_hyphens = "---\nname: foo--bar\ndescription: Valid desc.\n---";
        assert!(parse_standard_skill(content_hyphens, &skill_dir_hyphens, &matter).is_none());
    }

    #[test]
    fn test_rewrite_relative_links() {
        let text = "Here is [diagram](references/diagram.png) and ![icon](images/icon.jpg) but not [Google](https://google.com) or [absolute](/path/to/file) or [anchor](#top).";
        let rewritten = rewrite_relative_links(text, "my-skill");
        assert_eq!(
            rewritten,
            "Here is [diagram](skills/my-skill/references/diagram.png) and ![icon](skills/my-skill/images/icon.jpg) but not [Google](https://google.com) or [absolute](/path/to/file) or [anchor](#top)."
        );
    }

    #[test]
    fn test_inject_skill_dynamic_activation() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        // 1. 標準スキルディレクトリの作成
        let vitals_dir = skills_dir.join("vitals-coach");
        std::fs::create_dir_all(&vitals_dir).unwrap();
        std::fs::write(
            vitals_dir.join("SKILL.md"),
            "---\nname: vitals-coach\ndescription: Garmin coach.\n---\n# Garmin instructions",
        )
        .unwrap();

        // 2. 従来互換フラットスキルの作成
        std::fs::write(
            skills_dir.join("topic-patrol.md"),
            "# Topic Patrol\nPatrol description.",
        )
        .unwrap();

        // テストA: トリガーワードが含まれていない場合 (Discoveryのみ追加される)
        let prompt = "How is the weather today?";
        let result = inject_skill_content(dir.path(), prompt);
        assert!(result.contains("Available Skills"));
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
