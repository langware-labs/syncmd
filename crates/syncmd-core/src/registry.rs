//! The asset registry: declarative rules describing equivalence classes. Ships built-in
//! defaults and optionally reads `syncmd.toml` at the repo root to extend/override.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::model::{AssetType, Strategy};

/// Whether a rule's members are single files or folder-backed (a main file inside a folder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    File,
    Folder,
}

/// One harness's path pattern within a rule. May contain a single `{name}` wildcard.
#[derive(Debug, Clone)]
pub struct MountRule {
    pub harness: String,
    pub pattern: String,
}

impl MountRule {
    /// Whether this pattern is templated (contains `{name}`).
    pub fn templated(&self) -> bool {
        self.pattern.contains("{name}")
    }
}

/// A single equivalence-class rule.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Group name template, e.g. `instructions` or `skill:{name}`. Registry/canonical order is
    /// the order of `mounts` (tie-break only).
    pub group: String,
    pub asset_type: AssetType,
    pub layout: Layout,
    pub strategy: Strategy,
    pub mounts: Vec<MountRule>,
}

impl Rule {
    /// Whether this rule expands per discovered `{name}`.
    pub fn templated(&self) -> bool {
        self.mounts.iter().any(|m| m.templated())
    }
}

/// The full registry.
#[derive(Debug, Clone)]
pub struct Registry {
    pub rules: Vec<Rule>,
}

impl Registry {
    /// Built-in defaults: instructions (fixed files), skills + specs (folder), agents (file).
    pub fn builtin_defaults() -> Registry {
        let m = |h: &str, p: &str| MountRule {
            harness: h.into(),
            pattern: p.into(),
        };
        Registry {
            rules: vec![
                Rule {
                    group: "instructions".into(),
                    asset_type: AssetType::ClaudeMd,
                    layout: Layout::File,
                    strategy: Strategy::Newest,
                    mounts: vec![
                        m("agents", "AGENTS.md"),
                        m("claude", "CLAUDE.md"),
                        m("copilot", ".github/copilot-instructions.md"),
                        m("cursor", ".cursorrules"),
                        m("cursor-rules", ".cursor/rules/project.mdc"),
                        m("gemini", "GEMINI.md"),
                        m("windsurf", ".windsurf/rules/rules.md"),
                        m("cline", ".clinerules"),
                        m("roo", ".roo/rules/rules.md"),
                        m("junie", ".junie/guidelines.md"),
                        m("amazonq", ".amazonq/rules/project.md"),
                        m("kiro", ".kiro/steering/project.md"),
                        m("goose", ".goosehints"),
                        m("warp", "WARP.md"),
                        m("qwen", "QWEN.md"),
                        m("aider", "CONVENTIONS.md"),
                        m("openhands", ".openhands/microagents/repo.md"),
                        m("augment", ".augment-guidelines"),
                        m("replit", "replit.md"),
                    ],
                },
                Rule {
                    group: "skill:{name}".into(),
                    asset_type: AssetType::Skill,
                    layout: Layout::Folder,
                    strategy: Strategy::Newest,
                    mounts: vec![
                        m("claude", ".claude/skills/{name}/SKILL.md"),
                        m("agents", ".agents/skills/{name}/SKILL.md"),
                        m("github", ".github/skills/{name}/SKILL.md"),
                    ],
                },
                Rule {
                    group: "agent:{name}".into(),
                    asset_type: AssetType::Agent,
                    layout: Layout::File,
                    strategy: Strategy::Newest,
                    mounts: vec![
                        m("claude", ".claude/agents/{name}.md"),
                        m("agents", ".agents/agents/{name}.md"),
                    ],
                },
                Rule {
                    group: "spec:{name}".into(),
                    asset_type: AssetType::Spec,
                    layout: Layout::Folder,
                    strategy: Strategy::Newest,
                    mounts: vec![
                        m("claude", ".claude/specs/{name}/spec.md"),
                        m("agents", ".agents/specs/{name}/spec.md"),
                    ],
                },
            ],
        }
    }

    /// All distinct harness labels in the registry, sorted.
    pub fn known_harnesses(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .rules
            .iter()
            .flat_map(|r| r.mounts.iter().map(|m| m.harness.clone()))
            .collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Restrict the registry to the given harness labels (case-insensitive).
    ///
    /// Unknown labels are an error listing the known set. Rules left with no
    /// mounts are dropped; rules left with a single mount are kept (they plan
    /// as no-ops, which keeps reporting consistent).
    pub fn filtered(mut self, formats: &[String]) -> Result<Registry> {
        let known = self.known_harnesses();
        let wanted: Vec<String> = formats.iter().map(|f| f.trim().to_lowercase()).collect();
        for w in &wanted {
            if !known.iter().any(|k| k.eq_ignore_ascii_case(w)) {
                return Err(Error::BadConfig(format!(
                    "unknown format '{w}' — known formats: {}",
                    known.join(", ")
                )));
            }
        }
        for rule in &mut self.rules {
            rule.mounts
                .retain(|m| wanted.iter().any(|w| m.harness.eq_ignore_ascii_case(w)));
        }
        self.rules.retain(|r| !r.mounts.is_empty());
        Ok(self)
    }

    /// Load `syncmd.toml` from `repo_root` if present, else the built-in defaults.
    pub fn load(repo_root: &Path) -> Result<Registry> {
        let path = repo_root.join("syncmd.toml");
        if !path.exists() {
            return Ok(Registry::builtin_defaults());
        }
        let text = std::fs::read_to_string(&path)?;
        let file: RegistryFile =
            toml::from_str(&text).map_err(|e| Error::BadConfig(e.to_string()))?;
        file.into_registry()
    }
}

// ---- syncmd.toml deserialization ----

#[derive(Debug, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    rule: Vec<RuleFile>,
}

#[derive(Debug, Deserialize)]
struct RuleFile {
    group: String,
    #[serde(rename = "type")]
    type_: String,
    layout: String,
    #[serde(default = "default_strategy")]
    strategy: String,
    mounts: Vec<MountFile>,
}

#[derive(Debug, Deserialize)]
struct MountFile {
    harness: String,
    pattern: String,
}

fn default_strategy() -> String {
    "newest".into()
}

impl RegistryFile {
    fn into_registry(self) -> Result<Registry> {
        let mut rules = Vec::new();
        for r in self.rule {
            rules.push(Rule {
                group: r.group,
                asset_type: parse_type(&r.type_)?,
                layout: parse_layout(&r.layout)?,
                strategy: parse_strategy(&r.strategy)?,
                mounts: r
                    .mounts
                    .into_iter()
                    .map(|m| MountRule {
                        harness: m.harness,
                        pattern: m.pattern,
                    })
                    .collect(),
            });
        }
        Ok(Registry { rules })
    }
}

fn parse_type(s: &str) -> Result<AssetType> {
    Ok(match s {
        "skill" => AssetType::Skill,
        "agent" => AssetType::Agent,
        "markdown" => AssetType::Markdown,
        "spec" => AssetType::Spec,
        "claude_md" => AssetType::ClaudeMd,
        other => return Err(Error::BadConfig(format!("unknown asset type: {other}"))),
    })
}

fn parse_layout(s: &str) -> Result<Layout> {
    Ok(match s {
        "file" => Layout::File,
        "folder" => Layout::Folder,
        other => return Err(Error::BadConfig(format!("unknown layout: {other}"))),
    })
}

fn parse_strategy(s: &str) -> Result<Strategy> {
    Ok(match s {
        "newest" => Strategy::Newest,
        "error" => Strategy::Error,
        "interactive" => Strategy::Interactive,
        other => return Err(Error::BadConfig(format!("unknown strategy: {other}"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_four_rules() {
        let r = Registry::builtin_defaults();
        assert_eq!(r.rules.len(), 4);
        assert!(r.rules.iter().any(|r| r.group == "instructions" && !r.templated()));
        assert!(r.rules.iter().any(|r| r.group == "skill:{name}" && r.templated()));
    }

    #[test]
    fn parse_toml_overrides() {
        let toml = r#"
            [[rule]]
            group = "instructions"
            type = "claude_md"
            layout = "file"
            mounts = [
              { harness = "claude", pattern = "CLAUDE.md" },
              { harness = "agents", pattern = "AGENTS.md" },
            ]
        "#;
        let file: RegistryFile = toml::from_str(toml).unwrap();
        let reg = file.into_registry().unwrap();
        assert_eq!(reg.rules.len(), 1);
        assert_eq!(reg.rules[0].strategy, Strategy::Newest);
        assert_eq!(reg.rules[0].mounts.len(), 2);
    }
}
