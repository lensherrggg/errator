use crate::model::Report;
use crate::report::{RenderOptions, execution_device_label, semantic_description};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";

pub fn render(report: &Report, options: RenderOptions) -> String {
    if options.quiet {
        return render_quiet(report);
    }

    let mut output = String::new();
    let paint = |code: &str, value: &str| {
        if options.color {
            format!("{code}{value}{RESET}")
        } else {
            value.to_owned()
        }
    };

    output.push_str(&paint(BOLD, "Errator 勘误报告"));
    output.push('\n');
    output.push_str(&format!("源文件：{}\n", report.source));
    output.push_str(&format!("生成时间：{}\n", report.generated_at));
    output.push_str(&format!("工具版本：{}\n", report.tool.version));
    output.push_str(&format!(
        "计算设备：{}\n",
        execution_device_label(&report.semantic)
    ));
    output.push_str(&format!(
        "语义检查：{}\n",
        semantic_description(&report.semantic)
    ));
    if report.cancelled {
        output.push_str(&paint(YELLOW, "已取消：是（报告为部分结果）"));
        output.push('\n');
    }
    if let Some(warning) = &report.semantic_warning {
        output.push_str(&paint(DIM, &format!("语义提示：{warning}")));
        output.push('\n');
    }
    output.push('\n');

    output.push_str(&format!(
        "问题总数：{}  |  error：{}  |  warning：{}  |  info：{}\n",
        report.summary.total,
        report.summary.by_severity.error,
        report.summary.by_severity.warning,
        report.summary.by_severity.info
    ));

    if report.issues.is_empty() {
        output.push('\n');
        if report.cancelled {
            output.push_str("检查已取消，尚未发现问题。\n");
        } else {
            output.push_str(&paint(GREEN, "未发现问题。"));
            output.push('\n');
        }
        return output;
    }

    output.push('\n');
    for (index, issue) in report.issues.iter().enumerate() {
        let severity = match issue.severity {
            crate::model::Severity::Error => paint(RED, "error"),
            crate::model::Severity::Warning => paint(YELLOW, "warning"),
            crate::model::Severity::Info => paint(BLUE, "info"),
        };
        let file = issue
            .file
            .as_ref()
            .map(|file| format!("{file} · "))
            .unwrap_or_default();
        output.push_str(&format!(
            "{:>3}. [{}] {}{}（{}）  {}\n",
            index + 1,
            severity,
            file,
            issue.location.short(),
            issue.location.kind(),
            paint(BOLD, &issue.rule_id)
        ));
        output.push_str(&format!(
            "     原文：{}\n     建议：{}\n",
            issue.original, issue.suggestion
        ));
        if let Some(context) = issue.location.context()
            && context != issue.original
        {
            output.push_str(&format!(
                "     {}\n",
                paint(DIM, &format!("上下文：{context}"))
            ));
        }
        output.push_str(&format!(
            "     {} · 来源：{}（置信度 {:.2}）\n",
            issue.message,
            issue.source.label(),
            issue.confidence
        ));
        output.push('\n');
    }
    output
}

const GREEN: &str = "\x1b[32m";

fn render_quiet(report: &Report) -> String {
    let device = execution_device_label(&report.semantic);
    if report.issues.is_empty() {
        return format!("{}：未发现问题。计算设备：{device}。\n", report.source);
    }
    format!(
        "{}：{} 个问题（error {}，warning {}，info {}）· 计算设备：{device}{}\n",
        report.source,
        report.summary.total,
        report.summary.by_severity.error,
        report.summary.by_severity.warning,
        report.summary.by_severity.info,
        if report.cancelled {
            "（已取消，部分结果）"
        } else {
            ""
        }
    )
}
