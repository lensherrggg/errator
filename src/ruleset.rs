use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesetHeader {
    pub name: String,
    pub version: String,
    pub description: String,
    pub extends: Vec<String>,
}

impl Default for RulesetHeader {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.1.0".to_owned(),
            description: String::new(),
            extends: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledRuleset {
    pub name: String,
    pub version: String,
    pub description: String,
    pub extends: Vec<String>,
    pub path: String,
}

#[derive(Debug, Deserialize)]
struct RulesetFileRoot {
    #[serde(default)]
    ruleset: Option<RulesetHeader>,
}

pub struct LoadedRulesets {
    pub warnings: Vec<String>,
    pub names: Vec<String>,
}

/// Load and merge rulesets into a base config. `names` may contain installed
/// names or explicit `.toml` paths. Dependencies from `extends` are loaded
/// first; later definitions override earlier ones and emit a warning.
pub fn apply(config: &mut Config, names: &[String]) -> Result<LoadedRulesets> {
    let mut warnings = Vec::new();
    let mut loaded = Vec::new();
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for name in names {
        load_one(
            config,
            name,
            &mut visiting,
            &mut done,
            &mut warnings,
            &mut loaded,
        )?;
    }
    Ok(LoadedRulesets {
        warnings,
        names: loaded,
    })
}

fn load_one(
    config: &mut Config,
    name: &str,
    visiting: &mut HashSet<String>,
    done: &mut HashSet<String>,
    warnings: &mut Vec<String>,
    loaded: &mut Vec<String>,
) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "base" {
        return Ok(());
    }
    let path = resolve_ruleset_path(trimmed)?;
    let key = path.to_string_lossy().to_string();
    if done.contains(&key) {
        return Ok(());
    }
    if !visiting.insert(key.clone()) {
        bail!("规则集循环依赖：{}", path.display());
    }

    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("无法读取规则集 {}", path.display()))?;
    let root: RulesetFileRoot =
        toml::from_str(&text).with_context(|| format!("规则集格式错误 {}", path.display()))?;
    let header = root.ruleset.unwrap_or_else(|| RulesetHeader {
        name: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| trimmed.to_owned()),
        ..RulesetHeader::default()
    });
    if header.name.trim().is_empty() {
        bail!("规则集 {} 缺少 ruleset.name", path.display());
    }

    for dependency in &header.extends {
        load_one(config, dependency, visiting, done, warnings, loaded)?;
    }

    let mut ruleset_config = Config::from_text(&text, &path)
        .with_context(|| format!("无法加载规则集 {}", path.display()))?;
    // Rule checks inherit the caller's config directory resolution for model
    // and external paths; ruleset-local relative paths were already resolved
    // by Config::from_text for dictionary files.
    ruleset_config.source_dir = path
        .parent()
        .map(Path::to_owned)
        .unwrap_or_else(|| PathBuf::from("."));
    merge_config(config, &ruleset_config, &header.name, warnings)?;
    visiting.remove(&key);
    done.insert(key);
    loaded.push(header.name);
    Ok(())
}

fn resolve_ruleset_path(name: &str) -> Result<PathBuf> {
    let direct = Path::new(name);
    if direct.is_file() {
        return Ok(direct.to_owned());
    }
    if !valid_name(name) {
        bail!("规则集名称不合法：{name}");
    }
    let path = paths::rulesets_dir()?.join(format!("{name}.toml"));
    if !path.is_file() {
        bail!("未安装规则集：{name}（可运行 errator ruleset list 查看，或先 ruleset add）");
    }
    Ok(path)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        && name != "."
        && name != ".."
}

fn merge_config(
    target: &mut Config,
    source: &Config,
    name: &str,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for disabled in &source.rules.disabled {
        if !target.rules.disabled.contains(disabled) {
            target.rules.disabled.push(disabled.clone());
        }
    }
    for (rule_id, level) in &source.rules.severity_overrides {
        if target
            .rules
            .severity_overrides
            .insert(rule_id.clone(), *level)
            .is_some()
        {
            warnings.push(format!("规则集 `{name}` 覆盖了 {rule_id} 的严重级别"));
        }
    }
    merge_level_maps(
        &mut target.rules.by_file_type,
        &source.rules.by_file_type,
        name,
        warnings,
    );
    merge_level_maps(
        &mut target.rules.by_path,
        &source.rules.by_path,
        name,
        warnings,
    );

    for custom in &source.rules.custom {
        if let Some(existing) = target
            .rules
            .custom
            .iter_mut()
            .find(|existing| existing.id == custom.id)
        {
            warnings.push(format!("规则集 `{name}` 覆盖了规则 {}", custom.id));
            *existing = custom.clone();
        } else {
            target.rules.custom.push(custom.clone());
        }
    }

    extend_unique(
        &mut target.dictionary.words,
        source.dictionary.words.iter().cloned(),
    );
    extend_unique(
        &mut target.dictionary.ignore_words,
        source.dictionary.ignore_words.iter().cloned(),
    );
    extend_unique(
        &mut target.dictionary.whitelist,
        source.dictionary.whitelist.iter().cloned(),
    );
    target
        .dictionary
        .extra
        .extend(source.dictionary.extra.iter().cloned());
    target
        .dictionary
        .confusions
        .extend(source.dictionary.confusions.iter().cloned());

    for group in &source.consistency.groups {
        if !target.consistency.groups.contains(group) {
            target.consistency.groups.push(group.clone());
        }
    }

    for (checker_name, checker) in &source.external {
        if target
            .external
            .insert(checker_name.clone(), checker.clone())
            .is_some()
        {
            warnings.push(format!("规则集 `{name}` 覆盖了外部检查器 {checker_name}"));
        }
    }

    target.validate()?;
    Ok(())
}

fn merge_level_maps(
    target: &mut std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, crate::config::RuleLevel>,
    >,
    source: &std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, crate::config::RuleLevel>,
    >,
    name: &str,
    warnings: &mut Vec<String>,
) {
    for (pattern, overrides) in source {
        let entry = target.entry(pattern.clone()).or_default();
        for (rule_id, level) in overrides {
            if entry.insert(rule_id.clone(), *level).is_some() {
                warnings.push(format!(
                    "规则集 `{name}` 覆盖了路径/类型 `{pattern}` 中 {rule_id} 的级别"
                ));
            }
        }
    }
}

fn extend_unique<T: PartialEq>(target: &mut Vec<T>, values: impl IntoIterator<Item = T>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

pub fn list_installed() -> Result<Vec<InstalledRuleset>> {
    let directory = paths::rulesets_dir()?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut rulesets = Vec::new();
    for entry in std::fs::read_dir(&directory)
        .with_context(|| format!("无法读取规则集目录 {}", directory.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("无法读取规则集 {}", path.display()))?;
        if let Ok(root) = toml::from_str::<RulesetFileRoot>(&text) {
            let header = root.ruleset.unwrap_or_else(|| RulesetHeader {
                name: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                ..RulesetHeader::default()
            });
            rulesets.push(InstalledRuleset {
                name: header.name,
                version: header.version,
                description: header.description,
                extends: header.extends,
                path: path.display().to_string(),
            });
        }
    }
    rulesets.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rulesets)
}

pub fn install(source: &Path, force: bool) -> Result<InstalledRuleset> {
    if !source.is_file() {
        bail!("规则集文件不存在：{}", source.display());
    }
    let text = std::fs::read_to_string(source)
        .with_context(|| format!("无法读取规则集 {}", source.display()))?;
    let root: RulesetFileRoot =
        toml::from_str(&text).with_context(|| format!("规则集格式错误 {}", source.display()))?;
    let header = root.ruleset.unwrap_or_else(|| RulesetHeader {
        name: source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ..RulesetHeader::default()
    });
    if !valid_name(&header.name) {
        bail!("规则集名称不合法：{}", header.name);
    }
    // Validate the content before installing it.
    Config::from_text(&text, source)?;
    let directory = paths::rulesets_dir()?;
    std::fs::create_dir_all(&directory)?;
    let target = directory.join(format!("{}.toml", header.name));
    if target.exists() && !force {
        bail!("规则集已安装：{}（使用 --force 覆盖）", header.name);
    }
    std::fs::write(&target, text)
        .with_context(|| format!("无法写入规则集 {}", target.display()))?;
    Ok(InstalledRuleset {
        name: header.name,
        version: header.version,
        description: header.description,
        extends: header.extends,
        path: target.display().to_string(),
    })
}

pub fn remove(name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        bail!("规则集名称不合法：{name}");
    }
    let path = paths::rulesets_dir()?.join(format!("{name}.toml"));
    if !path.exists() {
        bail!("未安装规则集：{name}");
    }
    std::fs::remove_file(&path).with_context(|| format!("无法删除规则集 {}", path.display()))?;
    Ok(path)
}
