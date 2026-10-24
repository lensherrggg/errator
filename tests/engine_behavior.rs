use std::fs;

use errator::cancel::CancellationToken;
use errator::config::Config;
use errator::engine::{CheckOptions, Engine};
use errator::ignore::IgnoreStore;
use errator::semantic::SemanticMode;
use tempfile::TempDir;

fn options() -> CheckOptions {
    CheckOptions {
        semantic_mode: SemanticMode::Off,
        ..CheckOptions::default()
    }
}

#[test]
fn pre_cancelled_check_returns_partial_report_and_exit_three() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("doc.txt");
    fs::write(&source, "使用Rust开发。\n").unwrap();
    let ignores = IgnoreStore::load(&temp.path().join("ignores.json")).unwrap();
    let engine = Engine::with_ignores(Config::default(), ignores).unwrap();
    let token = CancellationToken::new();
    token.cancel();

    let outcome = engine.run(&source, &options(), &token, |_| {}).unwrap();
    assert!(outcome.cancelled);
    assert!(outcome.report.cancelled);
    assert_eq!(outcome.exit_code(), 3);
    assert_eq!(outcome.report.summary.total, 0);
}

#[test]
fn missing_input_and_empty_directory_are_runtime_errors() {
    let temp = TempDir::new().unwrap();
    let ignores = IgnoreStore::load(&temp.path().join("ignores.json")).unwrap();
    let engine = Engine::with_ignores(Config::default(), ignores).unwrap();
    let token = CancellationToken::new();
    assert!(
        engine
            .run(&temp.path().join("missing.txt"), &options(), &token, |_| {})
            .is_err()
    );
    assert!(engine.run(temp.path(), &options(), &token, |_| {}).is_err());
}
