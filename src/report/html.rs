use crate::model::Report;
use crate::report::semantic_description;

pub fn render(report: &Report) -> String {
    let mut output = String::new();
    output.push_str(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Errator 勘误报告</title>
<style>
:root { color-scheme: light dark; --bg:#f7f7f5; --fg:#1f2328; --muted:#656d76; --card:#fff; --line:#d8dee4; --accent:#0969da; --error:#cf222e; --warning:#9a6700; --info:#0969da; --code:#f6f8fa; }
@media (prefers-color-scheme: dark) { :root { --bg:#0d1117; --fg:#e6edf3; --muted:#8d96a0; --card:#161b22; --line:#30363d; --accent:#58a6ff; --error:#ff7b72; --warning:#d29922; --info:#79c0ff; --code:#0d1117; } }
* { box-sizing: border-box; }
body { margin:0; padding:2.5rem 1rem; background:var(--bg); color:var(--fg); font:16px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI","Microsoft YaHei",sans-serif; }
main { max-width: 980px; margin: 0 auto; }
h1 { margin:0 0 .35rem; font-size:2rem; letter-spacing:-.02em; }
.subtitle { color:var(--muted); margin:0 0 1.8rem; }
h2 { margin:2.2rem 0 1rem; font-size:1.25rem; border-bottom:1px solid var(--line); padding-bottom:.4rem; }
h3 { margin:0 0 .7rem; font-size:1.02rem; }
.meta { display:grid; grid-template-columns:repeat(auto-fit,minmax(230px,1fr)); gap:.55rem 1.5rem; margin:0; padding:1.1rem 1.2rem; background:var(--card); border:1px solid var(--line); border-radius:12px; }
.meta div { min-width:0; }
.meta dt { color:var(--muted); font-size:.78rem; text-transform:uppercase; letter-spacing:.05em; }
.meta dd { margin:.12rem 0 0; overflow-wrap:anywhere; }
.cancelled { margin-top:1rem; padding:.7rem 1rem; border:1px solid var(--warning); border-left-width:4px; border-radius:8px; color:var(--warning); }
table { width:100%; border-collapse:collapse; background:var(--card); border:1px solid var(--line); border-radius:12px; overflow:hidden; }
th,td { padding:.65rem .9rem; text-align:left; border-bottom:1px solid var(--line); }
th:last-child,td:last-child { text-align:right; }
tr:last-child td { border-bottom:0; }
.issue { margin:0 0 1rem; padding:1.1rem 1.25rem; background:var(--card); border:1px solid var(--line); border-left:4px solid var(--muted); border-radius:10px; }
.issue.error { border-left-color:var(--error); }
.issue.warning { border-left-color:var(--warning); }
.issue.info { border-left-color:var(--info); }
.rule { color:var(--muted); font-weight:400; font-size:.9rem; }
.badge { display:inline-block; margin-right:.5rem; font-size:.72rem; font-weight:700; letter-spacing:.04em; text-transform:uppercase; }
.error .badge { color:var(--error); } .warning .badge { color:var(--warning); } .info .badge { color:var(--info); }
dl.issue-meta { display:grid; grid-template-columns:5.5rem 1fr; gap:.3rem .75rem; margin:.8rem 0; font-size:.92rem; }
dl.issue-meta dt { color:var(--muted); } dl.issue-meta dd { margin:0; overflow-wrap:anywhere; }
pre { margin:.35rem 0 .9rem; padding:.85rem 1rem; overflow:auto; background:var(--code); border:1px solid var(--line); border-radius:8px; white-space:pre-wrap; overflow-wrap:anywhere; font: .9rem/1.55 ui-monospace,SFMono-Regular,Consolas,monospace; }
.empty { padding:1.4rem; border:1px dashed var(--line); border-radius:10px; color:var(--muted); }
footer { margin-top:2.5rem; color:var(--muted); font-size:.82rem; }
</style>
</head>
<body>
<main>
"#,
    );

    output.push_str("<h1>Errator 勘误报告</h1>\n");
    output.push_str(&format!(
        "<p class=\"subtitle\">{} 个问题 · error {} · warning {} · info {}</p>\n",
        report.summary.total,
        report.summary.by_severity.error,
        report.summary.by_severity.warning,
        report.summary.by_severity.info
    ));

    output.push_str("<dl class=\"meta\">\n");
    meta(&mut output, "源文件", &report.source);
    meta(&mut output, "生成时间", &report.generated_at);
    meta(&mut output, "工具版本", &report.tool.version);
    meta(
        &mut output,
        "是否取消",
        if report.cancelled {
            "是（部分结果）"
        } else {
            "否"
        },
    );
    meta(
        &mut output,
        "语义检查",
        &semantic_description(&report.semantic),
    );
    output.push_str("</dl>\n");

    if report.cancelled {
        output.push_str("<div class=\"cancelled\">检查已取消，以下为部分结果。</div>\n");
    }

    output.push_str(
        "<h2>摘要</h2>\n<table>\n<thead><tr><th>指标</th><th>数量</th></tr></thead>\n<tbody>\n",
    );
    for (label, count) in [
        ("问题总数", report.summary.total),
        ("error", report.summary.by_severity.error),
        ("warning", report.summary.by_severity.warning),
        ("info", report.summary.by_severity.info),
    ] {
        output.push_str(&format!(
            "<tr><td>{}</td><td>{count}</td></tr>\n",
            escape(label)
        ));
    }
    output.push_str("</tbody>\n</table>\n");

    output.push_str("<h2>问题</h2>\n");
    if report.issues.is_empty() {
        output.push_str("<div class=\"empty\">");
        output.push_str(if report.cancelled {
            "检查已取消，尚未发现问题。"
        } else {
            "未发现问题。"
        });
        output.push_str("</div>\n");
    } else {
        for issue in &report.issues {
            output.push_str(&format!(
                "<article class=\"issue {}\">\n<h3><span class=\"badge\">{}</span><span class=\"rule\">{}</span></h3>\n",
                escape(issue.severity.label()),
                escape(issue.severity.label()),
                escape(&issue.rule_id)
            ));
            output.push_str("<dl class=\"issue-meta\">\n");
            let file_prefix = issue
                .file
                .as_ref()
                .map(|file| format!("{file} · "))
                .unwrap_or_default();
            meta(
                &mut output,
                "位置",
                &format!(
                    "{}{}（{}）",
                    file_prefix,
                    issue.location.short(),
                    issue.location.kind()
                ),
            );
            if let Some(context) = issue.location.context()
                && context != issue.original
            {
                meta(&mut output, "上下文", context);
            }
            meta(
                &mut output,
                "来源",
                &format!("{}（置信度 {:.2}）", issue.source.label(), issue.confidence),
            );
            output.push_str("</dl>\n");
            output.push_str("<div class=\"label\">原文</div>\n<pre>");
            output.push_str(&escape(&issue.original));
            output.push_str("</pre>\n<div class=\"label\">建议</div>\n<pre>");
            output.push_str(&escape(&issue.suggestion));
            output.push_str("</pre>\n<p>");
            output.push_str(&escape(&issue.message));
            output.push_str("</p>\n</article>\n");
        }
    }

    output.push_str(&format!(
        "<footer>由 Errator {} 生成 · schema {} · 完全离线渲染</footer>\n</main>\n</body>\n</html>\n",
        escape(&report.tool.version),
        escape(&report.schema_version)
    ));
    output
}

fn meta(output: &mut String, label: &str, value: &str) {
    output.push_str(&format!(
        "<div><dt>{}</dt><dd>{}</dd></div>\n",
        escape(label),
        escape(value)
    ));
}

fn escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            other => output.push(other),
        }
    }
    output
}
