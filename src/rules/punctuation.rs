use crate::document::{is_ascii_alphanumeric, is_cjk, is_cjk_punctuation};
use crate::model::{Issue, Severity, Source};
use crate::rules::{Rule, RuleContext};

fn ascii_to_fullwidth(character: char) -> Option<char> {
    Some(match character {
        ',' => '，',
        '.' => '。',
        ':' => '：',
        ';' => '；',
        '?' => '？',
        '!' => '！',
        '(' => '（',
        ')' => '）',
        _ => return None,
    })
}

fn fullwidth_to_ascii(character: char) -> Option<char> {
    Some(match character {
        '，' => ',',
        '。' => '.',
        '：' => ':',
        '；' => ';',
        '？' => '?',
        '！' => '!',
        '（' => '(',
        '）' => ')',
        _ => return None,
    })
}

fn chinese_context(character: Option<char>) -> bool {
    character
        .map(|character| is_cjk(character) || is_cjk_punctuation(character))
        .unwrap_or(false)
}

fn ascii_word(character: Option<char>) -> bool {
    character.map(is_ascii_alphanumeric).unwrap_or(false)
}

pub struct HalfWidthInChineseRule;

impl Rule for HalfWidthInChineseRule {
    fn id(&self) -> &str {
        "PUNCT_HALF_WIDTH"
    }

    fn category(&self) -> &str {
        "punctuation"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn description(&self) -> &str {
        "中文语境的半角标点，建议使用全角标点"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for line in document.text.split_inclusive('\n') {
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            for (index, (relative, character)) in chars.iter().enumerate() {
                if context.cancelled() {
                    return issues;
                }
                let Some(fullwidth) = ascii_to_fullwidth(*character) else {
                    continue;
                };
                let previous = index.checked_sub(1).map(|i| chars[i].1);
                let next = chars.get(index + 1).map(|(_, c)| *c);
                let should_replace = match character {
                    '(' => next.map(is_cjk).unwrap_or(false) && !ascii_word(previous),
                    ')' => previous.map(is_cjk).unwrap_or(false) && !ascii_word(next),
                    '.' => {
                        chinese_context(previous)
                            && (next.is_none()
                                || next.map(char::is_whitespace).unwrap_or(false)
                                || chinese_context(next))
                    }
                    '?' | '!' => {
                        previous.map(is_cjk).unwrap_or(false)
                            && (next.is_none()
                                || next.map(char::is_whitespace).unwrap_or(false)
                                || chinese_context(next))
                    }
                    ',' | ':' | ';' => {
                        (chinese_context(previous) || chinese_context(next))
                            && !(ascii_word(previous) && ascii_word(next))
                    }
                    _ => false,
                };
                if !should_replace {
                    continue;
                }
                let start = line_start + relative;
                if document.is_masked(start, character.len_utf8()) {
                    continue;
                }
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    self.default_severity(),
                    start,
                    character.to_string(),
                    fullwidth.to_string(),
                    format!("中文语境建议使用全角标点「{fullwidth}」"),
                    Source::Rule,
                    0.9,
                ));
            }
        }
        issues
    }
}

pub struct FullWidthInEnglishRule;

impl Rule for FullWidthInEnglishRule {
    fn id(&self) -> &str {
        "PUNCT_FULL_WIDTH"
    }

    fn category(&self) -> &str {
        "punctuation"
    }

    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn description(&self) -> &str {
        "英文语境的标点建议使用半角形式"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for line in document.text.split_inclusive('\n') {
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            for (index, (relative, character)) in chars.iter().enumerate() {
                if context.cancelled() {
                    return issues;
                }
                let Some(halfwidth) = fullwidth_to_ascii(*character) else {
                    continue;
                };
                let previous = index.checked_sub(1).map(|i| chars[i].1);
                let next = chars.get(index + 1).map(|(_, c)| *c);
                let should_replace = match character {
                    '（' => {
                        next.map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
                            && !previous.map(is_cjk).unwrap_or(false)
                    }
                    '）' => {
                        previous.map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
                            && !next.map(is_cjk).unwrap_or(false)
                    }
                    '，' | '：' | '；' | '？' | '！' | '。' => {
                        ascii_word(previous) && ascii_word(next)
                    }
                    _ => false,
                };
                if !should_replace {
                    continue;
                }
                let start = line_start + relative;
                if document.is_masked(start, character.len_utf8()) {
                    continue;
                }
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    self.default_severity(),
                    start,
                    character.to_string(),
                    halfwidth.to_string(),
                    format!("英文语境建议使用半角标点「{halfwidth}」"),
                    Source::Rule,
                    0.7,
                ));
            }
        }
        issues
    }
}

pub struct DuplicatePunctuationRule;

impl Rule for DuplicatePunctuationRule {
    fn id(&self) -> &str {
        "PUNCT_DUPLICATE"
    }

    fn category(&self) -> &str {
        "punctuation"
    }

    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn description(&self) -> &str {
        "重复的标点符号（如「。。」）"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for line in document.text.split_inclusive('\n') {
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            let mut index = 0;
            while index + 1 < chars.len() {
                if context.cancelled() {
                    return issues;
                }
                let (relative, character) = chars[index];
                let (_, following) = chars[index + 1];
                if matches!(character, '，' | '。' | '、' | '；' | '：' | '！' | '？')
                    && character == following
                {
                    let start = line_start + relative;
                    if document.is_masked(start, character.len_utf8() * 2) {
                        index += 2;
                        continue;
                    }
                    let original = format!("{character}{following}");
                    issues.push(context.issue(
                        self.id(),
                        self.category(),
                        self.default_severity(),
                        start,
                        original,
                        character.to_string(),
                        "连续标点疑似重复",
                        Source::Rule,
                        0.8,
                    ));
                    index += 2;
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
    use crate::cancel::CancellationToken;
    use crate::config::Config;
    use crate::document::{Document, DocumentKind};
    use crate::rules::Rule;

    use super::*;

    fn check(rule: &dyn Rule, text: &str) -> Vec<Issue> {
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
        rule.check(&context)
    }

    #[test]
    fn flags_halfwidth_comma_in_chinese() {
        let issues = check(&HalfWidthInChineseRule, "这是一个错误,需要修改。\n");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].suggestion, "，");
    }

    #[test]
    fn does_not_flag_english_or_numeric_punctuation() {
        assert!(check(&HalfWidthInChineseRule, "Hello, world.\n").is_empty());
        assert!(check(&HalfWidthInChineseRule, "1,000 and 3.14\n").is_empty());
        assert!(check(&HalfWidthInChineseRule, "e.g. example\n").is_empty());
    }

    #[test]
    fn flags_repeated_chinese_punctuation() {
        let issues = check(&DuplicatePunctuationRule, "真的吗。。\n");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].original, "。。");
    }
}
