# Changelog

所有重要变更记录在此文件。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added

- `docs/offline-migration.md` 与 `scripts/package-portable.ps1`：生成包含程序、DirectML、VC 运行库、模型和便携启动器的离线迁移包。

### Planned

- SentencePiece tokenizer support for semantic models.
- Native SDK crate extraction and compile-time rule registration.
- Template-based custom exporters.
- Ruleset asset bundling for installed rulesets.

## [0.7.2] - 2026-10-24

### Changed

- TUI 问题详情改为紧凑布局：合并类型/来源/置信度到标题与位置行，移除多余空行，上下文提前到第 6 行左右。
- 上下文窗口的“问题前文”由 160 字缩短为 96 字，总长度仍约 400 字，让高亮位置更容易在有限窗口高度内看到。

### Fixed

- 终端窗口高度较小时，详情顶部的元信息占用过多行，导致上下文区域被截断。

## [0.7.1] - 2026-10-24

### Fixed

- 当 `context` 与问题原文完全相同时（例如短标题段落），TUI 此前完全不显示上下文；现在始终展示并高亮原文。
- 同一段落后文出现重复词时，上下文高亮改为选择最接近问题 `char_offset/column` 的匹配项，不再固定高亮第一次出现的位置。

## [0.7.0] - 2026-10-24

### Added

- TUI 问题详情的上下文最长保留约 400 字（原 120 字），更适合查看段落级语境。
- TUI 上下文中用彩色底色高亮当前问题原文（error 红 / warning 黄 / info 青）。
- TUI 详情支持 `Ctrl+↑/↓` 与 `Ctrl+PgUp/PgDn` 滚动，长段落不再只能看到开头。

## [0.6.0] - 2026-10-24

### Added

- `errator ignore clear`：一次清空全部持久化忽略项，并显示删除数量。
- TUI 文件浏览页按 `/` 按关键字实时筛选当前目录；支持 Enter 应用、Esc 取消、Backspace 删除、Ctrl+U 清空、筛选结果内方向键移动。
- TUI 报告页按 `t` 按问题类型（category）筛选，可与 error/warning/info 级别筛选叠加；顶部显示当前类型与过滤后条数。
- README 补齐两个新功能的详细操作说明。

## [0.5.2] - 2026-10-24

### Fixed

- TUI 报告页头部的设备与计数原先与超长源路径挤在同一行，路径过长时会被裁掉；现在设备固定显示在头部第一行，源文件单独一行并截断。
- TUI 报告页底部高度不足，第二条状态/设备行不可见；现在固定显示“计算设备 + 状态/提示”。
- TUI 文件浏览页底部快捷键行被裁掉；布局已调整。

### Changed

- README 重写为完整使用手册：安装/验证、TUI 操作、全部 CLI 命令、配置示例、报告格式、语义设备、CI 集成、FAQ 与故障排查。

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
