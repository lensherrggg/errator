use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};

use crate::cancel::CancellationToken;
use crate::config::{Config, RuleLevel};
use crate::document::{Document, collect_inputs};
use crate::external;
use crate::ignore::{IgnoreEntry, IgnoreStore};
use crate::model::{Issue, Report, Severity};
use crate::paths;
use crate::rules::{RuleContext, RuleRegistry};
use crate::semantic::{self, DeviceRequest, SemanticMode};

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub min_severity: Severity,
    pub include_context: bool,
    pub ignore_rule_ids: Vec<String>,
    pub semantic_mode: SemanticMode,
    pub device: DeviceRequest,
    pub no_gpu: bool,
    pub cancellation_enabled: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            min_severity: Severity::Info,
            include_context: true,
            ignore_rule_ids: Vec::new(),
            semantic_mode: SemanticMode::Auto,
            device: DeviceRequest::Auto,
            no_gpu: false,
            cancellation_enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Started {
        total_files: usize,
        semantic_device: Option<String>,
    },
    FileStarted {
        index: usize,
        total: usize,
        file: String,
    },
    FileFinished {
        index: usize,
        total: usize,
        file: String,
        issues: usize,
    },
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub report: Report,
    pub warnings: Vec<String>,
    pub file_count: usize,
    pub cancelled: bool,
}

impl CheckOutcome {
    pub fn exit_code(&self) -> i32 {
        if self.cancelled {
            3
        } else if self.report.has_errors() {
            1
        } else {
            0
        }
    }
}

/// Coordinates extraction, deterministic rules, optional inference and ignore
/// filtering. UI layers only see reports and progress events.
pub struct Engine {
    config: Config,
    registry: RuleRegistry,
    ignores: IgnoreStore,
    whitelist: Vec<String>,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self> {
        let registry = RuleRegistry::build(&config)?;
        let ignores = IgnoreStore::load_default()?;
        let whitelist = config.all_whitelist();
        Ok(Self {
            config,
            registry,
            ignores,
            whitelist,
        })
    }

    pub fn with_ignores(config: Config, ignores: IgnoreStore) -> Result<Self> {
        let registry = RuleRegistry::build(&config)?;
        let whitelist = config.all_whitelist();
        Ok(Self {
            config,
            registry,
            ignores,
            whitelist,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn registry(&self) -> &RuleRegistry {
        &self.registry
    }

    pub fn ignores(&self) -> &IgnoreStore {
        &self.ignores
    }

    pub fn ignore_issue(&mut self, issue: &Issue, note: Option<String>) -> Result<IgnoreEntry> {
        self.ignores.add(&issue.rule_id, &issue.original, note)
    }

    /// Check a file or recursively check a directory. Runtime failures (I/O,
    /// malformed docx, invalid custom regex) return `Err` and map to exit code 2
    /// at the CLI. Findings and cancellation are represented inside the report.
    pub fn run(
        &self,
        path: &Path,
        options: &CheckOptions,
        token: &CancellationToken,
        mut progress: impl FnMut(ProgressEvent),
    ) -> Result<CheckOutcome> {
        let files = collect_inputs(path)?;
        let is_batch = path.is_dir();
        let source = paths::display_path(path);

        let mut runtime = semantic::prepare(
            &self.config,
            options.semantic_mode,
            options.device,
            options.no_gpu,
        );
        let mut warnings = Vec::new();
        if let Some(warning) = runtime.warning.clone() {
            warnings.push(warning);
        }

        let ignored_rules: HashSet<&str> =
            options.ignore_rule_ids.iter().map(String::as_str).collect();
        let mut found: Vec<(usize, usize, Issue)> = Vec::new();
        let mut seen = HashSet::new();
        let mut cancelled = false;
        let mut order = 0usize;

        progress(ProgressEvent::Started {
            total_files: files.len(),
            semantic_device: runtime.is_active().then(|| runtime.info.device.clone()),
        });

        'files: for (file_index, file) in files.iter().enumerate() {
            if token.cancelled(options.cancellation_enabled) {
                cancelled = true;
                progress(ProgressEvent::Cancelled);
                break;
            }
            let display = paths::display_path_relative_to_cwd(file);
            progress(ProgressEvent::FileStarted {
                index: file_index + 1,
                total: files.len(),
                file: display.clone(),
            });
            let document = Document::load(file)?;

            for rule in self.registry.iter() {
                let level = self
                    .config
                    .rules
                    .effective_level(rule.id(), document.kind, &document.display)
                    .unwrap_or(RuleLevel::Auto);
                if self.registry.is_disabled(rule.id())
                    || ignored_rules.contains(rule.id())
                    || level.is_off()
                {
                    continue;
                }
                if token.cancelled(options.cancellation_enabled) {
                    cancelled = true;
                    break;
                }
                let rule_context = RuleContext {
                    document: &document,
                    config: &self.config,
                    include_context: options.include_context,
                    token,
                    cancellation_enabled: options.cancellation_enabled,
                };
                let issues = rule.check(&rule_context);
                let override_severity = level.severity();
                for mut issue in issues {
                    if let Some(severity) = override_severity {
                        issue.severity = severity;
                    }
                    if is_batch {
                        issue.file = Some(display.clone());
                    }
                    if !options.include_context {
                        strip_context(&mut issue);
                    }
                    if !self.keep_issue(&issue, options) {
                        continue;
                    }
                    if seen.insert(issue.fingerprint()) {
                        found.push((file_index, order, issue));
                        order += 1;
                    }
                }
            }

            if token.cancelled(options.cancellation_enabled) {
                cancelled = true;
            }

            if !cancelled {
                let external_outcome =
                    external::run_all(&self.config, &document, token, options.cancellation_enabled);
                warnings.extend(external_outcome.warnings);
                for mut issue in external_outcome.issues {
                    if token.cancelled(options.cancellation_enabled) {
                        cancelled = true;
                        break;
                    }
                    let level = self
                        .config
                        .rules
                        .effective_level(&issue.rule_id, document.kind, &document.display)
                        .unwrap_or(RuleLevel::Auto);
                    if level.is_off() {
                        continue;
                    }
                    if let Some(severity) = level.severity() {
                        issue.severity = severity;
                    }
                    if is_batch {
                        issue.file = Some(display.clone());
                    }
                    if !options.include_context {
                        strip_context(&mut issue);
                    }
                    if !self.keep_issue(&issue, options) {
                        continue;
                    }
                    if seen.insert(issue.fingerprint()) {
                        found.push((file_index, order, issue));
                        order += 1;
                    }
                }
            }

            if !cancelled && runtime.is_active() {
                let semantic_limit = self.config.semantic.max_text_length;
                let mut result = runtime.analyze(
                    &document,
                    token,
                    options.cancellation_enabled,
                    semantic_limit,
                );
                if result.is_err() && runtime.info.device != "cpu" {
                    let first_error = result
                        .as_ref()
                        .err()
                        .map(|error| error.to_string())
                        .unwrap_or_default();
                    if self.config.semantic.allow_gpu_fallback {
                        warnings.push(format!(
                            "{} 推理失败：{first_error}；正在尝试 CPU 回退",
                            runtime.info.device
                        ));
                        if let Err(error) = runtime.fallback_to_cpu() {
                            warnings.push(format!("CPU 回退失败：{error}"));
                            runtime.disable(format!("语义检查已禁用：{error}"));
                        } else {
                            result = runtime.analyze(
                                &document,
                                token,
                                options.cancellation_enabled,
                                semantic_limit,
                            );
                        }
                    } else {
                        runtime.disable(format!("语义检查已禁用：GPU 推理失败（{first_error}）"));
                        warnings.push(format!("GPU 推理失败（{first_error}）"));
                    }
                }
                match result {
                    Ok(mut issues) => {
                        let level = self
                            .config
                            .rules
                            .effective_level("SEMANTIC_CSC", document.kind, &document.display)
                            .unwrap_or(RuleLevel::Auto);
                        if !level.is_off() {
                            for mut issue in issues.drain(..) {
                                if let Some(severity) = level.severity() {
                                    issue.severity = severity;
                                }
                                if is_batch {
                                    issue.file = Some(display.clone());
                                }
                                if !options.include_context {
                                    strip_context(&mut issue);
                                }
                                if !self.keep_issue(&issue, options) {
                                    continue;
                                }
                                if seen.insert(issue.fingerprint()) {
                                    found.push((file_index, order, issue));
                                    order += 1;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        warnings.push(format!("语义推理失败：{error}"));
                    }
                }
            }

            if token.cancelled(options.cancellation_enabled) {
                cancelled = true;
            }

            let file_issue_count = found
                .iter()
                .filter(|(index, _, _)| *index == file_index)
                .count();
            progress(ProgressEvent::FileFinished {
                index: file_index + 1,
                total: files.len(),
                file: display,
                issues: file_issue_count,
            });

            if cancelled {
                progress(ProgressEvent::Cancelled);
                break 'files;
            }
        }

        found.sort_by(|(file_a, _, issue_a), (file_b, _, issue_b)| {
            file_a
                .cmp(file_b)
                .then_with(|| issue_a.offset.cmp(&issue_b.offset))
                .then_with(|| issue_a.rule_id.cmp(&issue_b.rule_id))
        });
        let issues: Vec<Issue> = found.into_iter().map(|(_, _, issue)| issue).collect();

        let semantic_info = runtime.info.clone();
        let semantic_warning = runtime.warning.clone();
        let report = Report::new(source, semantic_info, semantic_warning, issues, cancelled);
        Ok(CheckOutcome {
            report,
            warnings,
            file_count: files.len(),
            cancelled,
        })
    }

    fn keep_issue(&self, issue: &Issue, options: &CheckOptions) -> bool {
        if !issue.severity.passes(options.min_severity) {
            return false;
        }
        if options
            .ignore_rule_ids
            .iter()
            .any(|rule_id| rule_id == &issue.rule_id)
        {
            return false;
        }
        if self.ignores.matches(issue) {
            return false;
        }
        if self.is_whitelisted(issue) {
            return false;
        }
        true
    }

    fn is_whitelisted(&self, issue: &Issue) -> bool {
        // Whitelist matching is deliberately scoped to the reported text.
        // Project names often appear in the same line as unrelated findings;
        // matching context would silence far too much.
        self.whitelist
            .iter()
            .any(|term| !term.is_empty() && issue.original.trim() == term)
    }
}

fn strip_context(issue: &mut Issue) {
    match &mut issue.location {
        crate::model::Location::Text { context, .. }
        | crate::model::Location::Docx { context, .. } => *context = None,
    }
}

/// Convenience wrapper used by CLI and tests.
pub fn check_path(config: Config, path: &Path, options: &CheckOptions) -> Result<CheckOutcome> {
    let engine = Engine::new(config)?;
    let token = CancellationToken::new();
    engine
        .run(path, options, &token, |_| {})
        .with_context(|| format!("检查失败：{}", path.display()))
}
