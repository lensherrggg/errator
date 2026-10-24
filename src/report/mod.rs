mod csv;
mod html;
mod markdown;
mod text;

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

use crate::model::{Report, SemanticInfo, Severity};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[value(name = "text")]
    Text,
    #[value(name = "json")]
    Json,
    #[value(name = "md", alias = "markdown")]
    Markdown,
    #[value(name = "html")]
    Html,
    #[value(name = "csv")]
    Csv,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Json => "json",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Csv => "csv",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Csv => "csv",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub color: bool,
    pub quiet: bool,
}

/// Stable exporter extension point. A downstream crate can implement this trait
/// and reuse the same `Report` model; built-in formats are just implementations.
pub trait Exporter {
    fn format(&self) -> &str;
    fn export(&self, report: &Report, out: &mut dyn Write) -> Result<()>;
}

/// Built-in exporter selected by the CLI's `--format` value.
pub struct BuiltinExporter {
    format: OutputFormat,
    options: RenderOptions,
}

impl BuiltinExporter {
    pub fn new(format: OutputFormat, options: RenderOptions) -> Self {
        Self { format, options }
    }
}

impl Exporter for BuiltinExporter {
    fn format(&self) -> &str {
        self.format.label()
    }

    fn export(&self, report: &Report, out: &mut dyn Write) -> Result<()> {
        let rendered = match self.format {
            OutputFormat::Text => text::render(report, self.options),
            OutputFormat::Json => {
                let mut json = serde_json::to_string_pretty(report)?;
                json.push('\n');
                json
            }
            OutputFormat::Markdown => markdown::render(report),
            OutputFormat::Html => html::render(report),
            OutputFormat::Csv => csv::render(report),
        };
        out.write_all(rendered.as_bytes())?;
        Ok(())
    }
}

pub fn render(report: &Report, format: OutputFormat, options: RenderOptions) -> Result<String> {
    let exporter = BuiltinExporter::new(format, options);
    let mut output = Vec::new();
    exporter.export(report, &mut output)?;
    String::from_utf8(output).context("导出器输出不是 UTF-8")
}

/// Write a report safely via a sibling temporary file. Refuses to overwrite
/// unless `force` is set, matching the CLI's `--force` contract.
pub fn write_report(path: &Path, content: &str, force: bool) -> Result<()> {
    if path.is_dir() {
        bail!("输出路径是目录，请提供文件名：{}", path.display());
    }
    if path.exists() && !force {
        bail!("输出文件已存在：{}（使用 --force 覆盖）", path.display());
    }
    paths::ensure_parent(path)?;
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("out")
    ));
    std::fs::write(&temporary, content)
        .with_context(|| format!("无法写入临时文件 {}", temporary.display()))?;
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("无法替换输出文件 {}", path.display()))?;
    }
    std::fs::rename(&temporary, path)
        .with_context(|| format!("无法保存报告 {}", path.display()))?;
    Ok(())
}

pub fn semantic_label(info: &SemanticInfo) -> &'static str {
    if !info.enabled {
        return "关闭";
    }
    match info.device.as_str() {
        "dml" => "DirectML",
        "coreml" => "CoreML",
        _ => "CPU",
    }
}

/// Human-readable description of where the work actually runs. Deterministic
/// rules always execute on the CPU; semantic inference may use a GPU EP.
pub fn execution_device_label(info: &SemanticInfo) -> String {
    if !info.enabled {
        return "CPU（规则检查）".to_owned();
    }
    let mut label = match info.device.as_str() {
        "dml" => "GPU · DirectML".to_owned(),
        "coreml" => "GPU · CoreML".to_owned(),
        "cpu" => "CPU · ONNX Runtime".to_owned(),
        other => format!("{other} · 语义推理"),
    };
    if info.fallback_occurred {
        label.push_str("（已降级）");
    }
    label
}

pub fn semantic_description(info: &SemanticInfo) -> String {
    if !info.enabled {
        return "关闭（仅规则检查）".to_owned();
    }
    let mut value = format!("{}（后端：{}）", semantic_label(info), info.backend);
    if info.fallback_occurred {
        value.push_str("，已降级");
    }
    value
}

pub fn severity_rank(severity: Severity) -> u8 {
    severity.rank()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, Location, SemanticInfo, Source};

    fn sample_report(issues: Vec<Issue>) -> Report {
        Report::new(
            "docs/example.md",
            SemanticInfo::disabled(),
            None,
            issues,
            false,
        )
    }

    fn sample_issue() -> Issue {
        Issue {
            rule_id: "CJK_SPACE_MISSING".to_owned(),
            category: "spacing".to_owned(),
            severity: Severity::Warning,
            location: Location::Text {
                line: 12,
                column: 8,
                context: Some("使用Rust开发".to_owned()),
            },
            file: None,
            original: "使用Rust开发".to_owned(),
            suggestion: "使用 Rust 开发".to_owned(),
            message: "中英文之间建议加显式空格".to_owned(),
            source: Source::Rule,
            confidence: 1.0,
            offset: 0,
        }
    }

    #[test]
    fn device_labels_make_cpu_and_gpu_explicit() {
        let disabled = SemanticInfo::disabled();
        assert_eq!(execution_device_label(&disabled), "CPU（规则检查）");
        assert_eq!(semantic_description(&disabled), "关闭（仅规则检查）");

        let dml = SemanticInfo {
            enabled: true,
            device: "dml".to_owned(),
            backend: "onnxruntime".to_owned(),
            fallback_occurred: false,
        };
        assert_eq!(execution_device_label(&dml), "GPU · DirectML");

        let cpu = SemanticInfo {
            enabled: true,
            device: "cpu".to_owned(),
            backend: "onnxruntime".to_owned(),
            fallback_occurred: true,
        };
        assert_eq!(execution_device_label(&cpu), "CPU · ONNX Runtime（已降级）");
    }

    #[test]
    fn text_and_markdown_show_the_execution_device() {
        let report = sample_report(vec![sample_issue()]);
        let text = render(&report, OutputFormat::Text, RenderOptions::default()).unwrap();
        assert!(text.contains("计算设备：CPU（规则检查）"));
        let markdown = render(&report, OutputFormat::Markdown, RenderOptions::default()).unwrap();
        assert!(markdown.contains("- **计算设备**：CPU（规则检查）"));
    }

    #[test]
    fn markdown_contains_required_sections_and_renders_code_safely() {
        let report = sample_report(vec![sample_issue()]);
        let rendered = render(&report, OutputFormat::Markdown, RenderOptions::default()).unwrap();
        assert!(rendered.starts_with("# Errator 勘误报告\n"));
        assert!(rendered.contains("| 指标 | 数量 |"));
        assert!(rendered.contains("### [warning] CJK_SPACE_MISSING"));
        assert!(rendered.contains("使用Rust开发"));
        assert!(rendered.contains("```text"));
    }

    #[test]
    fn markdown_has_explicit_no_issue_message() {
        let rendered = render(
            &sample_report(Vec::new()),
            OutputFormat::Markdown,
            RenderOptions::default(),
        )
        .unwrap();
        assert!(rendered.contains("未发现问题"));
    }

    #[test]
    fn html_escapes_user_content_and_has_no_external_resources() {
        let mut issue = sample_issue();
        issue.original = "<script>alert('x')</script>".to_owned();
        let rendered = render(
            &sample_report(vec![issue]),
            OutputFormat::Html,
            RenderOptions::default(),
        )
        .unwrap();
        assert!(!rendered.contains("<script>alert"));
        assert!(rendered.contains("&lt;script&gt;"));
        assert!(!rendered.contains("http://"));
        assert!(!rendered.contains("https://"));
    }

    #[test]
    fn json_keeps_stable_fields_and_omits_internal_offset() {
        let rendered = render(
            &sample_report(vec![sample_issue()]),
            OutputFormat::Json,
            RenderOptions::default(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["schema_version"], "1.0");
        assert_eq!(value["tool"]["name"], "errator");
        assert_eq!(value["summary"]["total"], 1);
        assert!(value["issues"][0].get("offset").is_none());
    }

    #[test]
    fn csv_quotes_special_characters() {
        let mut issue = sample_issue();
        issue.original = "a,b\n\"c\"".to_owned();
        let rendered = render(
            &sample_report(vec![issue]),
            OutputFormat::Csv,
            RenderOptions::default(),
        )
        .unwrap();
        assert!(rendered.starts_with("file,rule_id,category,severity,"));
        assert!(rendered.contains("\"a,b\n\"\"c\"\"\""));
    }
}
