use crate::document::{is_ascii_alphanumeric, is_cjk};
use crate::model::{Severity, Source};
use crate::rules::{Rule, RuleContext};

pub struct MissingSpaceRule;

impl Rule for MissingSpaceRule {
    fn id(&self) -> &str {
        "CJK_SPACE_MISSING"
    }

    fn category(&self) -> &str {
        "spacing"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn description(&self) -> &str {
        "中文与英文/数字之间缺少空格（docx 默认为 info）"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<crate::model::Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        let severity = if document.kind == crate::document::DocumentKind::Docx {
            Severity::Info
        } else {
            self.default_severity()
        };

        for line in document.text.split_inclusive('\n') {
            // Offsets are tracked relative to the whole document.
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            if chars.is_empty() {
                continue;
            }
            let mut index = 0;
            while index < chars.len() {
                if context.cancelled() {
                    return issues;
                }
                let (relative, character) = chars[index];
                if !(is_cjk(character) || is_ascii_alphanumeric(character)) {
                    index += 1;
                    continue;
                }
                let start_index = index;
                while index < chars.len() {
                    let (_, value) = chars[index];
                    if is_cjk(value) || is_ascii_alphanumeric(value) {
                        index += 1;
                    } else {
                        break;
                    }
                }
                let end_index = index;
                if end_index - start_index < 2 {
                    continue;
                }
                let absolute_start = line_start + relative;
                let absolute_end =
                    line_start + chars[end_index - 1].0 + chars[end_index - 1].1.len_utf8();
                let original = &document.text[absolute_start..absolute_end];
                if !has_both_scripts(original) {
                    continue;
                }
                if document.is_masked(absolute_start, original.len()) {
                    continue;
                }
                let suggestion = insert_spaces(original);
                if suggestion == original {
                    continue;
                }
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    severity,
                    absolute_start,
                    original,
                    suggestion,
                    "中英文/数字之间建议加显式空格",
                    Source::Rule,
                    1.0,
                ));
            }
        }

        issues
    }
}

fn has_both_scripts(token: &str) -> bool {
    let mut has_cjk = false;
    let mut has_ascii = false;
    for character in token.chars() {
        has_cjk |= is_cjk(character);
        has_ascii |= is_ascii_alphanumeric(character);
        if has_cjk && has_ascii {
            return true;
        }
    }
    false
}

pub fn insert_spaces(token: &str) -> String {
    let mut result = String::with_capacity(token.len() + 8);
    let mut previous: Option<char> = None;
    for character in token.chars() {
        if let Some(previous) = previous {
            let previous_script = is_cjk(previous) || is_ascii_alphanumeric(previous);
            let current_script = is_cjk(character) || is_ascii_alphanumeric(character);
            let scripts_differ = (is_cjk(previous) && is_ascii_alphanumeric(character))
                || (is_ascii_alphanumeric(previous) && is_cjk(character));
            if previous_script && current_script && scripts_differ {
                result.push(' ');
            }
        }
        result.push(character);
        previous = Some(character);
    }
    result
}

pub struct ExtraSpaceRule;

impl Rule for ExtraSpaceRule {
    fn id(&self) -> &str {
        "CJK_SPACE_EXTRA"
    }

    fn category(&self) -> &str {
        "spacing"
    }

    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn description(&self) -> &str {
        "中文字符之间存在多余空格"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<crate::model::Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for line in document.text.split_inclusive('\n') {
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            let mut index = 0;
            while index < chars.len() {
                if context.cancelled() {
                    return issues;
                }
                if !is_cjk(chars[index].1) {
                    index += 1;
                    continue;
                }
                let left = chars[index];
                let mut cursor = index + 1;
                let mut spaces = 0;
                while cursor < chars.len() && chars[cursor].1 == ' ' {
                    spaces += 1;
                    cursor += 1;
                }
                if spaces > 0 && cursor < chars.len() && is_cjk(chars[cursor].1) {
                    let end = chars[cursor].0 + chars[cursor].1.len_utf8();
                    let start = line_start + left.0;
                    let end_absolute = line_start + end;
                    if document.is_masked(start, end_absolute - start) {
                        index = cursor;
                        continue;
                    }
                    let original = &document.text[start..end_absolute];
                    let suggestion = format!("{}{}", left.1, chars[cursor].1);
                    issues.push(context.issue(
                        self.id(),
                        self.category(),
                        self.default_severity(),
                        start,
                        original,
                        suggestion,
                        "中文字符之间通常不需要空格",
                        Source::Rule,
                        0.9,
                    ));
                    index = cursor + 1;
                } else {
                    index += 1;
                }
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_spaces_at_script_boundaries() {
        assert_eq!(insert_spaces("使用Rust开发"), "使用 Rust 开发");
        assert_eq!(insert_spaces("版本v0.5"), "版本 v0.5");
    }

    #[test]
    fn detects_mixed_script_token() {
        let doc = crate::document::Document::from_text(
            "使用Rust开发\n",
            crate::document::DocumentKind::Text,
        );
        let config = crate::config::Config::default();
        let token = crate::cancel::CancellationToken::new();
        let context = RuleContext {
            document: &doc,
            config: &config,
            include_context: true,
            token: &token,
            cancellation_enabled: true,
        };
        let issues = MissingSpaceRule.check(&context);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].original, "使用Rust开发");
        assert_eq!(issues[0].suggestion, "使用 Rust 开发");
    }
}
