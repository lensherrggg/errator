use crate::model::{Issue, Report};
use crate::report::semantic_description;

pub fn render(report: &Report) -> String {
    let mut output = String::new();
    output.push_str("# Errator 勘误报告\n\n");

    output.push_str(&format!("- **源文件**：`{}`\n", report.source));
    output.push_str(&format!("- **生成时间**：{}\n", report.generated_at));
    output.push_str(&format!("- **工具版本**：{}\n", report.tool.version));
    output.push_str(&format!(
        "- **是否取消**：{}\n",
        if report.cancelled {
            "是（部分结果）"
        } else {
            "否"
        }
    ));
    output.push_str(&format!(
        "- **语义检查**：{}\n",
        semantic_description(&report.semantic)
    ));
    output.push('\n');

    output.push_str("## 摘要\n\n");
    output.push_str("| 指标 | 数量 |\n");
    output.push_str("|---|---:|\n");
    output.push_str(&format!("| 问题总数 | {} |\n", report.summary.total));
    output.push_str(&format!(
        "| error | {} |\n",
        report.summary.by_severity.error
    ));
    output.push_str(&format!(
        "| warning | {} |\n",
        report.summary.by_severity.warning
    ));
    output.push_str(&format!("| info | {} |\n", report.summary.by_severity.info));
    output.push('\n');

    output.push_str("## 问题\n\n");
    if report.issues.is_empty() {
        output.push_str(if report.cancelled {
            "检查已取消，尚未发现问题。\n"
        } else {
            "未发现问题。\n"
        });
        return output;
    }

    for issue in &report.issues {
        output.push_str(&format!(
            "### [{}] {}\n\n",
            issue.severity.label(),
            issue.rule_id
        ));
        let file_prefix = issue
            .file
            .as_ref()
            .map(|file| format!("{file} · "))
            .unwrap_or_default();
        output.push_str(&format!(
            "- **位置**：{}{}（{}）\n",
            file_prefix,
            issue.location.short(),
            issue.location.kind()
        ));
        output.push_str("- **原文**：\n\n");
        output.push_str(&indented_code_block(&issue.original));
        output.push('\n');
        output.push_str("- **建议**：\n\n");
        output.push_str(&indented_code_block(&issue.suggestion));
        output.push('\n');
        output.push_str(&format!("- **说明**：{}\n", issue.message));
        if let Some(context) = issue.location.context()
            && context != issue.original
        {
            output.push_str(&format!("- **上下文**：`{}`\n", context.replace('`', "'")));
        }
        output.push_str(&format!(
            "- **来源**：{}（置信度 {:.2}）\n",
            issue.source.label(),
            issue.confidence
        ));
        output.push('\n');
    }
    output
}

fn indented_code_block(value: &str) -> String {
    let fence_len = (longest_backtick_run(value) + 1).max(3);
    let fence = "`".repeat(fence_len);
    let mut result = String::new();
    result.push_str(&format!("  {fence}text\n"));
    for line in value.split('\n') {
        result.push_str("  ");
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!("  {fence}\n"));
    result
}

fn longest_backtick_run(value: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[allow(dead_code)]
fn issue_file(issue: &Issue, report: &Report) -> String {
    issue.file.clone().unwrap_or_else(|| report.source.clone())
}
