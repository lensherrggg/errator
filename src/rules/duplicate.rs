use crate::document::is_cjk;
use crate::model::{Issue, Severity, Source};
use crate::rules::{Rule, RuleContext};

/// Only characters that are almost never legitimately doubled in prose are
/// included. Common reduplications such as 人人、常常、看看 are intentionally
/// absent to keep the signal usable.
const SENSITIVE: &[char] = &[
    '的', '了', '是', '在', '和', '与', '我', '你', '他', '她', '它', '这', '那', '有', '不', '就',
    '也', '都', '又', '而', '及', '或', '很', '最', '更', '已', '并', '把', '被', '给', '让', '从',
    '到', '对', '为', '以', '于', '之', '其', '该', '此', '者',
];

pub struct DuplicateCharRule;

impl Rule for DuplicateCharRule {
    fn id(&self) -> &str {
        "CHAR_DUPLICATE"
    }

    fn category(&self) -> &str {
        "duplication"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn description(&self) -> &str {
        "疑似重复输入的汉字（如「的的」）"
    }

    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue> {
        let document = context.document;
        let mut issues = Vec::new();
        for line in document.text.split_inclusive('\n') {
            let line_start = line.as_ptr() as usize - document.text.as_ptr() as usize;
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            for window in chars.windows(2) {
                if context.cancelled() {
                    return issues;
                }
                let (first_offset, first) = window[0];
                let (_, second) = window[1];
                if first != second || !is_cjk(first) || !SENSITIVE.contains(&first) {
                    continue;
                }
                let offset = line_start + first_offset;
                let length = first.len_utf8() * 2;
                if document.is_masked(offset, length) {
                    continue;
                }
                let original = format!("{first}{second}");
                issues.push(context.issue(
                    self.id(),
                    self.category(),
                    self.default_severity(),
                    offset,
                    original,
                    first.to_string(),
                    format!("汉字「{first}」连续出现，疑似重复"),
                    Source::Rule,
                    0.8,
                ));
            }
        }
        issues
    }
}
