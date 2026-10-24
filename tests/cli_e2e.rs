use std::fs;
use std::io::Write;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;
fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn command(home: &Path) -> Command {
    let mut command = Command::cargo_bin("errator").unwrap();
    command.env("ERRATOR_HOME", home);
    command.env("NO_COLOR", "1");
    command
}

fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON: {error}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn make_docx(path: &Path) {
    let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>标题段落</w:t></w:r></w:p>
    <w:p><w:r><w:t>使用Rust开发</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap();
}

#[test]
fn json_report_has_stable_fields_and_docx_location() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let docx = temp.path().join("sample.docx");
    make_docx(&docx);

    let output = command(&home)
        .args(["check"])
        .arg(&docx)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["schema_version"], "1.0");
    assert_eq!(json["tool"]["name"], "errator");
    assert_eq!(json["semantic"]["device"], "cpu");
    let issue = &json["issues"][0];
    assert_eq!(issue["rule_id"], "CJK_SPACE_MISSING");
    assert_eq!(issue["severity"], "info");
    assert_eq!(issue["location"]["kind"], "docx");
    assert_eq!(issue["location"]["paragraph"], 2);
    assert_eq!(issue["location"]["char_offset"], 1);
    assert!(issue["location"].get("line").is_none());
    assert!(issue.get("offset").is_none());
}

#[test]
fn markdown_report_contains_all_required_sections() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.md");
    write(&source, "# 标题\n\n使用Rust开发。\n");

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--format", "md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("# Errator 勘误报告"));
    assert!(text.contains("- **源文件**："));
    assert!(text.contains("- **生成时间**："));
    assert!(text.contains("- **工具版本**："));
    assert!(text.contains("- **计算设备**：CPU（规则检查）"));
    assert!(text.contains("| 问题总数 | 1 |"));
    assert!(text.contains("### [warning] CJK_SPACE_MISSING"));
    assert!(text.contains("```text"));
    assert!(!text.contains("未发现问题"));
}

#[test]
fn markdown_reports_no_issues_explicitly() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("clean.md");
    write(&source, "这是一段完全正确的中文。\n");

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--format", "md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("未发现问题"));
}

#[test]
fn error_severity_produces_exit_code_one() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.md");
    let config = temp.path().join("errator.toml");
    write(&source, "使用Rust开发。\n");
    write(
        &config,
        "[rules.severity_overrides]\nCJK_SPACE_MISSING = \"error\"\n",
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--config"])
        .arg(&config)
        .arg("--quiet")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn ruleset_can_be_installed_and_selected() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    let ruleset = temp.path().join("project.toml");
    write(&source, "这里用了云原生架构。\n");
    write(
        &ruleset,
        r#"
[ruleset]
name = "project"
version = "0.1.0"

[[rules.custom]]
id = "RULESET_PROJECT_001"
type = "pair"
category = "terminology"
severity = "warning"
wrong = "云原生"
right = "cloud native"
"#,
    );

    let install = command(&home)
        .args(["ruleset", "add"])
        .arg(&ruleset)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&install.stderr)
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--ruleset", "project", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["issues"][0]["rule_id"], "RULESET_PROJECT_001");
}

#[test]
fn persistent_ignore_filters_future_checks() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    write(&source, "使用Rust开发。\n");

    let ignored = command(&home)
        .args([
            "ignore",
            "add",
            "--rule",
            "CJK_SPACE_MISSING",
            "--text",
            "使用Rust开发",
        ])
        .output()
        .unwrap();
    assert!(
        ignored.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&ignored.stderr)
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["summary"]["total"], 0);
}

#[test]
fn directory_check_aggregates_files_without_changing_issue_shape() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let docs = temp.path().join("docs");
    write(&docs.join("a.md"), "使用Rust开发。\n");
    write(&docs.join("nested/b.txt"), "使用Go开发。\n");

    let output = command(&home)
        .current_dir(temp.path())
        .args(["check", "docs", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["summary"]["total"], 2);
    let issues = json["issues"].as_array().unwrap();
    assert!(issues.iter().all(|issue| issue["file"].is_string()));
    assert_eq!(json["source"], "docs");
}

#[test]
fn external_checker_protocol_is_executed_when_configured() {
    let python = ["python", "python3"].into_iter().find(|program| {
        std::process::Command::new(program)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    });
    let Some(python) = python else {
        // Python is optional in the contributor environment; unit tests still
        // cover protocol parsing and command splitting.
        return;
    };

    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    let config = temp.path().join("errator.toml");
    let checker = temp.path().join("checker.py");
    write(&source, "API 网管需要修复。\n");
    write(
        &checker,
        r#"
import json, sys
req = json.load(sys.stdin)
issues = []
for seg in req["segments"]:
    if "API 网管" in seg["text"]:
        idx = seg["text"].index("API 网管")
        issues.append({
            "rule_id": "EXT_001", "category": "terminology",
            "severity": "warning",
            "location": {"kind": "text", "line": seg["line"], "column": 1 + idx},
            "original": "API 网管", "suggestion": "API 网关",
            "message": "术语错误", "confidence": 0.9
        })
json.dump({"issues": issues}, sys.stdout, ensure_ascii=False)
"#,
    );
    write(
        &config,
        &format!(
            "[external.demo]\ncommand = \"{python} checker.py\"\ntimeout_ms = 5000\ncwd = \".\"\n"
        ),
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--config"])
        .arg(&config)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    let issue = json["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["rule_id"] == "EXT_001")
        .expect("external issue should be present");
    assert_eq!(issue["original"], "API 网管");
}

#[test]
fn output_refuses_overwrite_without_force() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    let output = temp.path().join("report.md");
    write(&source, "使用Rust开发。\n");
    write(&output, "existing");

    let refused = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--format", "md", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!refused.status.success());

    let forced = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--format", "md", "-o"])
        .arg(&output)
        .arg("--force")
        .output()
        .unwrap();
    assert!(
        forced.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(
        fs::read_to_string(&output)
            .unwrap()
            .contains("Errator 勘误报告")
    );
}

#[cfg(feature = "semantic")]
#[test]
fn semantic_model_runs_on_cpu_and_keeps_report_schema() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    let config = temp.path().join("errator.toml");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/semantic");
    write(&source, "你好吗？我们很好。\n");
    write(
        &config,
        &format!(
            "[semantic]\nmode = \"on\"\ndevice = \"cpu\"\nmodel_path = {:?}\nvocab_path = {:?}\nmin_confidence = 0.5\nmax_text_length = 100\n",
            fixture.join("toy_csc.onnx").display().to_string(),
            fixture.join("vocab.txt").display().to_string()
        ),
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--config"])
        .arg(&config)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    assert_eq!(json["semantic"]["enabled"], true);
    assert_eq!(json["semantic"]["device"], "cpu");
    assert_eq!(json["semantic"]["backend"], "onnxruntime");
    assert_eq!(json["schema_version"], "1.0");
    assert!(
        json["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["rule_id"] == "SEMANTIC_CSC")
    );
}

#[cfg(all(feature = "semantic-directml", target_os = "windows"))]
#[test]
fn directml_request_always_degrades_gracefully() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("doc.txt");
    let config = temp.path().join("errator.toml");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/semantic");
    write(&source, "你好吗？\n");
    write(
        &config,
        &format!(
            "[semantic]\nmode = \"on\"\ndevice = \"dml\"\nmodel_path = {:?}\nvocab_path = {:?}\nmin_confidence = 0.5\nmax_text_length = 100\n",
            fixture.join("toy_csc.onnx").display().to_string(),
            fixture.join("vocab.txt").display().to_string()
        ),
    );

    let output = command(&home)
        .args(["check"])
        .arg(&source)
        .args(["--config"])
        .arg(&config)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = stdout_json(&output);
    // DirectML may genuinely work, or the runtime may fail registration and
    // fall back. Both outcomes are valid; the check must not fail.
    assert!(["dml", "cpu"].contains(&json["semantic"]["device"].as_str().unwrap()));
    assert!(
        json["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["rule_id"] == "SEMANTIC_CSC")
    );
}
