use std::collections::BTreeSet;

use anyhow::Result;

use crate::cancel::CancellationToken;
use crate::config::Config;
use crate::document::{Document, DocumentKind};
use crate::rules::{CustomRegexRule, PairRule, Rule, RuleContext};

#[derive(Debug, Clone)]
pub struct RuleTestCase {
    pub rule_id: String,
    pub input: String,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
    pub passed: bool,
}

/// Run every `[[rules.custom.tests]]` case against the set of custom rules.
/// This is intentionally independent of built-in rules so a ruleset's tests
/// stay stable as built-in behavior evolves.
pub fn run_custom_tests(config: &Config) -> Result<Vec<RuleTestCase>> {
    let mut rules: Vec<Box<dyn Rule>> = Vec::new();
    for custom in config.rules.custom.iter().filter(|custom| custom.enabled) {
        match custom.resolved_type() {
            "pair" => rules.push(Box::new(PairRule::new(custom)?)),
            "regex" => rules.push(Box::new(CustomRegexRule::new(custom)?)),
            other => anyhow::bail!("自定义规则 `{}` 类型无效：{other}", custom.id),
        }
    }

    let token = CancellationToken::new();
    let mut results = Vec::new();
    for custom in config.rules.custom.iter().filter(|custom| custom.enabled) {
        for test in &custom.tests {
            let document = Document::from_text(test.input.clone(), DocumentKind::Text);
            let context = RuleContext {
                document: &document,
                config,
                include_context: false,
                token: &token,
                cancellation_enabled: false,
            };
            let mut actual = BTreeSet::new();
            for rule in &rules {
                for issue in rule.check(&context) {
                    actual.insert(issue.rule_id);
                }
            }
            let actual: Vec<String> = actual.into_iter().collect();
            let expected: BTreeSet<String> = test.expect.iter().cloned().collect();
            let expected_vec: Vec<String> = expected.iter().cloned().collect();
            let passed = actual == expected_vec;
            results.push(RuleTestCase {
                rule_id: custom.id.clone(),
                input: test.input.clone(),
                expected: expected_vec,
                actual,
                passed,
            });
        }
    }
    Ok(results)
}
