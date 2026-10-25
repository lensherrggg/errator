# 可选语义层

语义层是可选增强：规则检查、报告导出、TUI 与忽略闭环都不需要模型。模型不可用、GPU 不可用或加速失败时，Errator 不阻塞检查，只记录提示并按策略回退。

## 构建

| 平台 | 命令 | 后端 |
|---|---|---|
| Windows | `cargo build --release --features semantic-directml` | CPU + DirectML |
| macOS | `cargo build --release --features semantic-coreml` | CPU + CoreML |
| Linux | `cargo build --release --features semantic` | CPU |
| 任意平台，仅规则 | `cargo build --release` | 无 ONNX 依赖 |

ONNX Runtime 固定为 **1.22**（`ort = "=2.0.0-rc.10"`）。原因是 DirectML execution provider 在 ONNX Runtime 1.25+ 的官方打包中发生变化；该版本组合已在 Windows DirectML 上实测。

## 模型准备

1. 准备 BERT WordPiece 词表的字符级纠错 ONNX 模型（首选 MacBERT4CSC 量化版）。
2. 优先放置到 exe 同目录的 `models\`：
   ```text
   <errator.exe 所在目录>\
     errator.exe
     DirectML.dll
     models\
       macbert4csc.onnx
       vocab.txt
   ```
   也支持 `~/.errator/models/`；发现顺序为 `semantic.model_path` → `<exe>\models` → `<exe>` → `~/.errator/models`。
   或显式配置：
   ```toml
   [semantic]
   model_path = "D:/models/macbert4csc.onnx"
   vocab_path = "D:/models/vocab.txt"
   tokenizer = "bert-base-chinese"
   ```
   路径相对于配置文件解析，支持 `~`。
3. 运行：
   ```bash
   errator check doc.md --semantic on --device auto
   ```

也可显式下载（核心工具不会隐式联网；默认下载到 `~/.errator/models/`，可再复制到 `<exe>\models\`）：

```bash
errator models download \
  --url "https://example.com/macbert4csc.onnx" \
  --name macbert4csc.onnx \
  --sha256 "<可选 SHA-256>"
```

`errator models list` 查看模型目录；`errator devices` 查看构建与本地模型状态。

## 模型接口约定

- 输入：`input_ids`、`attention_mask`，可选 `token_type_ids`，dtype 为 `int64`，形状 `[1, seq]`。
- 输入名可通过 `semantic.input_names` 覆盖，顺序为 ids、mask、token_type。
- 输出：`logits`（或 `semantic.output_name`），形状 `[1, seq, vocab]` 或 `[seq, vocab]`，dtype `f32`。
- `vocab` 必须与 `vocab.txt` 行数一致。
- 每个中文单字 token 取 argmax；预测为不同汉字且 softmax 置信度 ≥ `min_confidence` 时生成 `SEMANTIC_CSC`。
- 文本按标点/换行切块，每块最多 `max_text_length` 字符（内部上限 500），避免超长序列。

## 设备与降级

| 配置 | Windows | macOS | 其他 |
|---|---|---|---|
| `auto` | DirectML → CPU | CoreML → CPU | CPU |
| `cpu` | CPU | CPU | CPU |
| `dml` | DirectML → CPU（失败时） | 平台不支持 → CPU | 平台不支持 → CPU |
| `coreml` | 平台不支持 → CPU | CoreML → CPU（失败时） | 平台不支持 → CPU |

- `--no-gpu` 等价于 `--device cpu`。
- `auto` 在 GPU 缺失时静默使用 CPU；显式指定 GPU 失败时输出警告并回退。
- `allow_gpu_fallback = false` 时，GPU 不可用会禁用可选语义层，但规则检查继续。
- 初始化失败与推理失败都会尝试降级；`semantic.fallback_occurred` 记录是否发生回退，`semantic.device` 记录实际使用设备。
- CLI 文本摘要、Markdown/HTML 报告与 TUI 会直接显示 **计算设备**（CPU 规则检查 / CPU ONNX / GPU DirectML / GPU CoreML），不用只靠 JSON 字段判断。
- 报告结构与 `schema_version` 不因设备变化而变化。

## 已验证模型示例

以下公开模型已在 Windows + DirectML 环境完成 CPU/DirectML 实测，可作为起点：

- 来源：[`Xenova/macbert4csc-base-chinese`](https://huggingface.co/Xenova/macbert4csc-base-chinese)（上游：[`shibing624/macbert4csc-base-chinese`](https://huggingface.co/shibing624/macbert4csc-base-chinese)）
- ONNX 文件：`onnx/model_quantized.onnx`（INT8 量化，103,596,411 bytes）
- SHA-256：`0c40a55ed4cae6df2fb32c666fb21c75bcea3543cd2dac653e0b4aff22e1e8c7`
- 词表：`vocab.txt`（109,540 bytes）
- SHA-256：`45bbac6b341c319adc98a532532882e91a9cefc0329aa57bac9ae761c27b291c`
- 本地命名示例：`macbert4csc.onnx` + `vocab.txt`

实测结果：CPU `semantic.device=cpu`，DirectML `semantic.device=dml`，二者均能输出 `SEMANTIC_CSC`；DirectML 初始化失败时回退 CPU。该模型由第三方分发，许可证与可用性以模型上游为准；Errator 发布包不包含权重。

## 已知限制

- **DirectML**：仅 Windows + DirectX 12；模型算子覆盖与静态/动态形状取决于运行时。
- **CoreML**：动态形状与 INT8 量化支持有限；推荐 FP16 或非量化模型，失败自动 CPU。本项目未在 Apple 硬件上做发布级实测。
- **分词器**：当前版本只支持 BERT WordPiece；SentencePiece 在路线图中。
- **取消**：ORT 单次 `session.run` 内部无法中断，Errator 在块之间轮询取消。长序列或很慢的 GPU 上，从按键到停止可能超过 2 秒。
- **模型质量**：模型会误报；用 `i` 或 `ignore` 闭环治理，不要把它当最终裁决。
- 不保证召回所有上下文错别字。

## 故障排查

```bash
errator devices
errator check doc.md --semantic on --device cpu
```

- 若显示“未启用 ONNX 后端”，当前二进制没带 `semantic` feature。
- 若提示缺 `vocab.txt`，把词表放到模型同目录或设置 `semantic.vocab_path`。
- 若报告 `fallback_occurred=true`，查看 stderr 的初始化/推理错误；确定所需 EP 是否被构建进二进制。
- 若 `semantic.device=dml` 但结果异常慢，GPU 可能只接管了部分节点，其余回退到 CPU。
