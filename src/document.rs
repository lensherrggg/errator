use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::model::Location;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Text,
    Markdown,
    Docx,
}

impl DocumentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Docx => "docx",
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match ext.as_str() {
            "txt" => Ok(Self::Text),
            "md" | "markdown" => Ok(Self::Markdown),
            "docx" => Ok(Self::Docx),
            _ => bail!(
                "不支持的文件类型：{}（支持 .txt / .md / .docx）",
                path.display()
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct ParagraphSpan {
    start: usize,
    end: usize,
}

/// An extracted source document with stable position mapping and protected
/// ranges (code, URLs, front matter). Rules never inspect protected ranges.
#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    pub display: String,
    pub kind: DocumentKind,
    pub text: String,
    line_starts: Vec<usize>,
    masks: Vec<Range<usize>>,
    paragraphs: Vec<ParagraphSpan>,
}

impl Document {
    pub fn load(path: &Path) -> Result<Self> {
        let kind = DocumentKind::from_path(path)?;
        let (text, paragraphs) = match kind {
            DocumentKind::Text | DocumentKind::Markdown => (read_text_file(path)?, Vec::new()),
            DocumentKind::Docx => read_docx(path)?,
        };
        let display = paths::display_path_relative_to_cwd(path);
        let masks = build_masks(kind, &text);
        let line_starts = build_line_starts(&text);
        Ok(Self {
            path: path.to_owned(),
            display,
            kind,
            text,
            line_starts,
            masks,
            paragraphs,
        })
    }

    /// Mainly for unit tests and the TUI preview.
    pub fn from_text(text: impl Into<String>, kind: DocumentKind) -> Self {
        let text = text.into();
        let masks = build_masks(kind, &text);
        Self {
            path: PathBuf::from("inline.txt"),
            display: "inline.txt".to_owned(),
            kind,
            line_starts: build_line_starts(&text),
            masks,
            text,
            paragraphs: Vec::new(),
        }
    }

    pub fn is_masked(&self, start: usize, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        let end = start.saturating_add(len);
        self.masks
            .iter()
            .any(|range| range.start < end && start < range.end)
    }

    pub fn location(&self, offset: usize, with_context: bool) -> Location {
        let offset = offset.min(self.text.len());
        match self.kind {
            DocumentKind::Text | DocumentKind::Markdown => {
                let line_index = self
                    .line_starts
                    .partition_point(|start| *start <= offset)
                    .saturating_sub(1);
                let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
                let line_end = self
                    .line_starts
                    .get(line_index + 1)
                    .copied()
                    .unwrap_or(self.text.len());
                let column = self.text[line_start..offset].chars().count() + 1;
                let context = with_context.then(|| {
                    clip_context(
                        self.text[line_start..line_end].trim_end_matches(['\r', '\n']),
                        offset.saturating_sub(line_start),
                    )
                });
                Location::Text {
                    line: line_index + 1,
                    column,
                    context,
                }
            }
            DocumentKind::Docx => {
                let index = self
                    .paragraphs
                    .partition_point(|span| span.start <= offset)
                    .saturating_sub(1);
                let Some(span) = self.paragraphs.get(index) else {
                    return Location::Docx {
                        paragraph: 1,
                        char_offset: 1,
                        context: None,
                    };
                };
                let paragraph = &self.text[span.start..span.end];
                let within = offset.min(span.end).saturating_sub(span.start);
                let context = with_context.then(|| clip_context(paragraph, within));
                Location::Docx {
                    paragraph: index + 1,
                    char_offset: paragraph[..within.min(paragraph.len())].chars().count() + 1,
                    context,
                }
            }
        }
    }

    pub fn context_for(&self, offset: usize) -> Option<String> {
        self.location(offset, true).context().map(str::to_owned)
    }

    /// Number of lines, used by progress/status displays.
    pub fn line_count(&self) -> usize {
        self.text.lines().count()
    }

    /// 1-based line and text content without line terminators. Used by the
    /// external-checker protocol.
    pub fn text_segments(&self) -> Vec<(usize, &str)> {
        self.text
            .split_inclusive('\n')
            .enumerate()
            .map(|(index, line)| (index + 1, line.trim_end_matches(['\r', '\n'])))
            .collect()
    }

    /// 1-based paragraph index and paragraph text. docx only.
    pub fn paragraph_segments(&self) -> Vec<(usize, &str)> {
        self.paragraphs
            .iter()
            .enumerate()
            .map(|(index, span)| (index + 1, &self.text[span.start..span.end]))
            .collect()
    }

    /// Map a reported location back to a byte offset for stable sorting and
    /// context recovery. Invalid locations fall back to offset zero.
    pub fn offset_for_location(&self, location: &Location) -> usize {
        match location {
            Location::Text { line, column, .. } => {
                let Some(line_start) = self.line_starts.get(line.saturating_sub(1)).copied() else {
                    return 0;
                };
                let line_end = self
                    .line_starts
                    .get(*line)
                    .copied()
                    .unwrap_or(self.text.len());
                let mut offset = line_start;
                for (count, (relative, character)) in
                    self.text[line_start..line_end].char_indices().enumerate()
                {
                    if count + 1 >= *column {
                        break;
                    }
                    offset = line_start + relative + character.len_utf8();
                }
                offset.min(line_end)
            }
            Location::Docx {
                paragraph,
                char_offset,
                ..
            } => {
                let Some(span) = self.paragraphs.get(paragraph.saturating_sub(1)) else {
                    return 0;
                };
                let mut offset = span.start;
                for (count, (relative, character)) in
                    self.text[span.start..span.end].char_indices().enumerate()
                {
                    if count + 1 >= *char_offset {
                        break;
                    }
                    offset = span.start + relative + character.len_utf8();
                }
                offset.min(span.end)
            }
        }
    }

    /// Fill in a missing context string from document content. External
    /// checkers are not required to echo context back.
    pub fn complete_location(&self, mut location: Location) -> Location {
        if location.context().is_some() {
            return location;
        }
        let offset = self.offset_for_location(&location);
        let context = self.location(offset, true).context().map(str::to_owned);
        match &mut location {
            Location::Text {
                context: target, ..
            }
            | Location::Docx {
                context: target, ..
            } => *target = context,
        }
        location
    }
}

fn build_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn clip_context(line: &str, anchor_byte: usize) -> String {
    const CONTEXT_MAX_CHARS: usize = 400;
    const CONTEXT_BEFORE_CHARS: usize = 96;

    let line = line.replace(['\r', '\n', '\t'], " ");
    if line.chars().count() <= CONTEXT_MAX_CHARS {
        return line.trim().to_owned();
    }
    let anchor_chars = line
        .get(..anchor_byte.min(line.len()))
        .map(|prefix| prefix.chars().count())
        .unwrap_or(0);
    let chars: Vec<char> = line.chars().collect();
    let start = anchor_chars.saturating_sub(CONTEXT_BEFORE_CHARS);
    let end = (start + CONTEXT_MAX_CHARS).min(chars.len());
    let start = end.saturating_sub(CONTEXT_MAX_CHARS);
    let mut result = String::new();
    if start > 0 {
        result.push('…');
    }
    result.extend(&chars[start..end]);
    if end < chars.len() {
        result.push('…');
    }
    result.trim().to_owned()
}

fn read_text_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("无法读取 {}", path.display()))?;
    decode_text(&bytes).with_context(|| {
        format!(
            "{} 不是有效的 UTF-8/UTF-16 文本，请先另存为 UTF-8",
            path.display()
        )
    })
}

fn decode_text(bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8(rest.to_vec())?);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        if rest.len() % 2 != 0 {
            bail!("UTF-16LE 数据长度为奇数");
        }
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        return String::from_utf16(&units).context("UTF-16LE 解码失败");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        if rest.len() % 2 != 0 {
            bail!("UTF-16BE 数据长度为奇数");
        }
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        return String::from_utf16(&units).context("UTF-16BE 解码失败");
    }
    Ok(String::from_utf8(bytes.to_vec())?)
}

fn read_docx(path: &Path) -> Result<(String, Vec<ParagraphSpan>)> {
    let file = File::open(path).with_context(|| format!("无法读取 {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("{} 不是有效的 docx（zip）文件", path.display()))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .with_context(|| format!("{} 缺少 word/document.xml", path.display()))?
        .read_to_string(&mut xml)
        .with_context(|| format!("无法读取 {} 的正文 XML", path.display()))?;
    let parsed = roxmltree::Document::parse(&xml)
        .with_context(|| format!("{} 的正文 XML 无法解析", path.display()))?;

    let mut text = String::new();
    let mut paragraphs = Vec::new();
    let mut found_paragraph = false;

    for paragraph in parsed
        .root()
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "p")
    {
        found_paragraph = true;
        let start = text.len();
        for node in paragraph.descendants().filter(|node| node.is_element()) {
            match node.tag_name().name() {
                "t" => {
                    if let Some(value) = node.text() {
                        text.push_str(value);
                    }
                }
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                "noBreakHyphen" => text.push('-'),
                _ => {}
            }
        }
        let end = text.len();
        paragraphs.push(ParagraphSpan { start, end });
        text.push('\n');
    }

    if !found_paragraph {
        // Extremely minimal documents can still have loose w:t nodes.
        for node in parsed
            .root()
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "t")
        {
            if let Some(value) = node.text() {
                text.push_str(value);
            }
        }
        paragraphs.push(ParagraphSpan {
            start: 0,
            end: text.len(),
        });
        text.push('\n');
    }

    Ok((text, paragraphs))
}

/// Determine which byte ranges must not be touched by prose rules.
fn build_masks(kind: DocumentKind, text: &str) -> Vec<Range<usize>> {
    let mut masks = Vec::new();
    if kind == DocumentKind::Markdown {
        mask_markdown(text, &mut masks);
    }
    mask_urls(text, &mut masks);
    if kind == DocumentKind::Markdown {
        mask_inline_code(text, &mut masks);
        mask_link_destinations(text, &mut masks);
        mask_html_comments(text, &mut masks);
    } else {
        mask_html_comments(text, &mut masks);
    }
    masks.sort_by_key(|range| (range.start, range.end));
    merge_ranges(masks)
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push(start..index + 1);
            start = index + 1;
        }
    }
    if start < text.len() {
        ranges.push(start..text.len());
    }
    ranges
}

fn mask_markdown(text: &str, masks: &mut Vec<Range<usize>>) {
    let lines = line_ranges(text);
    let mut fence: Option<(char, usize)> = None;
    let mut start_line = 0;

    if let Some(first) = lines.first() {
        let line = text[first.clone()].trim();
        if line == "---" {
            for (index, range) in lines.iter().enumerate().skip(1) {
                let value = text[range.clone()].trim();
                if value == "---" || value == "..." {
                    masks.push(0..range.end);
                    start_line = index + 1;
                    break;
                }
            }
        }
    }

    let mut index = start_line;
    while index < lines.len() {
        let range = lines[index].clone();
        let line = &text[range.clone()];
        let trimmed = line.trim_start();
        if let Some((fence_char, fence_len)) = fence {
            if is_closing_fence(trimmed, fence_char, fence_len) {
                masks.push(range.start..range.end);
                fence = None;
            } else {
                masks.push(range.clone());
            }
        } else if let Some((fence_char, fence_len)) = opening_fence(trimmed) {
            masks.push(range.start..range.end);
            fence = Some((fence_char, fence_len));
        }
        index += 1;
    }
    if fence.is_some()
        && let Some(last) = lines.last()
    {
        masks.push(last.start..last.end);
    }
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (len >= 3).then_some((marker, len))
}

fn is_closing_fence(line: &str, marker: char, minimum: usize) -> bool {
    let len = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    len >= minimum && line[len..].trim().is_empty()
}

fn mask_inline_code(text: &str, masks: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'`' || is_inside_masks(index, masks) {
            index += 1;
            continue;
        }
        let run_start = index;
        while index < bytes.len() && bytes[index] == b'`' {
            index += 1;
        }
        let run_len = index - run_start;
        let mut search = index;
        let mut found = None;
        while search < bytes.len() {
            if is_inside_masks(search, masks) {
                search += 1;
                continue;
            }
            if bytes[search] == b'`' {
                let candidate_start = search;
                while search < bytes.len() && bytes[search] == b'`' {
                    search += 1;
                }
                if search - candidate_start == run_len {
                    found = Some(search);
                    break;
                }
            } else {
                search += 1;
            }
        }
        if let Some(end) = found {
            masks.push(run_start..end);
            index = end;
        }
    }
}

fn mask_link_destinations(text: &str, masks: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b']' && bytes[index + 1] == b'(' && !is_inside_masks(index, masks) {
            let start = index;
            let mut depth = 0usize;
            let mut end = None;
            for (offset, byte) in bytes.iter().enumerate().skip(index + 1) {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = Some(offset + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(end) = end {
                masks.push(start..end);
                index = end;
                continue;
            }
        }
        index += 1;
    }
}

fn mask_html_comments(text: &str, masks: &mut Vec<Range<usize>>) {
    let mut search_from = 0;
    while let Some(start) = text[search_from..].find("<!--") {
        let start = search_from + start;
        let Some(relative_end) = text[start + 4..].find("-->") else {
            masks.push(start..text.len());
            break;
        };
        let end = start + 4 + relative_end + 3;
        masks.push(start..end);
        search_from = end;
    }
}

fn mask_urls(text: &str, masks: &mut Vec<Range<usize>>) {
    for prefix in ["http://", "https://", "www."] {
        let mut from = 0;
        while let Some(relative) = text[from..].find(prefix) {
            let start = from + relative;
            let mut end = start;
            for (offset, character) in text[start..].char_indices() {
                if character.is_whitespace()
                    || matches!(character, '<' | '>' | '"' | '“' | '”' | '）' | '】')
                {
                    break;
                }
                end = start + offset + character.len_utf8();
            }
            while end > start {
                let last = text[..end].chars().next_back().unwrap_or(' ');
                if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
                    end -= last.len_utf8();
                } else {
                    break;
                }
            }
            if end > start {
                masks.push(start..end);
            }
            from = end.max(start + prefix.len()).min(text.len());
        }
    }
}

fn is_inside_masks(offset: usize, masks: &[Range<usize>]) -> bool {
    masks
        .iter()
        .any(|range| range.start <= offset && offset < range.end)
}

fn merge_ranges(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if range.start >= range.end {
            continue;
        }
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

/// Expand a user path into supported files. Directory scans are recursive and
/// deterministic (case-insensitive path sort).
pub fn collect_inputs(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        bail!("路径不存在：{}", path.display());
    }
    if path.is_file() {
        DocumentKind::from_path(path)?;
        return Ok(vec![path.to_owned()]);
    }
    if !path.is_dir() {
        bail!("不是文件或目录：{}", path.display());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.with_context(|| format!("遍历目录失败：{}", path.display()))?;
        if entry.file_type().is_file() && paths::is_supported_extension(entry.path()) {
            files.push(entry.into_path());
        }
    }
    files.sort_by_cached_key(|path| paths::display_path(path).to_ascii_lowercase());
    if files.is_empty() {
        bail!(
            "目录中没有可检查的 .txt / .md / .docx 文件：{}",
            path.display()
        );
    }
    Ok(files)
}

pub fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
            | 0x30000..=0x323AF
    )
}

pub fn is_ascii_alphanumeric(character: char) -> bool {
    character.is_ascii_alphanumeric()
}

pub fn is_cjk_punctuation(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '？'
            | '！'
            | '（'
            | '）'
            | '【'
            | '】'
            | '《'
            | '》'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '—'
            | '…'
            | '·'
            | '「'
            | '」'
            | '『'
            | '』'
            | '〈'
            | '〉'
            | '～'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_location_is_one_based_and_uses_chars() {
        let doc = Document::from_text("第一行\n使用Rust开发\n", DocumentKind::Text);
        let offset = doc.text.find("Rust").unwrap();
        let location = doc.location(offset, true);
        match location {
            Location::Text {
                line,
                column,
                context,
            } => {
                assert_eq!(line, 2);
                assert_eq!(column, 3);
                assert_eq!(context.as_deref(), Some("使用Rust开发"));
            }
            _ => panic!("expected text location"),
        }
    }

    #[test]
    fn markdown_fenced_code_is_masked() {
        let doc = Document::from_text(
            "# 标题\n\n```rust\nlet x=使用Rust开发;\n```\n\n正文Rust开发\n",
            DocumentKind::Markdown,
        );
        let code_offset = doc.text.find("使用Rust").unwrap();
        assert!(doc.is_masked(code_offset, "使用Rust".len()));
        let prose_offset = doc.text.rfind("Rust").unwrap();
        assert!(!doc.is_masked(prose_offset, "Rust".len()));
    }

    #[test]
    fn inline_code_and_urls_are_masked() {
        let doc = Document::from_text(
            "`使用Rust开发` 与 https://example.com/中文Rust 结束",
            DocumentKind::Markdown,
        );
        assert!(doc.is_masked(doc.text.find("使用Rust").unwrap(), 4));
        assert!(doc.is_masked(doc.text.find("https").unwrap(), 4));
    }

    #[test]
    fn long_context_keeps_a_wide_window_around_the_issue() {
        let text = format!("{}目标错字{}", "前".repeat(300), "后".repeat(300));
        let doc = Document::from_text(text, DocumentKind::Text);
        let offset = doc.text.find("目标错字").unwrap();
        let context = doc.context_for(offset).unwrap();
        assert!(context.contains("目标错字"));
        assert!(context.chars().count() > 300);
        assert!(context.chars().count() <= 402);
    }
}
