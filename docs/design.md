# Errator 设计文档

| 项目 | 内容 |
|---|---|
| 对应版本 | 0.7.2 |
| 状态 | 已实现；Windows 首发，macOS/Linux 源码可构建 |
| 对应发布标签 | `v0.7.2` |
| 文档范围 | 核心架构、数据契约、关键流程、扩展机制、语义后端、TUI、质量与取舍 |
| 需求来源 | 本地需求文档与扩展文档（未纳入 Git） |

本文描述 Errator 0.7.2 的实际设计，而不是待实现构想。配置字段、规则、协议、命令等使用细节分别见 [configuration.md](configuration.md)、[rules.md](rules.md)、[external-checkers.md](external-checkers.md)、[semantic.md](semantic.md)。

---

## 1. 摘要

Errator 是一个离线优先的本地文本勘误报告生成器。它把文档提取、规则检查、可选语义检查、误报过滤和报告导出编排成一条稳定流水线，并提供两种入口：

- **CLI**：适合脚本、CI 与批量目录检查；
- **TUI**：适合人工浏览、逐条查看、忽略误报和导出报告。

核心设计原则：

1. 文档与规则检查不依赖网络；
2. 确定性的规则/词典层始终可用，语义层与 GPU 只是可选增强；
3. 报告是唯一稳定输出契约，任何 UI 或推理后端都不得改变它；
4. 通过配置、外部进程、规则集和 Rust trait 支持渐进式扩展；
5. 用单一 Rust 二进制降低分发与运行成本。

---

## 2. 目标与约束

### 2.1 设计目标

| 目标 | 设计响应 |
|---|---|
| 本地、默认离线 | 无检查期网络请求；HTML 自包含；模型下载仅显式触发 |
| 不绑定 Word | 直接读取 `.docx` ZIP/XML，不依赖 Office 或在线服务 |
| 位置可追溯 | 文本使用 line/column；docx 使用 paragraph/char_offset |
| CPU 永远可用 | 语义层可选，规则层无模型依赖；GPU 失败自动回退 |
| 设备透明 | CLI、Markdown/HTML、TUI 明确显示 CPU/GPU 与降级状态 |
| 人读与机读并重 | text/Markdown/HTML 与 JSON/CSV 共用同一 `Report` |
| 可扩展 | 配置分级、声明式规则、外部检查器、规则集、trait 扩展 |
| 操作低摩擦 | 无参数启动 TUI；文件浏览 → 检查 → 详情 → 导出闭环 |
| 可治理误报 | TUI `i`、CLI `ignore`、白名单、规则过滤 |
| 跨平台 | 路径规范化、终端抽象、平台特性编译期隔离 |

### 2.2 非目标

- 不做 Word 实时插件；
- 不做在线 API / SaaS；
- 不做全文改写、润色或降重；
- TUI 不编辑源文件；
- 不保证 100% 召回语义错误；
- 不把 GPU 作为运行前提；
- 当前版本不实现动态库插件、WASM 沙箱与 SentencePiece 分词。

### 2.3 约束

- 实现语言：Rust（edition 2024，MSRV 1.88）；
- 语义运行时：ONNX Runtime 1.22（`ort 2.0.0-rc.10`）；
- Windows DirectML 需要 DirectX 12 GPU，发布包需带 `DirectML.dll`；
- CoreML 动态形状/INT8 能力受模型与运行时限制；
- 单次 ONNX Runtime `Run` 不可在算子内部抢占；
- 报告 schema 版本独立于应用版本。

---

## 3. 质量属性与验收映射

| 质量属性 | 设计手段 | 验证 |
|---|---|---|
| 性能：10 万字规则检查 < 3s | 单次提取、线性扫描、release 优化 | 本机 10.8 万字符实测约 1.24s |
| 取消响应 | `CancellationToken` 在多处轮询 | 单元测试覆盖预取消；TUI 取消路径 |
| GPU 降级不阻塞 | 设备候选队列 + CPU 兜底 | toy ONNX CPU/DirectML 集成测试 |
| 报告结构稳定 | `Report`/`Issue` 固定字段 + schema 版本 | JSON 结构测试 + GPU 语义测试 |
| XSS 安全 | HTML 全量转义、无外部资源 | HTML 转义单元测试 |
| 跨平台 | CI 三平台；平台特性条件编译 | GitHub Actions + Windows 实机 |
| 可扩展 | 配置/规则/协议/trait 四层扩展 | 集成测试覆盖规则集与外部检查器 |
| 可维护 | 模块单向依赖、单一版本源、CHANGELOG | fmt/clippy/test 门禁 |

---

## 4. 系统上下文

```text
              ┌──────────────────────────────┐
              │            用户              │
              └───────────┬──────────────────┘
                          │
             ┌────────────┴─────────────┐
             │                          │
      ┌──────▼──────┐            ┌──────▼──────┐
      │    CLI      │            │     TUI     │
      └──────┬──────┘            └──────┬──────┘
             │                          │
             └────────────┬─────────────┘
                          │  Engine / Report
                 ┌────────▼─────────┐
                 │   检查编排引擎    │
                 └──┬─────┬─────┬───┘
                    │     │     │
        ┌───────────┘     │     └────────────┐
        │                 │                  │
┌───────▼───────┐ ┌───────▼────────┐ ┌───────▼────────┐
│ 文档提取+定位  │ │ 规则/词典/扩展 │ │ 可选语义后端   │
│ txt/md/docx   │ │ L0-L2、规则集  │ │ CPU/DML/CoreML │
└───────────────┘ └───────┬────────┘ └───────┬────────┘
                          │                  │
                          └────────┬─────────┘
                                   │
                          ┌────────▼────────┐
                          │ 统一 Report 模型 │
                          └────────┬────────┘
                                   │
                ┌──────────────────┼──────────────────┐
                │                  │                  │
          ┌─────▼─────┐     ┌──────▼──────┐    ┌──────▼──────┐
          │ 终端渲染   │     │ 文件导出     │    │ TUI 展示/导出│
          └───────────┘     └─────────────┘    └─────────────┘
```

外部检查器是唯一可执行第三方代码的扩展点：

```text
Engine ──JSON stdin──▶ 用户显式配置的子进程 ──JSON stdout──▶ Issue[]
```

---

## 5. 总体架构

### 5.1 分层

```text
┌─────────────────────────────────────────────────────┐
│ 表现层：cli / tui / report                          │
├─────────────────────────────────────────────────────┤
│ 编排层：engine（文件遍历、规则调度、过滤、聚合）     │
├─────────────────────────────────────────────────────┤
│ 能力层：rules / external / semantic / ignore        │
├─────────────────────────────────────────────────────┤
│ 基础层：document / config / model / paths / cancel  │
└─────────────────────────────────────────────────────┘
```

依赖方向保持单向：

- `model` 不依赖 CLI、TUI、规则实现；
- `document` 不依赖规则或 UI；
- 规则只接收 `RuleContext`，不接触文件系统、终端或引擎内部状态；
- `engine` 是唯一同时知道文档、规则、外部检查器、语义层与忽略库的模块；
- CLI/TUI 只负责输入采集、调用引擎、呈现与导出。

### 5.2 代码结构

```text
src/
  main.rs            进程入口
  lib.rs             模块导出 + sdk 重导出
  cli.rs             参数、命令、退出码、信号
  config.rs          配置发现、L0/L1 设置、词典文件
  document.rs        文档提取、位置映射、屏蔽区
  engine.rs          检查编排、去重/过滤/排序、报告聚合
  cancel.rs          取消令牌
  external.rs        子进程 JSON 协议
  ignore.rs          持久化忽略库
  model.rs           Issue/Location/Report
  models.rs          模型下载
  rules/             规则实现、注册表、测试运行器
  ruleset.rs         规则集安装、加载、合并
  semantic.rs        设备规划、降级、后端 trait
  semantic/onnx.rs   ONNX Runtime + BERT WordPiece
  report/            Exporter trait 与五种格式
  tui.rs             TUI 状态机与渲染
  version.rs         版本信息
```

完整目录见第 22 节。

---

## 6. 模块设计

### 6.1 CLI

CLI 使用 `clap` derive 定义命令树。`main` 只调用 `errator::run()`；`cli::run()` 返回退出码，便于测试与嵌入式复用。

处理顺序：

1. 安装一次 Ctrl+C 处理器，写入全局取消令牌；
2. 解析参数；
3. 根据命令加载配置/规则集；
4. 调用 Engine 或管理命令；
5. 将错误映射为退出码 2。

`check` 的 stdout 只输出报告；提示、进度、写入确认全部走 stderr，保证 `--format json | jq` 等管道行为稳定。终端摘要与 Markdown/HTML 报告都在元信息中直接显示实际计算设备（例如 `CPU（规则检查）` 或 `GPU · DirectML`），不仅记录在 JSON 的 `semantic.device` 中。

### 6.2 文档提取与定位

支持 `.txt`、`.md`、`.docx`，扩展名大小写不敏感。

**文本读取**

- UTF-8 直接解码；
- UTF-8 BOM 去除；
- UTF-16 LE/BE BOM 解码；
- 其他编码报错并提示另存为 UTF-8。

**Markdown 预处理**

构建一次屏蔽区（byte ranges）：

- YAML front matter；
- fenced code block（含未闭合块）；
- 行内代码；
- Markdown 链接目标；
- URL；
- HTML 注释。

规则通过 `Document::is_masked(start, len)` 跳过屏蔽区。自定义规则可选择 `include_excluded = true` 绕过。

**docx**

- 用 `zip` 读取 `word/document.xml`；
- 用 `roxmltree` 解析；
- 遍历 `w:p`，收集 `w:t`、`w:tab`、`w:br`、`w:cr`、`w:noBreakHyphen`；
- 段落之间加入换行，记录每个段落的字节 span；
- 不生成伪造 line/column。

**位置模型**

```rust
pub enum Location {
    Text { line: usize, column: usize, context: Option<String> },
    Docx { paragraph: usize, char_offset: usize, context: Option<String> },
}
```

- 所有序号从 1 开始；
- column/char_offset 按 Unicode scalar 计数；
- 每个内部 issue 保存不序列化的字节 `offset`，用于稳定排序与 TUI 导航；
- 上下文按行/段落截取，超长时截断并加省略号。

### 6.3 规则系统

核心 trait：

```rust
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn category(&self) -> &str;
    fn default_severity(&self) -> Severity;
    fn description(&self) -> &str;
    fn check(&self, context: &RuleContext<'_>) -> Vec<Issue>;
}
```

规则只返回原始 issue；**级别解析、忽略、白名单、去重、排序统一由 Engine 完成**，避免各规则重复实现策略。

内置规则：

| 规则 | 作用 |
|---|---|
| `CJK_SPACE_MISSING` | 中英文/数字间空格 |
| `CJK_SPACE_EXTRA` | 汉字间多余空格 |
| `PUNCT_HALF_WIDTH` | 中文语境半角标点 |
| `PUNCT_FULL_WIDTH` | 英文语境全角标点 |
| `PUNCT_DUPLICATE` | 连续重复标点 |
| `TYPO_CONFUSION` | 内置错别字词典 |
| `CHAR_DUPLICATE` | 高风险汉字重复 |
| `TERM_VARIANT` | 全文术语混用 |
| `CUSTOM_DICTIONARY` | 用户词典 |
| `CUSTOM_*` | pair/regex 声明式规则 |
| `SEMANTIC_CSC` | 可选语义模型 |

#### 空格规则算法

1. 按行扫描，忽略屏蔽区；
2. 找出由「汉字 / ASCII 字母数字」组成的连续 token；
3. token 同时含两种脚本才报告；
4. 在脚本切换处插入空格，生成 suggestion；
5. 同一 token 合并为一个 issue，避免每个边界刷屏。

#### 标点规则算法

- 用相邻字符判断语境（汉字、中文标点、ASCII 字母数字）；
- 排除数字内部、英文缩写、URL、代码；
- `.` 仅在中文语境句末/句中建议全角；
- 全角转半角只在两侧均为英文/数字时报告；
- 重复标点单独规则，排除合法 `……`、`——`。

#### 错别字与一致性

- 错别字使用静态词对与 `str::match_indices`，尊重屏蔽区；
- 一致性规则先统计各变体，只有同文档出现至少两个变体才报告；
- 推荐写法为组内第一项；全篇只用非推荐写法不报告。

### 6.4 检查编排

`Engine::run` 的主要流程：

```text
collect_inputs
  └─ prepare semantic runtime
      └─ for each file
          ├─ Document::load
          ├─ for each rule
          │   ├─ effective_level(rule, kind, path)
          │   ├─ rule.check
          │   └─ normalize/filter issues
          ├─ external checkers
          ├─ semantic inference
          └─ progress event
  └─ sort + build Report
```

Issue 统一后处理顺序：

1. 应用路径/类型/全局级别；
2. 批量目录添加可选 `file` 字段；
3. `--no-context` 时移除上下文；
4. 最低严重级别过滤；
5. `--ignore` 规则 ID 过滤；
6. 持久化忽略库匹配（`rule_id + original`）；
7. 白名单匹配（原文精确比较）；
8. 去重（fingerprint：文件、规则、位置、原文、建议）；
9. 稳定排序：文件顺序 → 字节 offset → 规则 ID；
10. 生成摘要与计数。

取消时：

- 停止启动新的文件/规则；
- 保留已产生 issue；
- 输出 `cancelled=true` 的部分报告；
- CLI 退出码 3。

### 6.5 配置系统

发现顺序：

1. `--config`；
2. 当前目录 `errator.toml`；
3. `~/.errator/config.toml`。

`ERRATOR_HOME` 覆盖数据目录（配置、忽略、模型、规则集）。

配置模型中的 `source_dir` 不序列化，用于解析相对路径：

- 外部词典文件；
- 外部检查器 `cwd`；
- 语义模型/词表。

规则级别优先级：

```text
by_path > by_file_type > severity_overrides > 规则内置默认
```

值为 `auto` 表示停止继续向下查找，采用规则自身默认（例如 docx 的空格规则为 info）。`off` 直接跳过。

### 6.6 扩展机制

按用户成本分四层：

| 层级 | 扩展方式 | 运行方式 |
|---|---|---|
| L0 | 配置启停/分级 | 内置 |
| L1 | pair/regex 规则、词典、规则集 | TOML 解释执行 |
| L2 | 外部检查器 | 子进程 JSON |
| L3 | Rust `Rule` / `Exporter` | 编译期 |

规则集：

- 安装到 `~/.errator/rulesets/<name>.toml`；
- `extends` 递归加载，检测循环；
- 后加载规则覆盖同 ID 规则并告警；
- 支持 `--ruleset <name>` 或直接路径。

L2 外部检查器：

- 不做 shell 展开，按引号规则拆分 argv；
- 每文档一次，stdin 写 JSON，stdout 读 JSON，stderr 仅日志；
- 超时/退出非 0/非法 JSON 只产生警告，不阻塞内置流程；
- 请求中非 ASCII 字符转义为 `\uXXXX`，规避 Windows 旧代码页读取问题；
- 环境只传 `PATH` 与显式 `env`，Windows 额外保留系统必要变量，并设置 Python UTF-8 变量；
- 取消/超时会 kill 子进程。

### 6.7 报告与导出

稳定模型：

```rust
pub struct Report {
    pub schema_version: String,
    pub tool: ToolInfo,
    pub generated_at: String,
    pub source: String,
    pub cancelled: bool,
    pub semantic: SemanticInfo,
    pub summary: Summary,
    pub issues: Vec<Issue>,
    // 不序列化：semantic_warning
}
```

`Issue` 字段固定为：

```text
rule_id, category, severity, location, original,
suggestion, message, source, confidence
```

批量目录报告额外使用可选 `file` 字段；单文件报告保持 schema 1.0 原样。内部 `offset` 永不序列化。

`Exporter` trait：

```rust
pub trait Exporter {
    fn format(&self) -> &str;
    fn export(&self, report: &Report, out: &mut dyn Write) -> Result<()>;
}
```

内置实现：text、JSON、Markdown、HTML、CSV。

- **Markdown**：固定一级标题、元信息（含计算设备）、摘要表、问题代码块；无问题时明确输出“未发现问题”。
- **HTML**：单文件内联 CSS，无外部请求，所有动态文本转义，支持明暗色，元信息含计算设备。
- **JSON**：稳定字段 + 缩进输出。
- **CSV**：固定表头、RFC 4180 风格转义、CRLF。
- 文件写入使用同目录临时文件 + rename；默认拒绝覆盖，需 `--force`。

### 6.8 TUI

TUI 是独立表现层，不包含规则逻辑。状态机：

```text
Browser ──Enter(file)──▶ Checking ──Done──▶ Report
   ▲                         │                 │
   │                         └─Cancel──────────┘
   └──────────────Esc──────────────────────────┘
```

- 浏览器列目录/is dir/支持文件，支持上级、刷新、Home/End；
- 浏览器支持 `/` 关键字实时筛选当前目录，Esc 取消编辑/清除筛选，进入目录或刷新会重置筛选；
- 检查在 worker 线程执行，主线程通过 `std::sync::mpsc` 接收进度；
- 取消令牌与 worker 共享；
- 报告页支持方向键、`f`/`0-3` 级别筛选、`t` 问题类型筛选、`i` 忽略、`e` 四格式导出；级别与类型可叠加；
- 导出写到源文件同目录，文件名带时间戳，不覆盖；
- 检查页显示当前计算设备；报告页头部第一行固定显示设备与问题计数，源文件独立一行并按终端宽度截断；底部固定显示设备与最新状态，避免超长路径或状态覆盖设备信息；
- 上下文保留约 400 字窗口（问题前约 96 字）；TUI 详情采用紧凑布局，元信息合并为标题/位置/原文/建议/说明，上下文前移到第 6 行左右；高亮问题原文（即使上下文与原文相同也展示，并在重复出现时选择最接近问题位置的一处），并支持 `Ctrl+↑/↓` / `Ctrl+PgUp/PgDn` 滚动长段落；
- 启动/退出严格恢复 raw mode、alternate screen、光标；
- 非 TTY 执行 `tui` 会明确报错，不尝试启动。

UI 渲染使用 `ratatui`；终端缓冲区测试覆盖浏览器、报告、CJK 宽度截断和 Markdown 导出。

---

## 7. 数据契约（JSON）

```json
{
  "schema_version": "1.0",
  "tool": { "name": "errator", "version": "0.7.2" },
  "generated_at": "2026-10-24T20:56:24+08:00",
  "source": "docs/example.md",
  "cancelled": false,
  "semantic": {
    "enabled": true,
    "device": "cpu",
    "backend": "onnxruntime",
    "fallback_occurred": false
  },
  "summary": {
    "total": 1,
    "by_severity": { "error": 0, "warning": 1, "info": 0 }
  },
  "issues": [
    {
      "rule_id": "CJK_SPACE_MISSING",
      "category": "spacing",
      "severity": "warning",
      "location": {
        "kind": "text",
        "line": 1,
        "column": 1,
        "context": "使用Rust开发"
      },
      "file": "docs/example.md",
      "original": "使用Rust开发",
      "suggestion": "使用 Rust 开发",
      "message": "中英文/数字之间建议加显式空格",
      "source": "rule",
      "confidence": 1.0
    }
  ]
}
```

不变式：

- `summary.total == issues.len()`；
- `summary.by_severity` 与 issues 一致；
- `source` ∈ {`rule`, `dictionary`, `model`}；
- `location.kind` ∈ {`text`, `docx`}；
- `file` 仅在批量目录检查出现；
- GPU/CPU 切换不改变 issues 结构；
- 破坏性字段变更必须提升 `schema_version`。

---

## 8. 关键流程

### 8.1 单文件检查

```text
用户命令
  → 解析配置/规则集
  → 收集文件
  → 规划语义设备（可选）
  → 读取文档、构建位置与屏蔽区
  → 规则逐条执行
  → 外部检查器
  → 语义推理（可选）
  → 过滤/去重/排序
  → 构建 Report
  → 渲染/写文件
  → 退出码
```

### 8.2 严重级别解析

```text
for rule:
  1. 查找匹配的 by_path 覆盖（BTreeMap 逆序，确定性优先于配置书写顺序）
  2. 查找匹配的 by_file_type 覆盖
  3. 查找 severity_overrides
  4. 若有覆盖：off 跳过；auto 用规则默认；否则用覆盖值
  5. 否则用规则默认
```

### 8.3 TUI 检查生命周期

1. 用户选中文件，构造 `CheckOptions` 与取消令牌；
2. 启动 worker 线程；
3. worker 创建 Engine，按正常 CLI 流程检查；
4. 进度事件通过 mpsc 发送；
5. 用户 Esc/Ctrl+C → token.cancel()；
6. worker 返回部分报告；
7. 主线程转入报告页，提交 `i` 时写忽略库并重新计数；
8. `e` 打开导出菜单，写文件后显示路径。

---

## 9. 语义层与 GPU 设计

### 9.1 特性与依赖

```toml
semantic         = ["dep:ort"]                       # CPU
semantic-directml = ["semantic", "ort/directml"]     # Windows GPU
semantic-coreml  = ["semantic", "ort/coreml"]        # macOS GPU
```

默认构建不包含 ONNX，保证核心二进制小、无原生运行时依赖。

### 9.2 设备选择

| 请求 | Windows | macOS | Linux |
|---|---|---|---|
| `auto` | DML → CPU | CoreML → CPU | CPU |
| `cpu` | CPU | CPU | CPU |
| `dml` | DML → CPU | 平台拒绝 → CPU | 平台拒绝 → CPU |
| `coreml` | 平台拒绝 → CPU | CoreML → CPU | 平台拒绝 → CPU |

`--no-gpu` 将请求强制为 CPU。

### 9.3 降级决策

```text
if semantic off: disabled
if no model: disabled（auto 静默；on 警告）
if feature unavailable: disabled + 警告
for device in candidates:
    if GPU not supported by platform:
        显式请求 → 警告，继续 CPU
    try create session with EP error_on_failure
        success → active，记录实际设备
        failure → 记录警告
if CPU failed: disabled + 警告
```

- `auto` 在 GPU 失败但 CPU 成功时不输出噪音警告；
- 显式 GPU 失败并回退 CPU 时，`fallback_occurred=true`；
- 推理阶段失败时，Engine 尝试重建 CPU 会话；
- `allow_gpu_fallback=false` 时，GPU 初始化失败会禁用语义层，而不是 CPU 回退；规则检查继续；
- 所有失败路径都不改变 `Report.issues` 结构。

### 9.4 推理管线

```text
文本 → 按标点/换行分块（≤ max_text_length，内部上限 500 字符）
     → BERT WordPiece tokenize（vocab.txt）
     → [CLS] + tokens + [SEP]
     → [1, seq] int64 张量
     → ORT Session（CPU/DML/CoreML）
     → logits [1, seq, vocab]
     → 每个中文单字 argmax + softmax 置信度
     → 与原文不同且 ≥ min_confidence
     → SEMANTIC_CSC Issue
```

只对单字符中文 token 生成建议，避免英文子词/特殊 token 导致破碎替换。

### 9.5 版本锁定

`ort = "=2.0.0-rc.10"`（ONNX Runtime 1.22）。原因：

- DirectML EP 在 ONNX Runtime 1.25+ 的官方分发方式已变化；
- 1.22 是经本项目实测能同时支持 CPU 与 DirectML 的组合；
- Windows 发布包附带 `DirectML.dll`。

### 9.6 取消与性能限制

- ORT 的单次 `Run` 内部不可抢占；
- 取消在分块之间、每 64 个 token 检查；
- 典型 400 字符块在 GPU 上通常远低于 2 秒，但无法对任意模型/硬件保证；
- 这个限制在 [semantic.md](semantic.md) 中明确告知用户。

---

## 10. 可取消与并发模型

- `CancellationToken` = `Arc<AtomicBool>`；
- 轮询点：文件循环、规则循环、规则内部主循环、外部子进程等待、语义分块；
- 非 TUI 检查在主线程执行，Ctrl+C 处理器设置 token；
- TUI 检查在独立线程执行，主线程保持渲染和输入响应；
- 不共享可变 Engine 状态；TUI worker 每次检查新建 Engine；
- TUI 在检查中只允许取消，不允许直接退出，避免与不可抢占推理和终端清理发生竞态。

---

## 11. 安全与隐私

- 默认零网络请求；
- 模型下载需要显式 URL；
- HTML 报告：所有动态文本 HTML 转义，CSS 内联，无脚本、无外部资源；
- 外部检查器：
  - 只执行用户显式配置的命令；
  - 不通过 shell 解释；
  - 不继承完整环境变量；
  - 超时/取消会终止；
  - 文档明确告知无沙箱，用户需自行容器化隔离不可信脚本；
- TUI/CLI 不写源文件；
- 忽略库和配置写入用户数据目录；
- 临时输出文件与 rename 减少半写文件风险。

---

## 12. 跨平台设计

| 关注点 | 处理 |
|---|---|
| 路径分隔符 | 报告统一 `/`；配置 glob 统一 `/` |
| 文件编码 | UTF-8/UTF-16 BOM；docx 内部 XML 为 UTF-8 |
| 终端 | crossterm + ratatui；非 TTY 自动禁用 TUI |
| GPU | 平台特性编译期隔离；运行时 EP 能力再判断 |
| 进程环境 | Windows 保留必要系统变量；Python 协议设 UTF-8 |
| 子进程 kill | POSIX/Windows 统一 `Child::kill`；不保证终止进程树 |
| CI | Windows/macOS/Linux 三平台测试 + 语义后端编译检查 |
| 发布 | Windows 首发；macOS/Linux 源码构建 |

macOS CoreML 分支已实现但未在 Apple 硬件上做发布级实测，作为已知风险。

---

## 13. 错误处理与退出码

| 场景 | 行为 | 退出码 |
|---|---|---|
| 无 error | 正常输出 | 0 |
| 存在 error | 输出报告 | 1 |
| 参数错误 | clap 报错 | 2 |
| 文件不存在/格式错误/写文件失败 | stderr 错误 | 2 |
| 规则集/配置无效 | stderr 错误 | 2 |
| 外部检查器失败 | 警告，继续 | 取决于是否仍有 error |
| 语义不可用 | 警告/降级，继续 | 取决于规则结果 |
| 用户取消 | 部分报告，`cancelled=true` | 3 |

外部检查器与语义层的失败不会把“规则检查本身”变成运行错误；它们只是可选层。

---

## 14. 性能设计

预算：

- 10 万字规则检查 < 3 秒；
- 取消响应 < 2 秒（规则路径）；
- 语义 CPU 10 万字 < 60 秒为参考值，不作硬承诺。

手段：

- release：`lto = "thin"`、单 codegen unit、strip；
- Markdown 屏蔽区只构建一次；
- 每条规则一次或少量线性扫描；
- issue 只在内存中保留结果，不保留整本文档集合；
- 目录检查逐文件提取，随后释放 Document；
- 语义按块推理并限制 token 序列；
- 报告排序按文件索引与字节 offset，避免字符串位置比较。

实测（Windows，release，108,000 字符 / 72,000 汉字输入）：规则检查约 1.24 秒。

---

## 15. 测试策略

| 层级 | 内容 |
|---|---|
| 单元测试 | 文档位置、Markdown 屏蔽、各规则正反例、配置优先级、glob、CSV/HTML 转义、TUI 渲染 |
| 集成测试 | 真实 CLI 二进制、docx fixture、目录聚合、忽略持久化、规则集、外部检查器、覆盖保护 |
| 语义测试 | toy ONNX + vocab；CPU 推理；DirectML 路径；降级字段 |
| 取消测试 | 预取消 → cancelled=true、退出码 3 |
| 门禁 | `cargo fmt --check`、`cargo clippy -D warnings`、全测试 |
| 打包验收 | Windows release zip、SHA-256、包内 exe `version`、DML 实测 |

当前结果：

- 默认特性：51 个测试通过；
- `--features semantic-directml`：55 个测试通过；
- Windows DirectML toy 模型实机通过；
- 打包产物 `git: v0.7.2`。

---

## 16. 版本管理与发布

- 应用版本唯一来源：`Cargo.toml`；
- 报告 schema：`SCHEMA_VERSION = "1.0"`；
- 忽略库 schema：`1`；
- 外部协议版本：`1.0`；
- `build.rs` 嵌入 `git describe --tags --always --dirty`；
- CRLF/LF 由 `.gitattributes` 固定；
- 发布用 annotated tag；
- CHANGELOG 遵循 Keep a Changelog；
- Windows 发布脚本运行测试、构建 `semantic-directml`、收集 `DirectML.dll`、生成 SHA-256 和 zip。

当前发布标签：`v0.7.2`。后续纯文档提交不移动已发布 tag。

---

## 17. 关键设计决策

| 决策 | 选择 | 备选 | 理由 / 代价 |
|---|---|---|---|
| 分发形态 | 单 crate 单二进制 | 多 crate workspace / 服务 | 交付快、部署简单；用 `sdk` 模块为未来拆分留口 |
| 文档读取 | 直接解析 docx ZIP/XML | pandoc、LibreOffice、Word COM | 无外部依赖、可保留段落映射；不支持复杂 Word 特性 |
| 检查策略 | 规则/词典 + 可选小模型 | 在线 LLM / 纯规则 | 离线、可解释、CPU 可用；语义能力有限 |
| 报告模型 | 单一 `Report`，导出器只读 | 各格式独立 DTO | 字段稳定、易测试；批量需可选 `file` 扩展 |
| 语义运行时 | ort 1.22 固定 | 最新 ort / 自建 C API | DirectML 可用性优先；需要跟踪版本风险 |
| GPU 策略 | 显式请求失败回退 CPU，auto 静默 | 强制 GPU / 禁用 CPU | 不阻塞用户；需用 `fallback_occurred` 保持透明 |
| 外部扩展 | 子进程 JSON | 动态库 / WASM | 语言无关、隔离更好；无沙箱，需要用户信任脚本 |
| TUI 检查 | worker 线程 + mpsc | 主线程同步 | 可显示进度并响应取消；增加状态同步复杂度 |
| 错误接口 | anyhow + 退出码映射 | 复杂错误枚举 | CLI 友好；库级扩展仍需区分错误类型 |
| 规则冲突 | 后加载覆盖 + 警告 | 硬失败 | 规则集可组合；需文档和告警保证可发现性 |

---

## 18. 已知限制与风险

| 风险/限制 | 影响 | 缓解 |
|---|---|---|
| DirectML 在 ORT 1.25+ 的去留 | 未来升级不确定 | 锁定 1.22；CPU 回退；版本说明 |
| CoreML 动态形状/INT8 支持有限 | Mac 语义可能失败 | 自动 CPU；文档；待 Apple 实机验证 |
| 单次 ORT 推理不可抢占 | 极端情况下取消 > 2 秒 | 分块、小批量；明确限制 |
| 模型未随包分发 | 用户需自行下载/放置 | `models download` + 文档 |
| 自定义规则误报 | 词典/正则可能过匹配 | 白名单、忽略、上下文条件、测试 |
| 外部检查器无沙箱 | 恶意脚本风险 | 仅显式配置、环境隔离、文档警告 |
| `ruleset add` 不复制资源文件 | 相对词典路径失效 | 建议自包含或绝对路径；后续资源打包 |
| 大文件内存 | 文档与 logits 占用内存 | 分块；后续可流式 docx |
| 批量报告替代 per-file 输出 | 某些 CI 消费不便 | 可选 `file` 字段；未来支持输出目录 |
| SentencePiece 未支持 | 部分模型无法直接用 | BERT WordPiece 优先；路线图 |

---

## 19. 后续演进

1. **errator-sdk 独立 crate**：把 `Rule`、`Exporter`、注册与 CLI 嵌入能力稳定下来。
2. **规则集资源打包**：安装时携带外部词典/脚本，校验完整性。
3. **模板式导出器**：`--format custom --template`，避免用户写 Rust。
4. **SentencePiece 与模型清单**：支持更多 CSC 模型；内置默认模型 URL + 校验和。
5. **语义取消增强**：更细粒度分块、超时和会话重建策略。
6. **输出目录模式**：目录检查时按文件输出报告，同时保留聚合摘要。
7. **TUI 搜索/跳转/分页**：大报告可用性。
8. **WASM 规则沙箱**：仅在社区有强需求时实现稳定 ABI。
9. **CoreML 发布级验证**：Apple Silicon 实测 + FP16/静态形状模型建议。
10. **性能基准 CI**：固定大文本基准，防止规则退化。

---

## 20. 目录结构

```text
errator/
├─ Cargo.toml / Cargo.lock
├─ build.rs
├─ README.md
├─ CHANGELOG.md
├─ LICENSE
├─ docs/
│  ├─ design.md               ← 本文
│  ├─ architecture.md         模块与扩展点速查
│  ├─ configuration.md        配置字段
│  ├─ rules.md                规则表与新增规则
│  ├─ rulesets.md             规则集
│  ├─ external-checkers.md    L2 协议
│  ├─ semantic.md             模型与 GPU
│  ├─ versioning.md           发布流程
│  └─ THIRD_PARTY_NOTICES.md
├─ scripts/package-windows.ps1
├─ src/                       Rust 实现
├─ tests/                     集成测试与 fixtures
├─ .github/workflows/         CI / release
├─ 需求.md                    本地需求（gitignored）
└─ 扩展.md                    本地扩展文档（gitignored）
```

---

## 21. 结论

Errator 0.7.2 的核心取舍是：**把确定性的规则检查与稳定的报告契约做扎实，把语义能力和 GPU 作为可替换、可降级、可观测的可选层**。这一结构允许后续在不改变 CLI、TUI 和报告 schema 的前提下增加规则、检查器、导出器和推理后端；也明确记录了当前在 CoreML、SentencePiece、取消粒度和规则集资源打包上的限制。
