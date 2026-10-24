use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use crate::cancel::CancellationToken;
use crate::document::{Document, is_ascii_alphanumeric, is_cjk};
use crate::model::{Issue, Severity, Source};
use crate::semantic::{BackendSettings, DeviceKind, SemanticBackend};

const MAX_SEQ_LEN: usize = 512;

pub fn create_backend(
    path: &Path,
    device: DeviceKind,
    settings: &BackendSettings,
) -> Result<Box<dyn SemanticBackend>> {
    let tokenizer = settings.tokenizer.trim().to_ascii_lowercase();
    if !(tokenizer.contains("bert") || tokenizer.contains("wordpiece") || tokenizer.is_empty()) {
        bail!(
            "不支持的语义分词器 `{}`；v0.5 支持 BERT WordPiece（MacBERT / bert-base-chinese）",
            settings.tokenizer
        );
    }
    let vocab_path = find_vocab(path, settings)?;
    let vocab = Vocab::load(&vocab_path)?;
    let session = build_session(path, device)?;
    let input_names: Vec<String> = session
        .inputs
        .iter()
        .map(|input| input.name.clone())
        .collect();
    let output_names: Vec<String> = session
        .outputs
        .iter()
        .map(|output| output.name.clone())
        .collect();
    let names = TensorNames::resolve(&input_names, &output_names, settings)?;
    Ok(Box::new(OnnxBackend {
        session,
        device,
        vocab,
        settings: settings.clone(),
        names,
    }))
}

fn build_session(path: &Path, device: DeviceKind) -> Result<Session> {
    let threads = std::thread::available_parallelism()
        .map(|value| value.get().clamp(1, 4))
        .unwrap_or(2);
    #[allow(unused_mut)]
    let mut builder = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads)?;

    match device {
        DeviceKind::Cpu => {}
        DeviceKind::Dml => {
            #[cfg(all(feature = "semantic-directml", target_os = "windows"))]
            {
                use ort::execution_providers::DirectMLExecutionProvider;
                builder =
                    builder.with_execution_providers([DirectMLExecutionProvider::default()
                        .build()
                        .error_on_failure()])?;
            }
            #[cfg(not(all(feature = "semantic-directml", target_os = "windows")))]
            {
                bail!(
                    "此构建不包含 DirectML 执行提供程序；Windows 上请使用 --features semantic-directml 构建"
                );
            }
        }
        DeviceKind::Coreml => {
            #[cfg(all(feature = "semantic-coreml", target_os = "macos"))]
            {
                use ort::execution_providers::CoreMLExecutionProvider;
                builder = builder.with_execution_providers([CoreMLExecutionProvider::default()
                    .build()
                    .error_on_failure()])?;
            }
            #[cfg(not(all(feature = "semantic-coreml", target_os = "macos")))]
            {
                bail!(
                    "此构建不包含 CoreML 执行提供程序；macOS 上请使用 --features semantic-coreml 构建"
                );
            }
        }
    }

    builder
        .commit_from_file(path)
        .with_context(|| format!("加载 ONNX 模型失败：{}", path.display()))
}

struct TensorNames {
    input_ids: String,
    attention_mask: String,
    token_type_ids: Option<String>,
    output: String,
}

impl TensorNames {
    fn resolve(
        actual_inputs: &[String],
        actual_outputs: &[String],
        settings: &BackendSettings,
    ) -> Result<Self> {
        if actual_outputs.is_empty() {
            bail!("ONNX 模型没有输出");
        }
        let defaults = [
            "input_ids".to_owned(),
            "attention_mask".to_owned(),
            "token_type_ids".to_owned(),
        ];
        let configured: Vec<String> = settings
            .input_names
            .iter()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .collect();
        let input_ids = configured
            .first()
            .cloned()
            .unwrap_or_else(|| defaults[0].clone());
        let attention_mask = configured
            .get(1)
            .cloned()
            .unwrap_or_else(|| defaults[1].clone());
        let actual: HashSet<&str> = actual_inputs.iter().map(String::as_str).collect();
        if !actual.contains(input_ids.as_str()) {
            bail!("ONNX 模型缺少输入 `{input_ids}`（实际输入：{actual_inputs:?}）");
        }
        if !actual.contains(attention_mask.as_str()) {
            bail!("ONNX 模型缺少输入 `{attention_mask}`（实际输入：{actual_inputs:?}）");
        }
        let configured_token_type = configured
            .get(2)
            .cloned()
            .unwrap_or_else(|| defaults[2].clone());
        let token_type_ids = if actual.contains(configured_token_type.as_str()) {
            Some(configured_token_type)
        } else {
            actual_inputs
                .iter()
                .find(|name| {
                    let lower = name.to_ascii_lowercase();
                    lower.contains("token_type") || lower == "segment_ids"
                })
                .cloned()
        };
        let known: HashSet<&str> = std::iter::once(input_ids.as_str())
            .chain(std::iter::once(attention_mask.as_str()))
            .chain(token_type_ids.as_deref())
            .collect();
        let unknown: Vec<&str> = actual_inputs
            .iter()
            .map(String::as_str)
            .filter(|name| !known.contains(name))
            .collect();
        if !unknown.is_empty() {
            bail!("ONNX 模型包含未支持的输入：{unknown:?}；请通过 semantic.input_names 配置");
        }

        let output = match &settings.output_name {
            Some(name) if !name.trim().is_empty() => {
                if !actual_outputs.iter().any(|actual| actual == name) {
                    bail!("ONNX 模型没有输出 `{name}`（实际输出：{actual_outputs:?}）");
                }
                name.clone()
            }
            _ => actual_outputs
                .iter()
                .find(|name| name.eq_ignore_ascii_case("logits"))
                .cloned()
                .unwrap_or_else(|| actual_outputs[0].clone()),
        };

        Ok(Self {
            input_ids,
            attention_mask,
            token_type_ids,
            output,
        })
    }
}

struct OnnxBackend {
    session: Session,
    device: DeviceKind,
    vocab: Vocab,
    settings: BackendSettings,
    names: TensorNames,
}

impl SemanticBackend for OnnxBackend {
    fn device(&self) -> DeviceKind {
        self.device
    }

    fn backend_name(&self) -> &'static str {
        "onnxruntime"
    }

    fn analyze(
        &mut self,
        document: &Document,
        token: &CancellationToken,
        cancellation_enabled: bool,
        _max_text_length: usize,
    ) -> Result<Vec<Issue>> {
        let max_chars = self.settings.max_text_length.clamp(16, 500);
        let mut issues = Vec::new();
        for range in chunk_ranges(&document.text, max_chars) {
            if token.cancelled(cancellation_enabled) {
                break;
            }
            let chunk = &document.text[range.clone()];
            if chunk.trim().is_empty() {
                continue;
            }
            let pieces = self.vocab.tokenize(chunk, range.start);
            if pieces.is_empty() {
                continue;
            }
            self.infer_chunk(document, &pieces, token, cancellation_enabled, &mut issues)?;
        }
        issues.sort_by_key(|issue| issue.offset);
        Ok(issues)
    }
}

impl OnnxBackend {
    fn infer_chunk(
        &mut self,
        document: &Document,
        pieces: &[Piece],
        token: &CancellationToken,
        cancellation_enabled: bool,
        issues: &mut Vec<Issue>,
    ) -> Result<()> {
        let pieces = if pieces.len() > MAX_SEQ_LEN - 2 {
            &pieces[..MAX_SEQ_LEN - 2]
        } else {
            pieces
        };
        let mut sequence: Vec<Piece> = Vec::with_capacity(pieces.len() + 2);
        sequence.push(Piece {
            id: self.vocab.cls_id,
            offset: None,
            character: None,
        });
        sequence.extend_from_slice(pieces);
        sequence.push(Piece {
            id: self.vocab.sep_id,
            offset: None,
            character: None,
        });
        let seq_len = sequence.len();
        let input_ids: Vec<i64> = sequence.iter().map(|piece| piece.id).collect();
        let attention_mask = vec![1i64; seq_len];
        let token_type_ids = vec![0i64; seq_len];

        let mut inputs: HashMap<&str, Tensor<i64>> = HashMap::new();
        inputs.insert(
            self.names.input_ids.as_str(),
            Tensor::from_array(([1usize, seq_len], input_ids))?,
        );
        inputs.insert(
            self.names.attention_mask.as_str(),
            Tensor::from_array(([1usize, seq_len], attention_mask))?,
        );
        if let Some(name) = &self.names.token_type_ids {
            inputs.insert(
                name.as_str(),
                Tensor::from_array(([1usize, seq_len], token_type_ids))?,
            );
        }

        let outputs = self.session.run(inputs)?;
        let output = outputs
            .get(self.names.output.as_str())
            .with_context(|| format!("模型输出缺少 `{}`", self.names.output))?;
        let logits = output
            .try_extract_array::<f32>()
            .context("模型输出不是 f32 logits")?;
        let shape = logits.shape();
        let (seq, vocab_size) = match shape {
            [1, seq, vocab] => (*seq, *vocab),
            [seq, vocab] => (*seq, *vocab),
            _ => bail!("模型输出形状应为 [1, seq, vocab]，实际为 {shape:?}"),
        };
        if seq != seq_len {
            bail!("模型输出序列长度 {seq} 与输入 {seq_len} 不一致");
        }
        if vocab_size != self.vocab.tokens.len() {
            bail!(
                "模型输出词表大小 {vocab_size} 与 vocab.txt 的 {} 不一致",
                self.vocab.tokens.len()
            );
        }
        let data = logits
            .as_slice()
            .context("模型输出不是连续内存布局，无法读取 logits")?;

        for (position, piece) in sequence.iter().enumerate() {
            if position % 64 == 0 && token.cancelled(cancellation_enabled) {
                break;
            }
            let (Some(offset), Some(original)) = (piece.offset, piece.character) else {
                continue;
            };
            let row = &data[position * vocab_size..(position + 1) * vocab_size];
            let (predicted, top_logit) = argmax(row);
            if predicted as i64 == piece.id {
                continue;
            }
            let Some(suggested) = self.vocab.tokens.get(predicted) else {
                continue;
            };
            if suggested.chars().count() != 1 || !is_cjk(suggested.chars().next().unwrap_or(' ')) {
                continue;
            }
            let confidence = softmax_confidence(row, predicted, top_logit);
            if confidence < self.settings.min_confidence {
                continue;
            }
            if !document
                .text
                .get(offset..)
                .map(|rest| rest.starts_with(original))
                .unwrap_or(false)
            {
                continue;
            }
            let length = original.len_utf8();
            if document.is_masked(offset, length) {
                continue;
            }
            issues.push(Issue {
                rule_id: "SEMANTIC_CSC".to_owned(),
                category: "semantic".to_owned(),
                severity: Severity::Warning,
                location: document.location(offset, true),
                file: None,
                original: original.to_string(),
                suggestion: suggested.clone(),
                message: format!("本地语义模型建议将「{original}」改为「{suggested}」"),
                source: Source::Model,
                confidence,
                offset,
            });
        }
        Ok(())
    }
}

fn argmax(row: &[f32]) -> (usize, f32) {
    let mut best_index = 0usize;
    let mut best = f32::NEG_INFINITY;
    for (index, value) in row.iter().enumerate() {
        if *value > best {
            best = *value;
            best_index = index;
        }
    }
    (best_index, best)
}

fn softmax_confidence(row: &[f32], predicted: usize, top: f32) -> f32 {
    let sum: f32 = row.iter().map(|value| (value - top).exp()).sum();
    if sum <= 0.0 || !sum.is_finite() {
        return 0.0;
    }
    let probability = (row[predicted] - top).exp() / sum;
    probability.clamp(0.0, 1.0)
}

fn find_vocab(model: &Path, settings: &BackendSettings) -> Result<PathBuf> {
    if let Some(path) = &settings.vocab_path {
        let path = crate::config::resolve_relative(path, &settings.source_dir)?;
        if !path.is_file() {
            bail!("semantic.vocab_path 不存在：{}", path.display());
        }
        return Ok(path);
    }
    let mut candidates = Vec::new();
    if let Some(parent) = model.parent() {
        candidates.push(parent.join("vocab.txt"));
    }
    let with_suffix = model.with_extension("vocab.txt");
    if !candidates.contains(&with_suffix) {
        candidates.push(with_suffix);
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("未找到 BERT 词表 vocab.txt；请放在模型同目录或设置 semantic.vocab_path")
}

#[derive(Debug, Clone)]
struct Piece {
    id: i64,
    /// Absolute byte offset in the full document for single-character tokens.
    offset: Option<usize>,
    character: Option<char>,
}

struct Vocab {
    ids: HashMap<String, i64>,
    tokens: Vec<String>,
    unk_id: i64,
    cls_id: i64,
    sep_id: i64,
}

impl Vocab {
    fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("无法读取词表 {}", path.display()))?;
        let tokens: Vec<String> = text
            .lines()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect();
        if tokens.is_empty() {
            bail!("词表为空：{}", path.display());
        }
        let ids: HashMap<String, i64> = tokens
            .iter()
            .enumerate()
            .map(|(index, token)| (token.clone(), index as i64))
            .collect();
        let required = |token: &str| -> Result<i64> {
            ids.get(token)
                .copied()
                .with_context(|| format!("词表缺少特殊 token `{token}`：{}", path.display()))
        };
        Ok(Self {
            unk_id: required("[UNK]")?,
            cls_id: required("[CLS]")?,
            sep_id: required("[SEP]")?,
            ids,
            tokens,
        })
    }

    fn tokenize(&self, chunk: &str, base_offset: usize) -> Vec<Piece> {
        let chars: Vec<(usize, char)> = chunk.char_indices().collect();
        let mut pieces = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            let (relative, character) = chars[index];
            if character.is_whitespace() {
                index += 1;
                continue;
            }
            if is_ascii_alphanumeric(character) {
                let start = index;
                while index < chars.len() && is_ascii_alphanumeric(chars[index].1) {
                    index += 1;
                }
                let word: String = chars[start..index].iter().map(|(_, c)| *c).collect();
                self.wordpiece(&word, base_offset + relative, &mut pieces);
                continue;
            }
            let token = character.to_string();
            let id = self.ids.get(&token).copied().unwrap_or(self.unk_id);
            let chinese = is_cjk(character);
            pieces.push(Piece {
                id,
                offset: chinese.then_some(base_offset + relative),
                character: chinese.then_some(character),
            });
            index += 1;
        }
        pieces
    }

    fn wordpiece(&self, word: &str, absolute_offset: usize, pieces: &mut Vec<Piece>) {
        let lower = word.to_ascii_lowercase();
        let mut start = 0usize;
        while start < lower.len() {
            let mut end = lower.len();
            let mut found = None;
            while start < end {
                let substring = &lower[start..end];
                let candidate = if start == 0 {
                    substring.to_owned()
                } else {
                    format!("##{substring}")
                };
                if let Some(id) = self.ids.get(&candidate).copied() {
                    found = Some((end, id));
                    break;
                }
                end -= 1;
            }
            match found {
                Some((end, id)) => {
                    pieces.push(Piece {
                        id,
                        offset: (start == 0).then_some(absolute_offset),
                        character: None,
                    });
                    start = end;
                }
                None => {
                    pieces.push(Piece {
                        id: self.unk_id,
                        offset: None,
                        character: None,
                    });
                    break;
                }
            }
        }
    }
}

/// Split text into chunks of roughly `max_chars` characters, preferring a
/// sentence boundary so the semantic model keeps local context.
fn chunk_ranges(text: &str, max_chars: usize) -> Vec<std::ops::Range<usize>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let tentative_end = (start + max_chars).min(chars.len());
        let mut end = tentative_end;
        let earliest_break = start + max_chars * 3 / 4;
        if tentative_end < chars.len() {
            for index in (earliest_break..tentative_end).rev() {
                if matches!(
                    chars[index].1,
                    '\n' | '。' | '！' | '？' | '；' | '!' | '?' | ';'
                ) {
                    end = index + 1;
                    break;
                }
            }
        }
        let byte_start = chars[start].0;
        let byte_end = if end < chars.len() {
            chars[end].0
        } else {
            text.len()
        };
        if byte_end > byte_start {
            ranges.push(byte_start..byte_end);
        }
        start = end.max(start + 1);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_prefers_sentence_boundary() {
        let text = "第一句话。第二句话。第三句话。";
        let ranges = chunk_ranges(text, 8);
        assert!(ranges.len() >= 2);
        for range in &ranges {
            assert!(range.end <= text.len());
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn softmax_confidence_is_reasonable() {
        let row = [0.0, 10.0, 0.0];
        let (index, top) = argmax(&row);
        assert_eq!(index, 1);
        assert!(softmax_confidence(&row, index, top) > 0.99);
    }
}
