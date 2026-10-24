# Errator

本地文本勘误报告生成器。读取 `.txt` / `.md` / `.docx`，运行格式、标点、空格、错别字、术语一致性与自定义规则检查；可选加载本地 ONNX 模型做上下文纠错。结果可在终端查看，也可导出 Markdown / HTML / JSON / CSV；无参数启动 TUI，全程默认离线。

当前版本：**0.5.0**（Windows 优先发布；源码面向 Windows / macOS / Linux）。

## 特性

- **两种形态**：脚本友好的 `errator check`，以及选文件、看详情、忽略误报、导出报告的交互式 TUI。
- **本地优先**：检查与报告渲染不发起网络请求；HTML 报告单文件自包含。
- **定位准确**：纯文本使用 1-based `line/column`；docx 使用 `paragraph/char_offset`，不伪造行号。
- **可扩展**：路径/文件类型分级、声明式词对与正则规则、外部检查器 JSON 协议、规则集、Rust `Rule` trait、`Exporter` trait。
- **可选语义层**：ONNX Runtime CPU 始终可用；Windows DirectML、macOS CoreML 为可选加速，初始化/推理失败自动回退 CPU，报告记录真实设备。
- **误报治理**：TUI `i` 持久化忽略；CLI `ignore add/list/remove`；每次运行可用 `--ignore` 临时过滤。
- **可取消**：`CancellationToken` + 流式进度；取消时输出部分报告并返回退出码 `3`。

## 快速开始

要求 Rust 1.88+（构建发布二进制）。默认构建包含规则引擎、CLI 和 TUI，不链接 ONNX 运行时：

```bash
cargo build --release
./target/release/errator check doc.md
```

Windows 完整版本（含 ONNX + DirectML）：

```powershell
cargo build --release --features semantic-directml
.\target\release\errator.exe check doc.md --semantic auto --device auto
```

macOS 可选 CoreML：

```bash
cargo build --release --features semantic-coreml
```

也可以直接使用仓库的 Windows 打包脚本：

```powershell
pwsh -File scripts/package-windows.ps1
```

产物位于 `dist/`。脚本会运行测试、构建 release、收集 DLL、生成 SHA-256，并打包 zip。

## 常用命令

```text
errator                              # TTY 中启动 TUI；非 TTY 输出帮助
errator tui [path]                   # 从文件或目录启动 TUI
errator check <path>                 # 检查文件或递归检查目录
errator check ./docs --format md -o report.md
errator check doc.txt --format json
errator check doc.txt --semantic on --device dml
errator rules list
errator rules test                   # 运行自定义规则的正例/反例
errator ignore add --rule CJK_SPACE_MISSING --text "使用Rust开发"
errator ignore list
errator ignore remove <id>
errator ruleset add ./rulesets/project.toml
errator ruleset list
errator check doc.md --ruleset project
errator models download --url <URL> --sha256 <HEX>
errator models list
errator devices
errator init
errator version
```

### `check` 主要参数

| 参数 | 说明 |
|---|---|
| `--format text\|json\|md\|html\|csv` | 报告格式，默认 `text` |
| `-o, --output <FILE>` | 写入文件；已存在时需 `--force` |
| `--config <FILE>` | 显式配置文件 |
| `--ruleset <NAME>` | 加载已安装规则集，可重复 |
| `--semantic auto\|on\|off` | 语义层模式；默认取配置，缺省 `auto` |
| `--device auto\|cpu\|dml\|coreml` | 推理设备；默认取配置，缺省 `auto` |
| `--no-gpu` | 等价于 `--device cpu` |
| `--min-severity error\|warning\|info` | 最低显示级别 |
| `--ignore <RULE[,RULE]>` | 本次忽略指定规则 ID |
| `--no-context` | 问题中不附带上下文 |
| `--no-color` / `--quiet` | 禁用颜色 / 只输出摘要（text） |
| `--no-cancel` | 忽略 Ctrl+C 取消 |
| `--force` | 覆盖输出文件 |
| `--no-tui` | 兼容参数；`check` 从不进入 TUI |

退出码：`0` 无 error；`1` 存在 error（且未被过滤）；`2` 运行错误；`3` 用户取消。

### TUI 快捷键

| 场景 | 按键 |
|---|---|
| 浏览 | `↑/↓`/`j/k` 选择，`Enter` 打开/检查，`Backspace` 上级，`r` 刷新 |
| 报告 | `↑/↓` 切换问题，`f`/`0-3` 筛选，`i` 忽略并持久化，`e` 导出，`Esc` 返回 |
| 导出 | `m` Markdown，`h` HTML，`j` JSON，`c` CSV |
| 通用 | `?` 帮助，`q` 退出，`Ctrl+C` 取消/退出 |

TUI 导出的文件写在源文件同目录，名称形如 `doc.errator-20261024-204500.md`，不会覆盖既有报告。

## 配置

按以下顺序发现配置：

1. `--config <FILE>`
2. 当前目录的 `errator.toml`（项目本地配置）
3. `~/.errator/config.toml`

运行 `errator init` 会在 `~/.errator/` 下写入带注释的默认配置、忽略库、模型目录和规则集目录。完整字段见 [docs/configuration.md](docs/configuration.md)。

```toml
[rules.severity_overrides]
CJK_SPACE_MISSING = "auto"

[rules.by_file_type.docx]
CJK_SPACE_MISSING = "info"

[rules.by_path."docs/legacy/**"]
PUNCT_HALF_WIDTH = "off"

[[rules.custom]]
id = "CUSTOM_LOGIN"
type = "pair"
category = "typo"
severity = "warning"
wrong = "登陆"
right = "登录"
when_prev = ["账号", "系统"]

[[dictionary.confusions]]
wrong = "勘误师"
right = "勘误执行者"
severity = "info"

[external.terminology]
command = "python checkers/terminology.py"
timeout_ms = 5000
```

等级优先级：`by_path` > `by_file_type` > `severity_overrides` > 内置默认。可用值为 `error` / `warning` / `info` / `off` / `auto`。

## 可选语义模型

语义层完全可选，规则检查不需要模型。

1. 将 BERT WordPiece 格式的 ONNX 纠错模型放到 `~/.errator/models/`，或显式下载：
   ```bash
   errator models download --url "https://example.com/macbert4csc.onnx" --name macbert4csc.onnx
   ```
   核心工具不会隐式联网；必须显式提供 URL（或设置 `ERRATOR_MODEL_URL`）。
2. 将对应的 `vocab.txt` 放在模型同目录，或设置 `semantic.vocab_path`。
3. `errator check doc.md --semantic on --device auto`。

模型约定与降级细节见 [docs/semantic.md](docs/semantic.md)。Windows 构建固定 ONNX Runtime 1.22（DirectML 在 1.25+ 的打包方式已变化），`DirectML.dll` 随发布包分发。DirectML/CoreML 不可用或推理失败时自动回退 CPU；`semantic.device` / `fallback_occurred` 如实记录实际设备。

## 扩展

按改造成本从低到高：

| 层级 | 方式 | 入口 |
|---|---|---|
| L0 | 启停规则、覆盖等级、按路径/类型分级 | `errator.toml` |
| L1 | 词对规则、正则规则、上下文条件、词典、规则集 | `[[rules.custom]]`、`[dictionary]`、`ruleset` |
| L2 | 任意语言外部检查器（stdin/stdout JSON） | `[external.<name>]`、[协议](docs/external-checkers.md) |
| L3 | Rust 原生规则/导出器 | `errator::rules::Rule`、`errator::report::Exporter`、`errator::sdk` |

内置规则表与新增规则流程见 [docs/rules.md](docs/rules.md)；模块边界与扩展点见 [docs/architecture.md](docs/architecture.md)；规则集打包见 [docs/rulesets.md](docs/rulesets.md)。

## 跨平台

| 平台 | 构建 | DirectML/CoreML | 说明 |
|---|---|---|---|
| Windows x64 | `cargo build --release --features semantic-directml` | DirectML（可选） | 当前首要发布目标；发布包带 `DirectML.dll` |
| macOS | `cargo build --release --features semantic-coreml` | CoreML（可选） | 动态形状/INT8 支持有限，失败回退 CPU |
| Linux | `cargo build --release --features semantic` | 仅 CPU | 规则、CLI、TUI 完整可用 |

核心代码不假设固定的路径分隔符或终端编码；配置中的路径统一支持 `/` 与 `\`，报告中的 Windows 路径规范为 `/`。

## 开发与测试

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --features semantic          # 额外跑 CPU ONNX 集成测试
cargo test --features semantic-directml # Windows DirectML 可用性测试
```

测试覆盖：规则正反例、Markdown golden、docx 定位、HTML 转义、CSV 引号、目录聚合、忽略持久化、规则集、外部检查器、取消、TUI 渲染、语义模型与降级。

## 版本管理

- 语义化版本 `MAJOR.MINOR.PATCH`，唯一来源是 `Cargo.toml`；`build.rs` 额外嵌入 `git describe` 信息，`errator version --json` 可读取。
- 报告数据契约独立版本化：当前 `schema_version = "1.0"`，GPU/新增导出不改变 `issues` 结构。
- 发布以 annotated Git tag（`v0.5.0`）标记，变更记录在 [CHANGELOG.md](CHANGELOG.md)；流程见 [docs/versioning.md](docs/versioning.md)。
- 规则 ID 一经发布不改名；废弃时保留 ID 并标记。

## 隐私与安全

- 默认零网络；只有显式 `models download` 与用户配置的 `[external.*]` 会启动额外网络/进程行为。
- 外部检查器只继承 `PATH`（Windows 另加 `SystemRoot`/`PATHEXT`/`COMSPEC`）和显式 `env`，并设置确定性的 UTF-8 变量；不会继承其他环境变量。
- TUI 与 CLI 都不修改源文件。
- HTML 渲染对所有报告文本做转义，无脚本、无外部资源。

## 已知限制

- 语义层 v0.5 仅支持 BERT WordPiece 词表（MacBERT / bert-base-chinese）；SentencePiece 在路线图中。
- 单次 ONNX 推理无法在算子内部抢占；取消在一次推理结束后生效，长序列 GPU 的 2 秒指标取决于模型与硬件。
- `errator models download` 不内置模型地址，需用户提供 URL 与可选校验和。
- 通过 `ruleset add` 安装的规则集建议自包含；相对词典路径在安装后仍相对于规则集所在目录。
- TUI 不编辑源文件；批量目录报告在 issue 上增加可选 `file` 字段，单文件报告保持 schema 1.0 原样。

## 许可证

MIT，见 [LICENSE](LICENSE)。
