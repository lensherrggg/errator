use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "1.0";

/// Issue severity. `Error` sorts first so it is the highest-priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    /// Lower rank means more severe. Useful for threshold comparisons.
    pub fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    /// Whether an issue at this severity should be shown when the configured
    /// minimum is `minimum`.
    pub fn passes(self, minimum: Severity) -> bool {
        self.rank() <= minimum.rank()
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl FromStr for Severity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" | "err" => Ok(Self::Error),
            "warning" | "warn" => Ok(Self::Warning),
            "info" | "information" => Ok(Self::Info),
            other => Err(format!(
                "未知严重级别 `{other}`（可用：error / warning / info）"
            )),
        }
    }
}

/// Where a finding came from. Kept deliberately small: new backends should
/// still report either deterministic rules/dictionaries or a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Rule,
    Dictionary,
    Model,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Dictionary => "dictionary",
            Self::Model => "model",
        }
    }
}

/// Source-specific location. Plain text uses 1-based line/column (columns are
/// Unicode scalar values); docx uses 1-based paragraph/character offsets and
/// never fabricates line/column numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Location {
    Text {
        line: usize,
        column: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
    },
    Docx {
        paragraph: usize,
        char_offset: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
    },
}

impl Location {
    pub fn context(&self) -> Option<&str> {
        match self {
            Self::Text { context, .. } | Self::Docx { context, .. } => context.as_deref(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Docx { .. } => "docx",
        }
    }

    /// Human-facing location only (without the `（text）` suffix).
    pub fn short(&self) -> String {
        match self {
            Self::Text { line, column, .. } => format!("{line}:{column}"),
            Self::Docx {
                paragraph,
                char_offset,
                ..
            } => format!("第 {paragraph} 段 · 第 {char_offset} 字符"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub rule_id: String,
    pub category: String,
    pub severity: Severity,
    pub location: Location,
    /// Present only when a report contains more than one file (directory
    /// checks). Single-file reports stay exactly schema 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub original: String,
    pub suggestion: String,
    pub message: String,
    pub source: Source,
    pub confidence: f32,
    /// Internal 0-based byte offset used for stable sorting and TUI navigation.
    /// Deliberately excluded from the JSON contract.
    #[serde(skip)]
    pub offset: usize,
}

impl Issue {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.file.as_deref().unwrap_or(""),
            self.rule_id,
            self.location.short(),
            self.original,
            self.suggestion
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticInfo {
    pub enabled: bool,
    pub device: String,
    pub backend: String,
    pub fallback_occurred: bool,
}

impl SemanticInfo {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            device: "cpu".to_owned(),
            backend: "none".to_owned(),
            fallback_occurred: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeverityCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
}

impl SeverityCounts {
    pub fn add(&mut self, severity: Severity) {
        match severity {
            Severity::Error => self.error += 1,
            Severity::Warning => self.warning += 1,
            Severity::Info => self.info += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.error + self.warning + self.info
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub by_severity: SeverityCounts,
}

impl Summary {
    pub fn from_issues(issues: &[Issue]) -> Self {
        let mut by_severity = SeverityCounts::default();
        for issue in issues {
            by_severity.add(issue.severity);
        }
        Self {
            total: issues.len(),
            by_severity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: String,
    pub tool: ToolInfo,
    pub generated_at: String,
    pub source: String,
    pub cancelled: bool,
    pub semantic: SemanticInfo,
    pub summary: Summary,
    pub issues: Vec<Issue>,
    /// Non-serialized diagnostic for the CLI/TUI. Never part of the stable
    /// report contract.
    #[serde(skip)]
    pub semantic_warning: Option<String>,
}

impl Report {
    pub fn new(
        source: impl Into<String>,
        semantic: SemanticInfo,
        semantic_warning: Option<String>,
        issues: Vec<Issue>,
        cancelled: bool,
    ) -> Self {
        let summary = Summary::from_issues(&issues);
        Self {
            schema_version: SCHEMA_VERSION.to_owned(),
            tool: ToolInfo {
                name: "errator".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            generated_at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            source: source.into(),
            cancelled,
            semantic,
            summary,
            issues,
            semantic_warning,
        }
    }

    pub fn text_issues(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter()
    }

    /// Recompute totals after a TUI ignore removed one or more issues.
    pub fn recount(&mut self) {
        self.summary = Summary::from_issues(&self.issues);
    }

    pub fn has_errors(&self) -> bool {
        self.summary.by_severity.error > 0
    }
}
