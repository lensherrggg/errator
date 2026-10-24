# Changelog

所有重要变更记录在此文件。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Changed

- README 重写为完整使用手册：安装/验证、TUI 操作、全部 CLI 命令、配置示例、报告格式、语义设备、CI 集成、FAQ 与故障排查。

### Planned

- SentencePiece tokenizer support for semantic models.
- Native SDK crate extraction and compile-time rule registration.
- Template-based custom exporters.
- Ruleset asset bundling for installed rulesets.

## [0.5.1] - 2026-10-24

### Added

- CLI 摘要、Markdown/HTML 报告与 TUI 检查页/报告页/状态栏显示实际计算设备：`CPU（规则检查）`、`CPU · ONNX Runtime`、`GPU · DirectML`、`GPU · CoreML`，降级时标注“已降级”。
- `docs/design.md`：0.5.1 系统设计文档（架构、数据契约、语义/GPU 降级、TUI、扩展与取舍）。
- Windows 发布包现在附带 `docs/`，README 中的文档链接在离线包内可用。

### Changed

- 语义信息摘要不再重复设备字段，改为“语义状态 + 计算设备”两行展示；JSON schema 与设备字段不变。

## [0.5.0] - 2026-10-24

首个公开版本，对应《Errator 需求文档 v0.5》与《扩展 Errator》的 Windows 优先实现。

### Added

- CLI：`check`、`tui`、`rules list`、`rules test`、`ignore add/list/remove`、`ruleset add/list/remove`、`models download/list`、`init`、`version`、`devices`。
- 文档读取：`.txt` / `.md` / `.docx`；UTF-8、UTF-16 BOM；Markdown fenced code、行内代码、链接目标、URL、HTML 注释、front matter 屏蔽；docx 段落/字符级定位。
- 规则引擎：`CJK_SPACE_MISSING`、`CJK_SPACE_EXTRA`、`PUNCT_HALF_WIDTH`、`PUNCT_FULL_WIDTH`、`PUNCT_DUPLICATE`、`TYPO_CONFUSION`、`CHAR_DUPLICATE`、`TERM_VARIANT`、`CUSTOM_DICTIONARY`、自定义 pair/regex 规则、`SEMANTIC_CSC`。
- 配置：`[rules.severity_overrides]`（兼容 `[rules.severity]`）、`[rules.by_file_type.*]`、`[rules.by_path.*]`、`[dictionary]`（兼容 `[dict]`）、`[consistency]`、`[external.*]`、`[semantic]`；项目本地 `errator.toml` 自动发现。
- 扩展：L0 等级覆盖、L1 声明式规则与 `[[rules.custom.tests]]`、外部规则文件、规则集继承、L2 子进程 JSON 协议、Rust `Rule`/`Exporter` trait 与 `sdk` 重导出。
- 报告：终端、Markdown（PRD v0.5 规格）、自包含 HTML（XSS 转义、明暗色）、JSON schema `1.0`、CSV；批量目录报告可选 `file` 字段。
- 交互：文件浏览、后台检查、进度显示、问题筛选、详情、`i` 持久化忽略、`e` 四格式导出、帮助覆盖层。
- 检查控制：Ctrl+C / TUI 取消、部分报告、退出码 3；`--no-cancel`。
- 误报治理：持久化忽略库、CLI 临时规则忽略、白名单/保留词。
- 语义层：可选 ONNX Runtime 1.22 + BERT WordPiece；CPU / DirectML / CoreML 设备选择；初始化与推理失败自动 CPU 回退；真实设备与 `fallback_occurred` 入报告；GPU 路径继续使用同一 `Report` 结构。
- 工程：跨平台 CI、Windows 打包脚本、版本命令、Markdown golden/规则/CLI/docx/语义集成测试、格式与 clippy 门禁。

### Security

- 默认离线，模型下载必须显式 URL。
- HTML 报告转义全部动态文本，无外部资源。
- 外部检查器不继承 `PATH` 之外的环境变量；仅显式配置会执行子进程。
