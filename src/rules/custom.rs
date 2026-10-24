use anyhow::Result;
use regex::{Regex, RegexBuilder};

use crate::config::CustomRuleConfig;
use crate::document::is_ascii_alphanumeric;
use crate::model::{Issue, Severity, Source};
use crate::rules::{Rule, RuleContext};

pub struct CustomRegexRule {
    id: String,
    category: String,
    severity: Severity,
    message: String,
    regex: Regex,
    replacement: String,
    include_excluded: bool,
}

impl CustomRegexRule {
    pub fn new(config: &CustomRuleConfig) -> Result<Self> {
        Ok(Self {
            id: config.id.clone(),
            category: category(config),
            severity: config.severity,
            message: message(config, "匹配自定义正则规则"),
            regex: Regex::new(&config.pattern)?,
            replacement: config.replacement.clone(),
            include_excluded: config.include_excluded,
        })
    }
}

impl Rule for CustomRegexRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn description(&self) -> &str {
        "用户自定义正则规则"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for found in self.regex.find_iter(&document.text) {
            if context.cancelled() {
                return issues;
            }
            if found.is_empty() {
                continue;
            }
            if !self.include_excluded && document.is_masked(found.start(), found.len()) {
                continue;
            }
            let original = found.as_str();
            let suggestion = if self.replacement.is_empty() {
                String::new()
            } else {
                self.regex
                    .replace(original, self.replacement.as_str())
                    .into_owned()
            };
            issues.push(context.issue(
                self.id(),
                self.category(),
                self.default_severity(),
                found.start(),
                original,
                suggestion,
                self.message.clone(),
                Source::Rule,
                1.0,
            ));
        }
        issues
    }
}

pub struct PairRule {
    id: String,
    category: String,
    severity: Severity,
    message: String,
    wrong: String,
    right: String,
    matcher: Regex,
    when_prev: Vec<String>,
    when_next: Vec<String>,
    when_prev_regex: Vec<Regex>,
    when_next_regex: Vec<Regex>,
    window: usize,
    include_excluded: bool,
}

impl PairRule {
    pub fn new(config: &CustomRuleConfig) -> Result<Self> {
        let mut builder = RegexBuilder::new(&regex::escape(&config.wrong));
        builder.case_insensitive(!config.case_sensitive);
        let matcher = builder.build()?;
        let mut when_prev_regex = Vec::new();
        let mut when_next_regex = Vec::new();
        for pattern in &config.when_prev_regex {
            when_prev_regex.push(Regex::new(pattern)?);
        }
        for pattern in &config.when_next_regex {
            when_next_regex.push(Regex::new(pattern)?);
        }
        Ok(Self {
            id: config.id.clone(),
            category: category(config),
            severity: config.severity,
            message: message(
                config,
                &format!("「{}」建议改为「{}」", config.wrong, config.right),
            ),
            wrong: config.wrong.clone(),
            right: config.right.clone(),
            matcher,
            when_prev: config.when_prev.clone(),
            when_next: config.when_next.clone(),
            when_prev_regex,
            when_next_regex,
            window: config.window.max(1),
            include_excluded: config.include_excluded,
        })
    }

    fn context_matches(&self, text: &str, start: usize, end: usize) -> bool {
        let prefix = take_previous_chars(&text[..start], self.window);
        let suffix = take_next_chars(&text[end..], self.window);
        if !self.when_prev.is_empty() && !self.when_prev.iter().any(|term| prefix.contains(term)) {
            return false;
        }
        if !self.when_next.is_empty() && !self.when_next.iter().any(|term| suffix.contains(term)) {
            return false;
        }
        if !self
            .when_prev_regex
            .iter()
            .all(|regex| regex.is_match(prefix))
        {
            return false;
        }
        if !self
            .when_next_regex
            .iter()
            .all(|regex| regex.is_match(suffix))
        {
            return false;
        }
        true
    }
}

impl Rule for PairRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn category(&self) -> &str {
        &self.category
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn description(&self) -> &str {
        "用户自定义词对规则"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for found in self.matcher.find_iter(&document.text) {
            if context.cancelled() {
                return issues;
            }
            if !self.include_excluded && document.is_masked(found.start(), found.len()) {
                continue;
            }
            if !boundary_ok(&document.text, found.start(), found.end(), &self.wrong) {
                continue;
            }
            if !self.context_matches(&document.text, found.start(), found.end()) {
                continue;
            }
            issues.push(context.issue(
                self.id(),
                self.category(),
                self.default_severity(),
                found.start(),
                found.as_str(),
                self.right.clone(),
                self.message.clone(),
                Source::Rule,
                1.0,
            ));
        }
        issues
    }
}

fn category(config: &CustomRuleConfig) -> String {
    if config.category.trim().is_empty() {
        "custom".to_owned()
    } else {
        config.category.clone()
    }
}

fn message(config: &CustomRuleConfig, fallback: &str) -> String {
    if config.message.trim().is_empty() {
        fallback.to_owned()
    } else {
        config.message.clone()
    }
}

/// For English-looking terms, enforce word boundaries on the ASCII side only.
/// Chinese words already have character boundaries in UTF-8 regex matching.
fn boundary_ok(text: &str, start: usize, end: usize, term: &str) -> bool {
    let first = term.chars().next();
    let last = term.chars().next_back();
    if first.map(is_ascii_alphanumeric).unwrap_or(false)
        && let Some(previous) = text[..start].chars().next_back()
        && is_ascii_alphanumeric(previous)
    {
        return false;
    }
    if last.map(is_ascii_alphanumeric).unwrap_or(false)
        && let Some(following) = text[end..].chars().next()
        && is_ascii_alphanumeric(following)
    {
        return false;
    }
    true
}

fn take_previous_chars(text: &str, count: usize) -> &str {
    let mut byte_start = text.len();
    for _ in 0..count {
        match text[..byte_start].char_indices().next_back() {
            Some((index, _)) => byte_start = index,
            None => break,
        }
    }
    &text[byte_start..]
}

fn take_next_chars(text: &str, count: usize) -> &str {
    let mut chars = text.char_indices();
    let mut end = 0;
    for _ in 0..count {
        match chars.next() {
            Some((_, character)) => end += character.len_utf8(),
            None => break,
        }
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancellationToken;
    use crate::config::CustomRuleConfig;
    use crate::document::{Document, DocumentKind};

    fn rule(config: CustomRuleConfig) -> Box<dyn Rule> {
        if config.resolved_type() == "pair" {
            Box::new(PairRule::new(&config).unwrap())
        } else {
            Box::new(CustomRegexRule::new(&config).unwrap())
        }
    }

    fn run(config: CustomRuleConfig, text: &str) -> Vec<Issue> {
        let document = Document::from_text(text, DocumentKind::Text);
        let global = crate::config::Config::default();
        let token = CancellationToken::new();
        let context = RuleContext {
            document: &document,
            config: &global,
            include_context: true,
            token: &token,
            cancellation_enabled: true,
        };
        rule(config).check(&context)
    }

    #[test]
    fn pair_rule_respects_context() {
        let config = CustomRuleConfig {
            id: "CUSTOM_DE".to_owned(),
            rule_type: "pair".to_owned(),
            wrong: "的".to_owned(),
            right: "地".to_owned(),
            when_prev: vec!["快速".to_owned()],
            when_next: vec!["走".to_owned()],
            ..CustomRuleConfig::default()
        };
        assert_eq!(run(config.clone(), "他快速的走开了").len(), 1);
        assert!(run(config, "他快速的思考").is_empty());
    }

    #[test]
    fn pair_rule_respects_english_boundaries() {
        let config = CustomRuleConfig {
            id: "CUSTOM_API".to_owned(),
            rule_type: "pair".to_owned(),
            wrong: "API".to_owned(),
            right: "接口".to_owned(),
            ..CustomRuleConfig::default()
        };
        assert_eq!(run(config.clone(), "API 很好用").len(), 1);
        assert!(run(config, "APIs are fine").is_empty());
    }
}
