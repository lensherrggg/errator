use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cancel::CancellationToken;
use crate::config::Config;
use crate::document::{Document, DocumentKind};
use crate::model::{Issue, Location, Severity, Source};

#[derive(Debug, Serialize)]
struct CheckerRequest<'a> {
    protocol_version: &'static str,
    source: &'a str,
    kind: &'a str,
    text: &'a str,
    segments: Vec<CheckerSegment<'a>>,
}

#[derive(Debug, Serialize)]
struct CheckerSegment<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paragraph: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    char_offset: Option<usize>,
    text: &'a str,
}

#[derive(Debug, Deserialize)]
struct CheckerResponse {
    #[serde(default)]
    issues: Vec<RawExternalIssue>,
}

#[derive(Debug, Deserialize)]
struct RawExternalIssue {
    rule_id: String,
    #[serde(default)]
    category: String,
    severity: Severity,
    location: Location,
    #[serde(default)]
    original: String,
    #[serde(default)]
    suggestion: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Default)]
pub struct ExternalOutcome {
    pub issues: Vec<Issue>,
    pub warnings: Vec<String>,
}

/// Run every enabled external checker for one document. Checker failures are
/// warnings and never abort the built-in pipeline.
pub fn run_all(
    config: &Config,
    document: &Document,
    token: &CancellationToken,
    cancellation_enabled: bool,
) -> ExternalOutcome {
    let mut outcome = ExternalOutcome::default();
    for (name, checker) in &config.external {
        if !checker.enabled {
            continue;
        }
        if token.cancelled(cancellation_enabled) {
            break;
        }
        match run_one(name, config, document, token, cancellation_enabled) {
            Ok(mut issues) => outcome.issues.append(&mut issues),
            Err(error) => outcome
                .warnings
                .push(format!("外部检查器 `{name}` 失败：{error:#}")),
        }
    }
    outcome
}

fn run_one(
    name: &str,
    config: &Config,
    document: &Document,
    token: &CancellationToken,
    cancellation_enabled: bool,
) -> Result<Vec<Issue>> {
    let checker = config
        .external
        .get(name)
        .with_context(|| format!("外部检查器 `{name}` 配置缺失"))?;
    let argv = split_command(&checker.command)?;
    let Some(program) = argv.first() else {
        bail!("command 为空");
    };

    let request = build_request(document);
    // Emit ASCII-only JSON (non-ASCII escaped as \uXXXX). Windows checkers
    // written in Python/Node often read stdin with the legacy ANSI code page,
    // not UTF-8; escaped JSON is valid UTF-8 and ASCII at the same time.
    let payload = ascii_json_bytes(&request)?;

    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    if cfg!(windows) {
        for key in ["SystemRoot", "PATHEXT", "COMSPEC"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    for (key, value) in &checker.env {
        command.env(key, value);
    }
    // The protocol is UTF-8. Python on Windows otherwise uses the ANSI code
    // page for piped stdin/stdout; these variables make the documented example
    // portable without inheriting unrelated environment variables.
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    if let Some(cwd) = &checker.cwd {
        let cwd = crate::config::resolve_relative(cwd, &config.source_dir)?;
        if !cwd.is_dir() {
            bail!("cwd 不存在：{}", cwd.display());
        }
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("无法启动命令：{}", checker.command))?;
    let mut stdin = child.stdin.take().context("无法打开检查器 stdin")?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&payload);
        // Dropping stdin closes the pipe and signals EOF to the checker.
    });
    let stdout = child.stdout.take().context("无法打开检查器 stdout")?;
    let stderr = child.stderr.take().context("无法打开检查器 stderr")?;
    let stdout_reader = std::thread::spawn(move || read_limited(stdout, 16 * 1024 * 1024));
    let stderr_reader = std::thread::spawn(move || read_limited(stderr, 1024 * 1024));

    let timeout = Duration::from_millis(checker.timeout_ms.max(1));
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        if token.cancelled(cancellation_enabled) {
            cancelled = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = writer.join();
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    if cancelled {
        bail!("已取消");
    }
    if timed_out {
        bail!("超时（{} ms），进程已终止", checker.timeout_ms);
    }
    let Some(status) = status else {
        bail!("进程未返回退出状态");
    };
    if !status.success() {
        let stderr_text = String::from_utf8_lossy(&stderr);
        let detail = truncate_tail(stderr_text.trim(), 400);
        bail!(
            "退出码 {} {}",
            status.code().unwrap_or(-1),
            if detail.is_empty() {
                String::new()
            } else {
                format!("：{detail}")
            }
        );
    }

    let stdout_text = String::from_utf8(stdout).context("检查器 stdout 不是 UTF-8")?;
    let stdout_text = stdout_text.trim_start_matches('\u{feff}').trim();
    if stdout_text.is_empty() {
        bail!("检查器 stdout 为空，预期 JSON");
    }
    let response: CheckerResponse =
        serde_json::from_str(stdout_text).context("检查器 stdout 不是合法协议 JSON")?;

    let mut issues = Vec::new();
    for raw in response.issues {
        if raw.rule_id.trim().is_empty() {
            continue;
        }
        let location = document.complete_location(raw.location);
        let offset = document.offset_for_location(&location);
        issues.push(Issue {
            rule_id: raw.rule_id,
            category: if raw.category.trim().is_empty() {
                "external".to_owned()
            } else {
                raw.category
            },
            severity: raw.severity,
            location,
            file: None,
            original: raw.original,
            suggestion: raw.suggestion,
            message: if raw.message.trim().is_empty() {
                "外部检查器报告的问题".to_owned()
            } else {
                raw.message
            },
            source: Source::Rule,
            confidence: raw.confidence.unwrap_or(1.0).clamp(0.0, 1.0),
            offset,
        });
    }
    Ok(issues)
}

fn build_request(document: &Document) -> CheckerRequest<'_> {
    let segments = match document.kind {
        DocumentKind::Text | DocumentKind::Markdown => document
            .text_segments()
            .into_iter()
            .map(|(line, text)| CheckerSegment {
                line: Some(line),
                column: Some(1),
                paragraph: None,
                char_offset: None,
                text,
            })
            .collect(),
        DocumentKind::Docx => document
            .paragraph_segments()
            .into_iter()
            .map(|(paragraph, text)| CheckerSegment {
                line: None,
                column: None,
                paragraph: Some(paragraph),
                char_offset: Some(1),
                text,
            })
            .collect(),
    };
    CheckerRequest {
        protocol_version: "1.0",
        source: &document.display,
        kind: match document.kind {
            DocumentKind::Text | DocumentKind::Markdown => "text",
            DocumentKind::Docx => "docx",
        },
        text: &document.text,
        segments,
    }
}

fn read_limited(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = reader.by_ref().take(limit as u64).read_to_end(&mut buffer);
    buffer
}

fn ascii_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_string(value)?;
    let mut output = String::with_capacity(json.len());
    for character in json.chars() {
        if character.is_ascii() {
            output.push(character);
            continue;
        }
        let mut units = [0u16; 2];
        for unit in character.encode_utf16(&mut units) {
            output.push_str(&format!("\\u{unit:04x}"));
        }
    }
    Ok(output.into_bytes())
}

fn truncate_tail(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_owned();
    }
    let tail: String = chars[chars.len() - max_chars..].iter().collect();
    format!("…{tail}")
}

/// Split a command string without invoking a shell. Supports single and double
/// quotes. Backslashes are preserved unless they escape a quote, whitespace or
/// another backslash, which keeps Windows paths intact.
pub fn split_command(value: &str) -> Result<Vec<String>> {
    let chars: Vec<char> = value.chars().collect();
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_content = false;
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        if character == '\\' && quote != Some('\'') {
            let next = chars.get(index + 1).copied();
            let escapes = next
                .map(|next| {
                    next == '\\'
                        || next == '"'
                        || next == '\''
                        || (next.is_whitespace() && quote.is_none())
                })
                .unwrap_or(false);
            if escapes {
                current.push(next.unwrap_or('\\'));
                index += 2;
                continue;
            }
            current.push('\\');
            has_content = true;
            index += 1;
            continue;
        }
        match quote {
            Some('\'') => {
                if character == '\'' {
                    quote = None;
                } else {
                    current.push(character);
                }
                has_content = true;
            }
            Some('"') => {
                if character == '"' {
                    quote = None;
                } else {
                    current.push(character);
                }
                has_content = true;
            }
            Some(_) => unreachable!(),
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    has_content = true;
                }
                character if character.is_whitespace() => {
                    if has_content {
                        args.push(std::mem::take(&mut current));
                        has_content = false;
                    }
                }
                other => {
                    current.push(other);
                    has_content = true;
                }
            },
        }
        index += 1;
    }
    if quote.is_some() {
        bail!("command 中的引号未闭合");
    }
    if has_content {
        args.push(current);
    }
    if args.is_empty() {
        bail!("command 为空");
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_commands_without_shell() {
        assert_eq!(
            split_command("python \"checkers/my checker.py\" --flag").unwrap(),
            vec![
                "python".to_owned(),
                "checkers/my checker.py".to_owned(),
                "--flag".to_owned()
            ]
        );
        assert_eq!(
            split_command(r#""C:\Program Files\Python\python.exe" checker.py"#).unwrap(),
            vec![
                r#"C:\Program Files\Python\python.exe"#.to_owned(),
                "checker.py".to_owned()
            ]
        );
        assert!(split_command("\"unterminated").is_err());
    }

    #[test]
    fn parses_protocol_issues() {
        let response: CheckerResponse = serde_json::from_str(
            r#"{"issues":[{"rule_id":"EXT_1","severity":"warning","location":{"kind":"text","line":1,"column":1},"original":"a"}]}"#,
        )
        .unwrap();
        assert_eq!(response.issues.len(), 1);
    }
}
