mod consistency;
mod custom;
mod dictionary;
mod duplicate;
mod punctuation;
mod spacing;
pub mod test_support;

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::cancel::CancellationToken;
use crate::config::{Config, RuleLevel};
use crate::document::Document;
use crate::model::{Issue, Location, Severity, Source};

pub use consistency::ConsistencyRule;
pub use custom::{CustomRegexRule, PairRule};
pub use dictionary::{ConfusionRule, DictionaryEntry};
pub use duplicate::DuplicateCharRule;
pub use punctuation::{DuplicatePunctuationRule, FullWidthInEnglishRule, HalfWidthInChineseRule};
pub use spacing::{ExtraSpaceRule, MissingSpaceRule};

/// Read-only inputs made available to every rule. Rules must not mutate the
/// document and must remain safe to move across worker threads.
pub struct RuleContext<'a> {
    pub document: &'a Document,
    pub config: &'a Config,
    pub include_context: bool,
    pub token: &'a CancellationToken,
    pub cancellation_enabled: bool,
}

impl<'a> RuleContext<'a> {
    pub fn cancelled(&self) -> bool {
        self.token.cancelled(self.cancellation_enabled)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        &self,
        rule_id: &str,
        category: &str,
        severity: Severity,
        offset: usize,
        original: impl Into<String>,
        suggestion: impl Into<String>,
        message: impl Into<String>,
        source: Source,
        confidence: f32,
    ) -> Issue {
        let location = self.document.location(offset, self.include_context);
        Issue {
            rule_id: rule_id.to_owned(),
            category: category.to_owned(),
            severity,
            location,
            file: None,
            original: original.into(),
            suggestion: suggestion.into(),
            message: message.into(),
            source,
            confidence,
            offset,
        }
    }

    pub fn location(&self, offset: usize) -> Location {
        self.document.location(offset, self.include_context)
    }
}

/// A deterministic prose check. Adding a rule means implementing this trait
/// and registering it in [`RuleRegistry::build`]; the report schema does not
/// change.
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn category(&self) -> &str;
    fn default_severity(&self) -> Severity;
    fn description(&self) -> &str;
    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue>;
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleInfo {
    pub id: String,
    pub category: String,
    pub severity: Severity,
    pub description: String,
    pub enabled: bool,
}

pub struct RuleRegistry {
    rules: Vec<Box<dyn Rule>>,
    severity_overrides: BTreeMap<String, RuleLevel>,
    disabled: HashSet<String>,
}

impl RuleRegistry {
    pub fn build(config: &Config) -> Result<Self> {
        let mut rules: Vec<Box<dyn Rule>> = vec![
            Box::new(MissingSpaceRule),
            Box::new(ExtraSpaceRule),
            Box::new(HalfWidthInChineseRule),
            Box::new(FullWidthInEnglishRule),
            Box::new(DuplicatePunctuationRule),
            Box::new(ConfusionRule::builtin()),
            Box::new(DuplicateCharRule),
            Box::new(ConsistencyRule::from_config(config)),
        ];

        let mut extra_entries: Vec<DictionaryEntry> = Vec::new();
        for entry in config
            .dictionary
            .extra
            .iter()
            .chain(config.dictionary.confusions.iter())
        {
            if entry.wrong.trim().is_empty() || entry.wrong == entry.right {
                bail!("dictionary 混淆词条目的 wrong 不能为空且不能与 right 相同");
            }
            extra_entries.push(DictionaryEntry::with_severity(
                &entry.wrong,
                &entry.right,
                entry.severity,
            ));
        }
        if !extra_entries.is_empty() {
            rules.push(Box::new(ConfusionRule::custom(extra_entries)));
        }

        for custom in &config.rules.custom {
            if !custom.enabled {
                continue;
            }
            rules.push(match custom.resolved_type() {
                "pair" => Box::new(PairRule::new(custom)?),
                "regex" => Box::new(CustomRegexRule::new(custom)?),
                other => bail!("自定义规则 `{}` 的类型无效：{other}", custom.id),
            });
        }

        let mut known = HashSet::new();
        for rule in &rules {
            if !known.insert(rule.id().to_owned()) {
                bail!("规则 id 重复：{}", rule.id());
            }
        }

        Ok(Self {
            rules,
            severity_overrides: config.rules.severity_overrides.clone(),
            disabled: config.rules.disabled.iter().cloned().collect(),
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Rule> {
        self.rules.iter().map(|rule| rule.as_ref())
    }

    pub fn is_disabled(&self, rule_id: &str) -> bool {
        self.disabled.contains(rule_id)
            || matches!(self.severity_overrides.get(rule_id), Some(RuleLevel::Off))
    }

    pub fn severity_override(&self, rule_id: &str) -> Option<Severity> {
        self.severity_overrides
            .get(rule_id)
            .and_then(|level| level.severity())
    }

    pub fn severity_for(&self, rule: &dyn Rule) -> Severity {
        self.severity_override(rule.id())
            .unwrap_or_else(|| rule.default_severity())
    }

    pub fn info(&self) -> Vec<RuleInfo> {
        self.rules
            .iter()
            .map(|rule| RuleInfo {
                id: rule.id().to_owned(),
                category: rule.category().to_owned(),
                severity: self.severity_for(rule.as_ref()),
                description: rule.description().to_owned(),
                enabled: !self.is_disabled(rule.id()),
            })
            .collect()
    }

    pub fn contains(&self, rule_id: &str) -> bool {
        self.rules.iter().any(|rule| rule.id() == rule_id)
    }
}

/// Rules are normally applied through the registry; this helper is used by
/// tests and by future embedders that provide their own rule list.
pub fn sort_issues(issues: &mut [Issue]) {
    issues.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.location.short().cmp(&right.location.short()))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
}
