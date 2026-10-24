use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::document::DocumentKind;
use crate::model::Severity;
use crate::paths;

pub const DEFAULT_CONFIG_TOML: &str = r#"# Errator configuration (schema 1.0)
# All fields are optional. Delete this file to restore defaults.
# A project-local ./errator.toml takes precedence over ~/.errator/config.toml.

[general]
# Minimum severity shown and included in reports: error | warning | info
min_severity = "info"
# Include a short context string with each issue.
context = true
# Installed rulesets to load (see `errator ruleset list`).
rulesets = []

[rules]
# Rule ids to disable, for example ["CJK_SPACE_MISSING"].
disabled = []

# Global per-rule levels: error | warning | info | off | auto.
# The older [rules.severity] table is accepted as an alias.
[rules.severity_overrides]
# CJK_SPACE_MISSING = "auto"

# Override by file type (txt | md | docx). The docx default is applied when
# CJK_SPACE_MISSING is "auto".
# [rules.by_file_type.md]
# CJK_SPACE_MISSING = "warning"

# Override by path glob. Paths use forward slashes on every platform.
# Priority: by_path > by_file_type > severity_overrides > built-in default.
# [rules.by_path."docs/legacy/**"]
# PUNCT_HALF_WIDTH = "off"

# Declarative pair rule with context conditions.
# [[rules.custom]]
# id = "CUSTOM_LOGIN"
# category = "typo"
# severity = "warning"
# type = "pair"
# wrong = "登陆"
# right = "登录"
# message = "账号场景应使用「登录」"
# case_sensitive = false
# window = 8
# when_prev = ["账号", "系统"]
# include_excluded = false
# [[rules.custom.tests]]
# input = "系统登陆异常"
# expect = ["CUSTOM_LOGIN"]
# [[rules.custom.tests]]
# input = "船只登陆海滩"
# expect = []

# Regex rule. `replacement` is optional; omit it to report without a fix.
# [[rules.custom]]
# id = "CUSTOM_TODO"
# category = "format"
# severity = "info"
# type = "regex"
# pattern = 'TODO\((\w+)\)'
# replacement = "TODO($1)"
# message = "TODO 标记格式不规范"

# Dictionary section. [dict] is accepted as an alias.
[dictionary]
# Correct project terms; used to reduce false positives.
words = []
# Never report these terms.
ignore_words = []
# Terms that should never be reported (legacy spelling).
whitelist = []
# External dictionary files, relative to this config file.
files = []

# [[dictionary.extra]]
# wrong = "按装"
# right = "安装"

# [[dictionary.confusions]]
# wrong = "勘误师"
# right = "勘误执行者"
# severity = "info"

[consistency]
use_builtin = true
# groups = [["账号", "帐号"]]

# L2 external checker. Receives the JSON protocol on stdin, returns issues on
# stdout. Only explicitly configured commands are executed.
# [external.terminology]
# command = "python checkers/terminology.py"
# timeout_ms = 5000
# enabled = true
# cwd = "."
# [external.terminology.env]
# PROJECT = "docs"

[semantic]
# auto: enable when a model exists; on: require a model; off: disable
mode = "auto"
# auto | cpu | dml | coreml
device = "auto"
allow_gpu_fallback = true
# Optional explicit ONNX model path. When empty, the newest *.onnx in
# ~/.errator/models/ is used.
# model_path = "C:/Users/me/.errator/models/macbert4csc.onnx"
# Optional BERT WordPiece vocabulary path; defaults to vocab.txt next to model.
# vocab_path = "C:/Users/me/.errator/models/vocab.txt"
# v1 supports BERT WordPiece tokenizers (MacBERT, BERT-base-chinese).
# tokenizer = "bert-base-chinese"
# input_names = ["input_ids", "attention_mask", "token_type_ids"]
# output_name = "logits"
# min_confidence = 0.5
max_text_length = 400
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleLevel {
    Off,
    Auto,
    Error,
    Warning,
    Info,
}

impl RuleLevel {
    pub fn severity(self) -> Option<Severity> {
        match self {
            Self::Error => Some(Severity::Error),
            Self::Warning => Some(Severity::Warning),
            Self::Info => Some(Severity::Info),
            Self::Off | Self::Auto => None,
        }
    }

    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub rules: RuleSettings,
    #[serde(alias = "dict")]
    pub dictionary: DictionarySettings,
    pub consistency: ConsistencySettings,
    #[serde(default)]
    pub external: BTreeMap<String, ExternalCheckerConfig>,
    pub semantic: SemanticConfig,
    /// Directory containing the config file. Never serialized; used to resolve
    /// relative dictionary files and external checker working directories.
    #[serde(skip)]
    pub source_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            rules: RuleSettings::default(),
            dictionary: DictionarySettings::default(),
            consistency: ConsistencySettings::default(),
            external: BTreeMap::new(),
            semantic: SemanticConfig::default(),
            source_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub min_severity: Severity,
    pub context: bool,
    pub rulesets: Vec<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            min_severity: Severity::Info,
            context: true,
            rulesets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleSettings {
    pub disabled: Vec<String>,
    #[serde(alias = "severity")]
    pub severity_overrides: BTreeMap<String, RuleLevel>,
    #[serde(default)]
    pub by_file_type: BTreeMap<String, BTreeMap<String, RuleLevel>>,
    #[serde(default)]
    pub by_path: BTreeMap<String, BTreeMap<String, RuleLevel>>,
    #[serde(default)]
    pub custom: Vec<CustomRuleConfig>,
}

impl RuleSettings {
    /// Effective level for one rule/document. `None` means "use the rule's
    /// built-in default". Priority mirrors the extension contract:
    /// by_path > by_file_type > severity_overrides > built-in.
    pub fn effective_level(
        &self,
        rule_id: &str,
        kind: DocumentKind,
        display_path: &str,
    ) -> Option<RuleLevel> {
        let normalized = display_path.replace('\\', "/");
        for (pattern, overrides) in self.by_path.iter().rev() {
            if glob_match(pattern, &normalized)
                && let Some(level) = overrides.get(rule_id)
            {
                return Some(*level);
            }
        }
        for (file_type, overrides) in &self.by_file_type {
            if file_type_matches(file_type, kind)
                && let Some(level) = overrides.get(rule_id)
            {
                return Some(*level);
            }
        }
        self.severity_overrides.get(rule_id).copied()
    }
}

fn file_type_matches(value: &str, kind: DocumentKind) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "text" | "txt" => kind == DocumentKind::Text,
        "md" | "markdown" => kind == DocumentKind::Markdown,
        "docx" | "word" => kind == DocumentKind::Docx,
        _ => false,
    }
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.trim_start_matches("./");
    let mut regex = String::from("(?i)^");
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        regex.push_str("(?:.*/)?");
                    } else {
                        regex.push_str(".*");
                    }
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '/' => regex.push('/'),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex)
        .map(|compiled| compiled.is_match(path))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomRuleConfig {
    pub id: String,
    pub category: String,
    pub severity: Severity,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub message: String,
    pub enabled: bool,
    pub include_excluded: bool,
    // Regex form.
    pub pattern: String,
    pub replacement: String,
    // Pair form.
    pub wrong: String,
    pub right: String,
    pub case_sensitive: bool,
    pub when_prev: Vec<String>,
    pub when_next: Vec<String>,
    pub when_prev_regex: Vec<String>,
    pub when_next_regex: Vec<String>,
    pub window: usize,
    pub tests: Vec<RuleTest>,
}

impl Default for CustomRuleConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            category: "custom".to_owned(),
            severity: Severity::Warning,
            rule_type: String::new(),
            message: String::new(),
            enabled: true,
            include_excluded: false,
            pattern: String::new(),
            replacement: String::new(),
            wrong: String::new(),
            right: String::new(),
            case_sensitive: false,
            when_prev: Vec::new(),
            when_next: Vec::new(),
            when_prev_regex: Vec::new(),
            when_next_regex: Vec::new(),
            window: 8,
            tests: Vec::new(),
        }
    }
}

impl CustomRuleConfig {
    /// `type` is explicit in the extension spec, but older configs used
    /// pattern-only custom rules, so infer the form when it is absent.
    pub fn resolved_type(&self) -> &str {
        match self.rule_type.trim().to_ascii_lowercase().as_str() {
            "pair" => "pair",
            "regex" => "regex",
            "" if !self.wrong.is_empty() => "pair",
            "" if !self.pattern.is_empty() => "regex",
            "" => "unknown",
            _ => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct RuleTest {
    pub input: String,
    pub expect: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionarySettings {
    /// Correct project terms used to reduce false positives.
    pub words: Vec<String>,
    /// Terms that must never be reported.
    pub ignore_words: Vec<String>,
    pub whitelist: Vec<String>,
    pub extra: Vec<ConfusionConfig>,
    pub confusions: Vec<ConfusionConfig>,
    /// External dictionary files, resolved relative to the config file.
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfusionConfig {
    pub wrong: String,
    pub right: String,
    #[serde(default)]
    pub severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConsistencySettings {
    pub use_builtin: bool,
    pub groups: Vec<Vec<String>>,
}

impl Default for ConsistencySettings {
    fn default() -> Self {
        Self {
            use_builtin: true,
            groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalCheckerConfig {
    pub command: String,
    pub timeout_ms: u64,
    pub enabled: bool,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl Default for ExternalCheckerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            timeout_ms: 5000,
            enabled: true,
            env: BTreeMap::new(),
            cwd: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SemanticConfig {
    pub mode: String,
    pub device: String,
    pub allow_gpu_fallback: bool,
    pub model_path: Option<PathBuf>,
    pub vocab_path: Option<PathBuf>,
    pub tokenizer: String,
    pub input_names: Vec<String>,
    pub output_name: Option<String>,
    pub min_confidence: f32,
    pub max_text_length: usize,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            mode: "auto".to_owned(),
            device: "auto".to_owned(),
            allow_gpu_fallback: true,
            model_path: None,
            vocab_path: None,
            tokenizer: "bert-base-chinese".to_owned(),
            input_names: vec![
                "input_ids".to_owned(),
                "attention_mask".to_owned(),
                "token_type_ids".to_owned(),
            ],
            output_name: None,
            min_confidence: 0.5,
            max_text_length: 400,
        }
    }
}

impl Config {
    /// Load an explicit config path, or discover `./errator.toml`, falling back
    /// to `~/.errator/config.toml` when neither exists.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(path) => path.to_owned(),
            None => {
                let project = std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join("errator.toml"))
                    .filter(|path| path.is_file());
                match project {
                    Some(path) => path,
                    None => paths::config_path()?,
                }
            }
        };
        if !path.exists() {
            if explicit.is_some() {
                bail!("配置文件不存在：{}", path.display());
            }
            return Ok(Self::default());
        }
        Self::from_file(&path)
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("无法读取配置文件 {}", path.display()))?;
        Self::from_text(&text, path)
    }

    pub fn from_text(text: &str, path: &Path) -> Result<Self> {
        let mut config: Self =
            toml::from_str(text).with_context(|| format!("配置文件格式错误 {}", path.display()))?;
        config.source_dir = path
            .parent()
            .map(Path::to_owned)
            .unwrap_or_else(|| PathBuf::from("."));
        config.load_dictionary_files()?;
        for checker in config.external.values_mut() {
            if let Some(cwd) = &checker.cwd {
                checker.cwd = Some(resolve_relative(cwd, &config.source_dir)?);
            }
        }
        config.validate()?;
        Ok(config)
    }

    fn load_dictionary_files(&mut self) -> Result<()> {
        let files = std::mem::take(&mut self.dictionary.files);
        for file in files {
            let path = resolve_relative(&file, &self.source_dir)?;
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("无法读取词典文件 {}", path.display()))?;
            let loaded = parse_dictionary_file(&text)
                .with_context(|| format!("词典文件格式错误 {}", path.display()))?;
            self.dictionary.words.extend(loaded.words);
            self.dictionary.ignore_words.extend(loaded.ignore_words);
            self.dictionary.whitelist.extend(loaded.whitelist);
            self.dictionary.extra.extend(loaded.extra);
            self.dictionary.confusions.extend(loaded.confusions);
        }
        Ok(())
    }

    pub fn all_whitelist(&self) -> Vec<String> {
        self.dictionary
            .whitelist
            .iter()
            .chain(self.dictionary.words.iter())
            .chain(self.dictionary.ignore_words.iter())
            .map(|term| term.trim().to_owned())
            .filter(|term| !term.is_empty())
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        for custom in &self.rules.custom {
            if custom.id.trim().is_empty() {
                bail!("自定义规则缺少 id");
            }
            if !ids.insert(custom.id.clone()) {
                bail!("自定义规则 id 重复：{}", custom.id);
            }
            match custom.resolved_type() {
                "regex" => {
                    if custom.pattern.is_empty() {
                        bail!("自定义正则规则 `{}` 的 pattern 为空", custom.id);
                    }
                    Regex::new(&custom.pattern)
                        .with_context(|| format!("自定义规则 `{}` 的正则无效", custom.id))?;
                }
                "pair" => {
                    if custom.wrong.is_empty() || custom.right.is_empty() {
                        bail!("自定义词对规则 `{}` 需要 wrong 和 right", custom.id);
                    }
                }
                _ => bail!(
                    "自定义规则 `{}` 的 type 无效；可用 pair 或 regex",
                    custom.id
                ),
            }
            for pattern in custom
                .when_prev_regex
                .iter()
                .chain(custom.when_next_regex.iter())
            {
                Regex::new(pattern).with_context(|| {
                    format!("自定义规则 `{}` 的上下文正则无效：{pattern}", custom.id)
                })?;
            }
            for test in &custom.tests {
                if test.input.is_empty() {
                    bail!("自定义规则 `{}` 存在空测试输入", custom.id);
                }
            }
            if custom.window == 0 {
                bail!("自定义规则 `{}` 的 window 必须大于 0", custom.id);
            }
        }
        for (name, checker) in &self.external {
            if checker.command.trim().is_empty() {
                bail!("外部检查器 `{name}` 缺少 command");
            }
            if checker.timeout_ms == 0 {
                bail!("外部检查器 `{name}` 的 timeout_ms 必须大于 0");
            }
        }
        if self.semantic.max_text_length < 16 {
            bail!("semantic.max_text_length 不能小于 16");
        }
        if !(0.0..=1.0).contains(&self.semantic.min_confidence) {
            bail!("semantic.min_confidence 必须在 0 到 1 之间");
        }
        Ok(())
    }
}

fn parse_dictionary_file(text: &str) -> Result<DictionarySettings> {
    #[derive(Deserialize)]
    struct Wrapped {
        #[serde(default, alias = "dictionary")]
        dict: DictionarySettings,
    }
    if let Ok(wrapped) = toml::from_str::<Wrapped>(text) {
        // A `[dict]` wrapper is the documented format. If the wrapper is empty
        // and the document has root-level fields, fall through to root parsing.
        if !wrapped.dict.words.is_empty()
            || !wrapped.dict.confusions.is_empty()
            || !wrapped.dict.extra.is_empty()
            || !wrapped.dict.ignore_words.is_empty()
            || !wrapped.dict.whitelist.is_empty()
        {
            if !wrapped.dict.files.is_empty() {
                bail!("外部词典文件不能再引用 files");
            }
            return Ok(wrapped.dict);
        }
    }
    let root: DictionarySettings = toml::from_str(text)?;
    if !root.files.is_empty() {
        bail!("外部词典文件不能再引用 files");
    }
    Ok(root)
}

pub fn resolve_relative(path: &Path, base_dir: &Path) -> Result<PathBuf> {
    let expanded = expand_home(path)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(base_dir.join(expanded))
    }
}

pub fn expand_home(path: &Path) -> Result<PathBuf> {
    let text = path.to_string_lossy();
    if text == "~" {
        return dirs::home_dir().context("无法确定用户主目录");
    }
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return Ok(dirs::home_dir().context("无法确定用户主目录")?.join(rest));
    }
    Ok(path.to_owned())
}

/// Write the annotated default configuration. Used by `errator init`.
pub fn write_default_config(path: &Path, force: bool) -> Result<bool> {
    if path.exists() && !force {
        return Ok(false);
    }
    paths::ensure_parent(path)?;
    std::fs::write(path, DEFAULT_CONFIG_TOML)
        .with_context(|| format!("无法写入配置文件 {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_priority_and_double_star() {
        assert!(glob_match("docs/legacy/**", "docs/legacy/a.md"));
        assert!(glob_match("docs/**/a.md", "docs/a.md"));
        assert!(glob_match("**/*.md", "a.md"));
        assert!(glob_match("**/*.md", "docs/sub/a.md"));
        assert!(!glob_match("docs/*.md", "docs/sub/a.md"));
    }

    #[test]
    fn rule_level_priority() {
        let mut settings = RuleSettings::default();
        settings
            .severity_overrides
            .insert("R".to_owned(), RuleLevel::Info);
        settings.by_file_type.insert(
            "md".to_owned(),
            BTreeMap::from([("R".to_owned(), RuleLevel::Warning)]),
        );
        settings.by_path.insert(
            "docs/**".to_owned(),
            BTreeMap::from([("R".to_owned(), RuleLevel::Error)]),
        );
        assert_eq!(
            settings.effective_level("R", DocumentKind::Markdown, "docs/a.md"),
            Some(RuleLevel::Error)
        );
        assert_eq!(
            settings.effective_level("R", DocumentKind::Markdown, "notes/a.md"),
            Some(RuleLevel::Warning)
        );
        assert_eq!(
            settings.effective_level("R", DocumentKind::Text, "notes/a.txt"),
            Some(RuleLevel::Info)
        );
    }
}
