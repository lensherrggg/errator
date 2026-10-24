use std::collections::BTreeMap;

use crate::config::Config;
use crate::model::{Issue, Severity, Source};
use crate::rules::{Rule, RuleContext};

const BUILTIN_GROUPS: &[&[&str]] = &[
    &["登录", "登陆"],
    &["账号", "帐号"],
    &["其他", "其它"],
    &["成分", "成份"],
    &["笔画", "笔划"],
    &["报道", "报导"],
    &["唯一", "惟一"],
    &["做出", "作出"],
    &["林荫", "林阴"],
];

/// Flags inconsistent use of a term *only when more than one variant occurs in
/// the same document*. A document that exclusively uses the non-preferred form
/// is left alone; consistency, not prescriptivism.
pub struct ConsistencyRule {
    groups: Vec<Vec<String>>,
}

impl ConsistencyRule {
    pub fn from_config(config: &Config) -> Self {
        let mut groups: Vec<Vec<String>> = Vec::new();
        if config.consistency.use_builtin {
            groups.extend(
                BUILTIN_GROUPS
                    .iter()
                    .map(|group| group.iter().map(|term| (*term).to_owned()).collect()),
            );
        }
        for group in &config.consistency.groups {
            let cleaned: Vec<String> = group
                .iter()
                .map(|term| term.trim().to_owned())
                .filter(|term| !term.is_empty())
                .collect();
            if cleaned.len() >= 2 {
                groups.push(cleaned);
            }
        }
        Self { groups }
    }
}

impl Rule for ConsistencyRule {
    fn id(&self) -> &str {
        "TERM_VARIANT"
    }

    fn category(&self) -> &str {
        "consistency"
    }

    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn description(&self) -> &str {
        "同一文档内术语写法不一致"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();

        for group in &self.groups {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            let mut positions: Vec<(&str, usize)> = Vec::new();
            for term in group {
                for (offset, _) in document.text.match_indices(term) {
                    if document.is_masked(offset, term.len()) {
                        continue;
                    }
                    *counts.entry(term.as_str()).or_default() += 1;
                    positions.push((term.as_str(), offset));
                }
            }
            if positions.len() < 2 || counts.len() < 2 {
                continue;
            }

            // Preferred term is the explicitly listed first variant if it is
            // present; otherwise use the most frequent variant.
            let preferred = if counts.contains_key(group[0].as_str()) {
                group[0].as_str()
            } else {
                counts
                    .iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(term, _)| *term)
                    .unwrap_or(group[0].as_str())
            };

            let used: Vec<&str> = counts.keys().copied().collect();
            for (term, offset) in &positions {
                if context.cancelled() {
                    return issues;
                }
                if *term == preferred {
                    continue;
                }
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    self.default_severity(),
                    *offset,
                    *term,
                    preferred,
                    format!(
                        "全文混用了「{}」；建议统一为「{}」",
                        used.join("」「"),
                        preferred
                    ),
                    Source::Rule,
                    0.85,
                ));
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancellationToken;
    use crate::document::{Document, DocumentKind};

    fn check(text: &str) -> Vec<Issue> {
        let document = Document::from_text(text, DocumentKind::Text);
        let config = Config::default();
        let token = CancellationToken::new();
        let context = RuleContext {
            document: &document,
            config: &config,
            include_context: true,
            token: &token,
            cancellation_enabled: true,
        };
        ConsistencyRule::from_config(&config).check(&context)
    }

    #[test]
    fn flags_only_mixed_usage() {
        assert!(check("请先登录账号。\n").is_empty());
        let issues = check("请先登录账号，然后查看帐号设置。\n");
        assert!(!issues.is_empty());
        assert!(issues.iter().all(|issue| issue.suggestion == "账号"));
        assert!(check("全文只用登陆这个词，也不强制改写。\n").is_empty());
    }
}
