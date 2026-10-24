//! Errator — local, offline-first proofreading reports for plain text and Word files.
//!
//! The crate is intentionally split into small modules so that new rules, input
//! formats, report exporters and inference backends can be added without
//! touching the CLI or the stable report schema.

pub mod cancel;
pub mod cli;
pub mod config;
pub mod document;
pub mod engine;
pub mod external;
pub mod ignore;
pub mod model;
pub mod models;
pub mod paths;
pub mod report;
pub mod rules;
pub mod ruleset;
pub mod semantic;
pub mod tui;
pub mod version;

/// Compile-time extension surface. Keeping these re-exports in one place makes
/// it possible to extract an `errator-sdk` crate without breaking downstream
/// native rules/exporters.
pub mod sdk {
    pub use crate::cancel::CancellationToken;
    pub use crate::config::{Config, CustomRuleConfig};
    pub use crate::document::Document;
    pub use crate::ignore::IgnoreStore;
    pub use crate::model::{
        Issue, Location, Report, SCHEMA_VERSION, SemanticInfo, Severity, Source,
    };
    pub use crate::report::{
        BuiltinExporter, Exporter, OutputFormat, RenderOptions, render, write_report,
    };
    pub use crate::rules::{Rule, RuleContext};
}

pub use model::Severity;

/// High-level process entry point. Keeping this in the library makes the CLI
/// testable and leaves `main.rs` a three-line shim.
pub fn run() -> i32 {
    cli::run()
}
