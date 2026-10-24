# Errator

本地文本勘误报告生成器。读取 `.txt` / `.md` / `.docx`，运行格式、标点、空格、错别字、术语一致性与自定义规则检查；可选加载本地 ONNX 模型做上下文纠错。结果可在终端查看，也可导出 Markdown / HTML / JSON / CSV；无参数启动 TUI，全程默认离线。

当前版本：**0.7.2**（Windows 优先发布；源码面向 Windows / macOS / Linux）。

## 目录

- [特性](#特性)
- [1. 安装与验证](#1-安装与验证)
- [2. 基础使用](#2-基础使用)
- [3. TUI 使用说明](#3-tui-使用说明)
- [4. CLI 命令参考](#4-cli-命令参考)
- [5. 配置与自定义规则](#5-配置与自定义规则)
- [6. 报告格式与退出码](#6-报告格式与退出码)
- [7. 可选语义检查](#7-可选语义检查)
- [8. CI 与自动化集成](#8-ci-与自动化集成)
- [9. 常见问题](#9-常见问题)
- [10. 跨平台与构建](#10-跨平台与构建)
- [11. 开发与测试](#11-开发与测试)
- [12. 版本管理](#12-版本管理)
- [13. 隐私与安全](#13-隐私与安全)
- [14. 已知限制](#14-已知限制)
- [文档索引](#文档索引)

## 特性

- **两种形态**：脚本友好的 `errator check`，以及选文件、看详情、忽略误报、导出报告的交互式 TUI。
- **本地优先**：检查与报告渲染不发起网络请求；HTML 报告单文件自包含。
- **定位准确**：纯文本使用 1-based `line/column`；docx 使用 `paragraph/char_offset`，不伪造行号。
- **可扩展**：路径/文件类型分级、声明式词对与正则规则、外部检查器 JSON 协议、规则集、Rust `Rule` trait、`Exporter` trait。
- **可选语义层**：ONNX Runtime CPU 始终可用；Windows DirectML、macOS CoreML 为可选加速，初始化/推理失败自动回退 CPU，报告记录真实设备。
- **设备透明**：CLI 摘要、Markdown/HTML 报告和 TUI 状态栏明确显示实际执行设备（CPU 规则检查 / CPU ONNX / GPU DirectML / GPU CoreML）。
- **误报治理**：TUI `i` 持久化忽略；CLI `ignore add/list/remove/clear`；每次运行可用 `--ignore` 临时过滤。
- **上下文可读性**：上下文最长保留 400 字；TUI 详情中用高亮色标出问题原文，并支持滚动查看长段落。
- **快速定位**：TUI 文件浏览页按 `/` 按文件名筛选当前目录；报告页按 `t` 在问题类型（category）之间筛选。
- **可取消**：`CancellationToken` + 流式进度；取消时输出部分报告并返回退出码 `3`。

---

## 1. 安装与验证

### 1.1 使用 Windows 发布包

发布包解压后包含：

```text
errator.exe
DirectML.dll
README.md
CHANGELOG.md
LICENSE
THIRD_PARTY_NOTICES.md
VERSION.txt
docs/
```

`DirectML.dll` 必须与 `errator.exe` 放在同一目录；否则 ONNX GPU 加速不可用，但 CPU/规则检查不受影响。

在 PowerShell 中：

```powershell
# 假设解压到 D:\tools\errator-0.7.2-windows-x86_64
cd D:\tools\errator-0.7.2-windows-x86_64

.\errator.exe version
.\errator.exe check .\doc.md
```

可选：仅当前 PowerShell 会话加入 PATH：

```powershell
$env:Path += ";D:\tools\errator-0.7.2-windows-x86_64"
errator check .\doc.md
```

需要长期生效时，可把该目录加入用户 PATH（通过“系统属性 → 环境变量”或等价命令）。重新打开终端后使用 `errator`。

校验下载包（PowerShell）：

```powershell
Get-FileHash .\errator-0.7.2-windows-x86_64.zip -Algorithm SHA256
# 与 .sha256 文件中的值比较
```

> 不要把 `errator.exe` 单独移到其他目录后使用 DirectML。非语义功能仍可运行，但找不到同目录的 `DirectML.dll` 时会降级。

### 1.2 从源码构建

要求 Rust 1.88+ 与 Cargo。

默认构建（规则 + CLI + TUI，不含 ONNX）：

```bash
cargo build --release
./target/release/errator --version
```

Windows 完整版本（含 ONNX + DirectML）：

```powershell
cargo build --release --features semantic-directml
.\target\release\errator.exe --version
```

macOS 可选 CoreML：

```bash
cargo build --release --features semantic-coreml
```

Linux CPU 语义层：

```bash
cargo build --release --features semantic
```

快速生成 Windows 发布 zip（运行测试并自动收集 DLL）：

```powershell
pwsh -File scripts/package-windows.ps1
```

产物位于 `dist/`。

### 1.3 首次初始化

建议先执行一次：

```bash
errator init
```

它会在 `~/.errator/` 下创建：

```text
~/.errator/
  config.toml          带注释的默认配置
  ignores.json         持久化忽略库
  models/              本地 ONNX 模型
  rulesets/            已安装规则集
```

Windows 对应 `%USERPROFILE%\.errator\`。已有配置不会被覆盖；需要重建时使用：

```bash
errator init --force
```

使用自定义数据目录（便携/隔离环境）：

```powershell
$env:ERRATOR_HOME = "D:\portable\errator-data"
errator init
```

`ERRATOR_HOME` 同时影响配置、忽略库、模型和规则集。

### 1.4 验证安装

```bash
errator version
errator version --json
errator devices
```

`errator devices` 会显示当前二进制是否编译了语义后端、是否发现本地模型，以及 CPU/DirectML/CoreML 的可用状态。

---

## 2. 基础使用

### 2.1 检查单个文件

```bash
errator check doc.md
```

默认输出终端文本报告。问题按文件位置排序，包含级别、规则 ID、位置、原文、建议、说明和来源。

示例：

```text
Errator 勘误报告
源文件：doc.md
生成时间：2026-10-24T21:06:39+08:00
工具版本：0.7.2
计算设备：CPU（规则检查）
语义检查：关闭（仅规则检查）

问题总数：1  |  error：0  |  warning：1  |  info：0

  1. [warning] 1:1（text）  CJK_SPACE_MISSING
     原文：使用Rust开发
     建议：使用 Rust 开发
     中英文/数字之间建议加显式空格 · 来源：rule（置信度 1.00）
```

支持的输入：

- `.txt`
- `.md` / `.markdown`
- `.docx`

不支持旧版 `.doc`，请先用 Word 另存为 `.docx`。

### 2.2 选择输出格式

```bash
errator check doc.md --format text     # 默认，终端可读
errator check doc.md --format json     # 机读
errator check doc.md --format md       # Markdown 报告
errator check doc.md --format html     # 单文件 HTML
errator check doc.md --format csv      # 表格
```

`--format markdown` 是 `md` 的别名。

### 2.3 保存报告

```bash
errator check doc.md --format md -o report.md
errator check doc.md --format html -o report.html
errator check doc.md --format json -o report.json
```

- 输出目录不存在时会自动创建。
- 目标文件已存在时默认拒绝写入，使用 `--force` 覆盖：
  ```bash
  errator check doc.md --format md -o report.md --force
  ```
- 使用 `-o` 时，stdout 保持干净；写入提示走 stderr。

### 2.4 批量检查目录

```bash
errator check ./docs
errator check ./docs --format md -o report.md
```

- 目录递归扫描 `.txt` / `.md` / `.docx`。
- 按路径排序，报告在 issue 上增加可选 `file` 字段。
- 单文件报告不包含 `file` 字段，保持 schema 1.0 原样。
- 目录为空或不含支持的文件时返回运行错误（退出码 `2`）。

示例 JSON 中的批量 issue：

```json
{
  "file": "docs/guide.md",
  "rule_id": "CJK_SPACE_MISSING",
  "location": { "kind": "text", "line": 12, "column": 8 }
}
```

### 2.5 过滤问题

```bash
errator check doc.md --min-severity warning   # 显示 error + warning
errator check doc.md --min-severity error     # 只显示 error
errator check doc.md --no-context             # 不附带上下文
```

临时忽略某些规则（仅本次运行）：

```bash
errator check doc.md --ignore CJK_SPACE_MISSING
errator check doc.md --ignore CJK_SPACE_MISSING,PUNCT_DUPLICATE
```

`--ignore` 接受**规则 ID**，不是忽略项 ID。

### 2.6 安静模式与颜色

```bash
errator check doc.md --quiet
errator check doc.md --no-color
```

- `--quiet` 仅影响 `--format text`：只输出一行摘要。
- JSON/Markdown 等显式格式不受 `--quiet` 影响。
- 颜色仅在 stdout 是 TTY 时自动开启；`--no-color` 强制关闭。

### 2.7 取消检查

- 非 TUI 模式下 `Ctrl+C` 会请求取消，输出已完成的部分报告，退出码 `3`。
- `--no-cancel` 会忽略取消请求，直到检查完成。
- TUI 检查中按 `Esc` 或 `q` 请求取消。

---

## 3. TUI 使用说明

### 3.1 启动

```bash
errator                     # 在交互式终端中从当前目录启动
errator tui                 # 同上
errator tui ./docs          # 指定起始目录
errator tui ./doc.md        # 直接检查文件
```

非 TTY 环境（CI、重定向输出）不会启动 TUI；裸跑 `errator` 会输出帮助。需要检查时使用 `errator check`。

可用参数：

```bash
errator tui ./docs --config ./errator.toml
errator tui ./docs --ruleset project
errator tui ./docs --semantic on --device cpu
errator tui ./docs --no-gpu
```

### 3.2 文件浏览

| 按键 | 功能 |
|---|---|
| `↑` / `↓` 或 `k` / `j` | 移动选择 |
| `PageUp` / `PageDown` | 快速移动 |
| `Home` / `End` 或 `g` / `G` | 跳到首/尾 |
| `Enter` / `→` | 打开目录或开始检查文件 |
| `Backspace` / `←` | 返回上级目录 |
| `/` | 按文件名筛选当前目录 |
| `r` | 刷新当前目录 |
| `?` | 帮助 |
| `q` / `Esc` | 退出 |

浏览器只显示目录和受支持的文件；`[DIR]` 表示目录，`[MD ]` / `[TXT]` / `[DOCX]` 表示可检查文件。

#### 目录关键字筛选

在文件浏览页按 `/` 进入搜索状态：

- 输入关键字时，列表会实时按文件名/目录名做**不区分大小写的包含匹配**；
- `Enter` 保留当前筛选并退出输入状态；
- `Esc` 取消本次编辑，恢复进入搜索前的筛选；
- `Backspace` 删除一个字符，`Ctrl+U` 清空输入；
- 筛选状态下按 `↑/↓` 可在结果中移动；
- 筛选生效时再按 `Esc` 清除筛选；再按一次退出 TUI；
- 进入其他目录或刷新会清除筛选。

### 3.3 检查与报告页

选择文件后进入检查页，显示：

- 当前文件与进度；
- 已经过时间；
- 当前计算设备（如 `CPU（规则检查，未启用语义层）`、`GPU · DirectML（语义推理）`）。

检查完成后进入报告页：

- 左侧（窄屏为上方）是问题列表；
- 右侧（窄屏为下方）显示选中问题的详情、原文、建议、上下文与来源；
- 详情采用紧凑布局：标题包含类型，位置/来源/置信度合并一行，上下文通常在第六行左右出现；
- “上下文”最长约 400 字，问题前保留约 96 字；问题原文在上下文中用彩色底色高亮；即使上下文与原文完全相同也会展示；
- 同一段中存在重复词时，高亮位置会尽量依据问题在段落中的 `char_offset/column` 选择正确的那一处；
- 顶部第一行固定显示**计算设备**与问题计数，第二行显示源文件，第三行显示筛选状态；
- 底部第一行是快捷键，第二行固定显示计算设备与最近状态/提示；
- 文件浏览页的底部显示目录状态和快捷键。

| 按键 | 功能 |
|---|---|
| `↑` / `↓` 或 `k` / `j` | 切换问题 |
| `PageUp` / `PageDown` | 快速移动 |
| `Home` / `End` 或 `g` / `G` | 首/尾问题 |
| `f` | 循环筛选级别：全部 → error → warning → info |
| `0` / `1` / `2` / `3` | 直接筛选全部 / error / warning / info |
| `t` | 循环筛选问题类型（category），如 `spacing`、`punctuation`、`semantic` |
| `Ctrl+↑` / `Ctrl+↓` | 滚动问题详情（长上下文） |
| `Ctrl+PageUp` / `Ctrl+PageDown` | 快速滚动问题详情 |
| `i` | 忽略当前误报并持久化 |
| `e` | 打开导出菜单 |
| `Esc` | 返回文件浏览 |
| `?` | 帮助 |
| `q` | 退出 |

等级筛选与类型筛选可叠加；顶部会显示当前 `筛选：级别 · 类型：category` 和显示条数。类型筛选按问题数量排序，`t` 依次切换“全部类型 → 各类型 → 全部类型”。

### 3.4 忽略误报

按 `i` 后，Errator 会：

1. 把当前问题的 `rule_id + original` 写入 `~/.errator/ignores.json`；
2. 从当前报告中移除所有匹配项；
3. 重新计算摘要。

忽略是**全局**的，不区分文件；下次检查任何文件时同样生效。可以用 CLI 管理：

```bash
errator ignore list
errator ignore list --format json
errator ignore remove <id>
errator ignore clear
```

### 3.5 导出报告

在报告页按 `e`，然后：

| 按键 | 格式 |
|---|---|
| `m` | Markdown |
| `h` | HTML |
| `j` | JSON |
| `c` | CSV |

文件写在源文件同目录，名称形如：

```text
doc.errator-20261024-210639.md
```

不会覆盖已有报告；如果同一秒内重名会追加序号。导出路径会显示在状态栏。

### 3.6 取消与退出

- 检查页：`Esc` / `q` 请求取消；不可抢占的单次语义推理结束后返回。
- 报告页：`Esc` 返回文件浏览，`q` 退出。
- `Ctrl+C`：检查中请求取消，其他页面退出。
- TUI 不修改源文件。

---

## 4. CLI 命令参考

所有命令和子命令都支持 `--help`：

```bash
errator --help
errator check --help
errator rules --help
```

### 4.1 `errator`

```bash
errator
```

交互式 TTY 中从当前目录启动 TUI；非 TTY 输出帮助。

### 4.2 `errator check <path>`

```bash
errator check <FILE_OR_DIR> [OPTIONS]
```

参数：

| 参数 | 默认 | 说明 |
|---|---|---|
| `--format text\|json\|md\|html\|csv` | `text` | 报告格式 |
| `-o, --output <FILE>` | stdout | 写入文件 |
| `--config <FILE>` | 自动发现 | 显式配置文件 |
| `--ruleset <NAME>` | — | 加载规则集，可重复 |
| `--semantic auto\|on\|off` | 配置/`auto` | 语义层模式 |
| `--device auto\|cpu\|dml\|coreml` | 配置/`auto` | 推理设备 |
| `--no-gpu` | false | 等价 `--device cpu` |
| `--min-severity error\|warning\|info` | 配置/`info` | 最低显示级别 |
| `--ignore <RULE[,RULE]>` | — | 本次忽略规则 |
| `--no-context` | false | 不附加上下文 |
| `--no-color` | false | 禁用 ANSI 颜色 |
| `--quiet` | false | 仅 text 摘要 |
| `--no-tui` | false | 兼容参数；永不自动进 TUI |
| `--no-cancel` | false | 忽略 Ctrl+C |
| `--force` | false | 覆盖输出文件 |

退出码见 [报告格式与退出码](#6-报告格式与退出码)。

### 4.3 `errator rules`

列出所有内置与自定义规则：

```bash
errator rules list
errator rules list --format json
errator rules list --ruleset project
```

测试自定义规则的正例/反例（来自 `[[rules.custom.tests]]`）：

```bash
errator rules test
errator rules test --config ./errator.toml
errator rules test --ruleset project
```

任一测试失败时命令返回退出码 `1`。

### 4.4 `errator ignore`

```bash
errator ignore add --rule CJK_SPACE_MISSING --text "使用Rust开发"
errator ignore add --rule CUSTOM_LOGIN --text "登陆" --note "产品名"
errator ignore list
errator ignore list --format json
errator ignore remove <id>
errator ignore clear
```

- `add` 至少需要 `--rule` 和 `--text`。
- 持久化匹配使用精确的 `rule_id + original`。
- `--note` 只用于说明。
- `clear` 清空全部持久化忽略，并显示删除条目数。
- TUI 的 `i` 等价于 `ignore add`。
- 对外部检查器/语义规则 ID 也可添加；若 ID 不在内置注册表中，会给出提示但允许保存。

### 4.5 `errator ruleset`

```bash
errator ruleset add ./rulesets/project.toml
errator ruleset add ./rulesets/project.toml --force
errator ruleset list
errator ruleset remove project
```

安装后：

```bash
errator check doc.md --ruleset project
```

项目配置中也可默认加载：

```toml
[general]
rulesets = ["project"]
```

规则集格式、继承和合并规则见 [docs/rulesets.md](docs/rulesets.md)。

### 4.6 `errator models`

```bash
errator models download --url "https://example.com/macbert4csc.onnx"
errator models download --url "..." --name macbert4csc.onnx --sha256 "<hex>" --force
errator models list
```

- 下载目标默认为 `~/.errator/models/`。
- `--name` 必须以 `.onnx` 结尾。
- 没有 `--url` 时读取 `ERRATOR_MODEL_URL`；两者都没有则报错。
- 模型下载是唯一由核心工具主动发起的网络行为。
- 下载完成后需自行提供同目录 `vocab.txt`，或配置 `semantic.vocab_path`。

### 4.7 `errator devices`

```bash
errator devices
```

显示语义后端是否编译、模型发现状态和各后端可用性。用于排查 GPU 不生效、模型未发现等问题。

### 4.8 `errator init`

```bash
errator init
errator init --force
errator init --config ./errator.toml
```

创建/重建默认配置、忽略库、模型目录、规则集目录。`--config` 仅改变配置路径，其余数据仍在 `ERRATOR_HOME`。

### 4.9 `errator version`

```bash
errator version
errator version --json
errator --version
```

显示应用版本、报告 schema、Git 描述、平台、架构与构建 profile。

---

## 5. 配置与自定义规则

### 5.1 配置发现顺序

1. `--config <FILE>`
2. 当前目录的 `errator.toml`
3. `~/.errator/config.toml`

显式配置不存在会报错；默认配置不存在则使用内置默认值。项目本地配置适合随仓库提交团队规则。

### 5.2 常用配置

```toml
[general]
min_severity = "info"
context = true
rulesets = ["project"]

[rules]
disabled = ["CJK_SPACE_EXTRA"]

[rules.severity_overrides]
CJK_SPACE_MISSING = "auto"

[rules.by_file_type.docx]
CJK_SPACE_MISSING = "info"

[rules.by_path."docs/legacy/**"]
PUNCT_HALF_WIDTH = "off"

[dictionary]
words = ["Errator"]
ignore_words = ["错误词"]

[[dictionary.confusions]]
wrong = "勘误师"
right = "勘误执行者"
severity = "info"

[semantic]
mode = "auto"
device = "auto"
allow_gpu_fallback = true
```

等级优先级：

```text
by_path > by_file_type > severity_overrides > 内置默认
```

可用等级：`error` / `warning` / `info` / `off` / `auto`。`auto` 表示采用规则自身默认（例如 docx 的空格规则是 info）。

完整字段说明见 [docs/configuration.md](docs/configuration.md)。

### 5.3 声明式词对规则

```toml
[[rules.custom]]
id = "CUSTOM_LOGIN"
type = "pair"
category = "typo"
severity = "warning"
wrong = "登陆"
right = "登录"
case_sensitive = false
when_prev = ["账号", "系统"]
window = 8
message = "账号场景应使用「登录」"
include_excluded = false

[[rules.custom.tests]]
input = "系统登陆异常"
expect = ["CUSTOM_LOGIN"]

[[rules.custom.tests]]
input = "船只登陆海滩"
expect = []
```

- `when_prev` / `when_next`：前/后窗口内任一命中即可。
- `when_prev_regex` / `when_next_regex`：提供的正则必须全部匹配。
- 英文词自动要求 ASCII 边界，中文按字符边界。
- `include_excluded = true` 时也检查代码块/URL。

### 5.4 正则规则

```toml
[[rules.custom]]
id = "CUSTOM_TODO"
type = "regex"
category = "format"
severity = "info"
pattern = 'TODO\((\w+)\)'
replacement = "TODO($1)"
message = "TODO 标记格式不规范"
```

`replacement` 可省略；此时只报告，不提供替换建议。

### 5.5 外部检查器

```toml
[external.terminology]
command = "python checkers/terminology.py"
timeout_ms = 5000
enabled = true
cwd = "."
[external.terminology.env]
PROJECT = "docs"
```

外部检查器通过 stdin/stdout JSON 交换 issue；退出码非 0、超时或输出非法只产生警告。完整协议见 [docs/external-checkers.md](docs/external-checkers.md)。

### 5.6 扩展层级

| 层级 | 方式 | 是否写代码 |
|---|---|---|
| L0 | 启停规则、覆盖等级、按路径/类型分级 | 否 |
| L1 | 词对规则、正则规则、上下文条件、词典、规则集 | 否 |
| L2 | 任意语言外部检查器 | 是（非 Rust） |
| L3 | Rust `Rule` / `Exporter` / `sdk` | 是（Rust） |

内置规则表与新增规则流程见 [docs/rules.md](docs/rules.md)，模块设计见 [docs/design.md](docs/design.md)。

---

## 6. 报告格式与退出码

### 6.1 Text

适合终端人工阅读，默认格式。显示摘要、设备、语义状态、逐条问题；`--quiet` 只保留摘要。

### 6.2 Markdown

固定结构：

- `# Errator 勘误报告`
- 源文件、生成时间、工具版本、是否取消、语义检查、计算设备
- 摘要表
- 问题列表（位置、原文、建议、说明、来源、上下文）
- 无问题时输出“未发现问题”

原文与建议放在动态长度 fenced code block 中，避免 Markdown 特殊字符破坏渲染。

### 6.3 HTML

单文件自包含：内联 CSS、无脚本、无外部资源；支持浅色/暗色；所有动态文本做 HTML 转义。

### 6.4 JSON

稳定 schema：

```json
{
  "schema_version": "1.0",
  "tool": { "name": "errator", "version": "0.7.2" },
  "generated_at": "2026-10-24T21:06:39+08:00",
  "source": "doc.md",
  "cancelled": false,
  "semantic": {
    "enabled": false,
    "device": "cpu",
    "backend": "none",
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
      "original": "使用Rust开发",
      "suggestion": "使用 Rust 开发",
      "message": "中英文/数字之间建议加显式空格",
      "source": "rule",
      "confidence": 1.0
    }
  ]
}
```

- 目录批量报告额外增加可选 `file` 字段。
- 内部排序字段 `offset` 不序列化。
- GPU/CPU 切换不改变 issues 结构。
- JSON 中含实际设备与 `fallback_occurred`。

### 6.5 CSV

固定表头：

```text
file,rule_id,category,severity,kind,line,column,paragraph,char_offset,original,suggestion,message,source,confidence
```

UTF-8、CRLF、RFC 4180 风格引号转义，适合电子表格与数据管道。

### 6.6 退出码

| 退出码 | 含义 |
|---|---|
| `0` | 无 error；warning/info 不影响 |
| `1` | 报告中存在 error（未过滤/未忽略） |
| `2` | 运行错误：路径、配置、docx、写文件等 |
| `3` | 用户取消，报告为部分结果 |

`--min-severity` 和忽略项在退出码判定前生效。

---

## 7. 可选语义检查

### 7.1 准备模型

语义检查需要：

1. BERT WordPiece 词表的 ONNX 纠错模型；
2. 同目录的 `vocab.txt`；
3. 二进制包含 `semantic` feature（Windows 发布包默认包含 CPU + DirectML）。

放置到：

```text
~/.errator/models/
  macbert4csc.onnx
  vocab.txt
```

也可以通过配置显式指定：

```toml
[semantic]
mode = "auto"
device = "auto"
model_path = "D:/models/macbert4csc.onnx"
vocab_path = "D:/models/vocab.txt"
min_confidence = 0.5
max_text_length = 400
```

`model_path` 为空时，Errator 会选取 `~/.errator/models/` 中最新的 `.onnx`。

### 7.2 检查设备状态

```bash
errator devices
```

会显示：

- 当前构建是否包含语义后端；
- 是否发现本地模型；
- CPU/DirectML/CoreML 是否被编译。

### 7.3 运行语义检查

```bash
errator check doc.md --semantic auto
errator check doc.md --semantic on --device auto
errator check doc.md --semantic on --device cpu
errator check doc.md --semantic on --device dml
errator check doc.md --no-gpu
```

| 模式 | 行为 |
|---|---|
| `auto` | 发现模型才启用；GPU 不可用静默用 CPU |
| `on` | 要求语义层；无模型/后端失败时告警但继续规则检查 |
| `off` | 完全不加载模型 |
| `--device cpu` | 只用 CPU |
| `--device dml` | Windows DirectML，失败回退 CPU |
| `--device coreml` | macOS CoreML，失败回退 CPU |
| `--device auto` | Windows 优先 DML，macOS 优先 CoreML，其他 CPU |
| `--no-gpu` | 等价 `--device cpu` |

### 7.4 设备显示

人类可读报告会显示实际计算设备：

```text
计算设备：CPU（规则检查）
```

```text
计算设备：GPU · DirectML
```

GPU 初始化或推理失败后：

```text
计算设备：CPU · ONNX Runtime（已降级）
```

`semantic.fallback_occurred=true` 时表示发生过降级；实际设备以 `semantic.device` 为准。

### 7.5 配置与限制

- `allow_gpu_fallback = false`：GPU 初始化失败会禁用可选语义层（规则检查继续），不强制 CPU 回退。
- DirectML 需要 Windows + DirectX 12 GPU，并要求 `DirectML.dll` 与 exe 同目录。
- CoreML 的动态形状/INT8 支持有限，失败自动回退 CPU。
- 单次 ONNX 推理不可在算子内部抢占，取消在分块之间生效。
- 当前版本仅支持 BERT WordPiece，不支持 SentencePiece。

完整说明见 [docs/semantic.md](docs/semantic.md)。

---

## 8. CI 与自动化集成

### 8.1 基本检查门禁

```bash
errator check docs --format json -o errator-report.json --force
```

有 error 时退出码为 `1`，可直接作为流水线失败条件。

### 8.2 生成 Markdown 报告

```bash
errator check docs --format md -o errator-report.md --force
```

报告会写入文件；同时 stderr 输出结果提示，stdout 不混入内容。

### 8.3 只关注 error/warning

```bash
errator check docs --min-severity warning --format json -o report.json --force
```

### 8.4 临时忽略规则

```bash
errator check docs --ignore CJK_SPACE_MISSING,PUNCT_DUPLICATE
```

与持久化忽略库不同，`--ignore` 不写文件，适合单次 CI。

### 8.5 GitHub Actions 示例

```yaml
- name: Errator
  run: |
    errator check docs --format md -o errator-report.md --force
```

要保留报告即使检查失败，可先允许该步失败再上传 artifact，或者使用 `if: always()`。

### 8.6 批量输出建议

当前目录批量检查生成一份聚合报告；issue 带可选 `file` 字段。若需要按文件拆分的报告，使用循环脚本：

```bash
for file in docs/*.md; do
  errator check "$file" --format json -o "reports/$(basename "$file").json" --force
done
```

---

## 9. 常见问题

### Q1：提示“不是有效的 UTF-8/UTF-16 文本”

Errator 仅接受 UTF-8、UTF-8 BOM、UTF-16 LE/BE BOM。请用编辑器另存为 UTF-8。

### Q2：输出文件已存在

```text
输出文件已存在：...（使用 --force 覆盖）
```

加 `--force`，或改用新的输出文件名。

### Q3：TUI 无法启动

```text
TUI 需要交互式终端；非 TTY 环境请使用 errator check
```

在 CI、重定向 stdout 或通过非交互管道运行时不能启动 TUI。

### Q4：`errator` 命令找不到

把发布目录加入当前 PATH：

```powershell
$env:Path += ";D:\tools\errator-0.7.2-windows-x86_64"
```

或使用完整路径 `.\errator.exe`。注意 PATH 只在当前会话有效时，重开终端会恢复。

### Q5：语义检查没有启用

按顺序检查：

```bash
errator devices
errator check doc.md --semantic on --device cpu
```

常见原因：

- 当前二进制默认构建未带 `semantic` feature；
- `~/.errator/models/` 中没有 `.onnx`；
- 缺少 `vocab.txt`；
- 配置的 `model_path` / `vocab_path` 不存在。

### Q6：显示 DirectML 但仍是 CPU/速度没变

- 检查 `semantic.fallback_occurred`；
- 确认 `DirectML.dll` 与 exe 同目录；
- 某些模型算子可能由 CPU 执行，报告设备记录的是已注册的 EP 路径；
- 用 `--device cpu` 对比，确认 GPU 是否有实际收益。

### Q7：忽略项没有生效

忽略匹配是 `rule_id + original` 精确匹配。确认：

```bash
errator rules list
errator ignore list
```

规则 ID 或原文任一不一致都不会命中。`--ignore` 使用的是规则 ID，不是忽略项 ID。

### Q8：退出码 3 是什么

用户取消。报告仍然会输出，但 `cancelled=true`。CI 中应把 `3` 视为未完成，而不是通过。

### Q9：docx 没有问题

- `docx` 的空格规则默认是 `info`，可能被 `--min-severity warning` 过滤；
- 部分 Word 内容可能是图片/文本框；
- 使用 `--format json` 检查原始结果；
- 确认文件是 `.docx`，不是旧版 `.doc`。

### Q10：报告里没有上下文

配置文件设置了 `context = false`，或使用了 `--no-context`。

### Q11：如何清空全部忽略

```bash
errator ignore clear
```

只删除某一条时先用 `errator ignore list` 查看 ID，再执行 `errator ignore remove <id>`。清空操作会修改 `~/.errator/ignores.json`；具体路径受 `ERRATOR_HOME` 影响。

---

## 10. 跨平台与构建

| 平台 | 构建 | 语义后端 | 说明 |
|---|---|---|---|
| Windows x64 | `cargo build --release --features semantic-directml` | CPU + DirectML | 当前首要发布目标；包内带 `DirectML.dll` |
| macOS | `cargo build --release --features semantic-coreml` | CPU + CoreML | 动态形状/INT8 支持有限；未做 Apple 硬件发布级实测 |
| Linux | `cargo build --release --features semantic` | CPU | 规则、CLI、TUI 完整可用 |
| 任意 | `cargo build --release` | 无 | 最小规则/CLI/TUI 构建，不链接 ONNX |

跨平台注意：

- 报告中的路径统一使用 `/`。
- 配置 glob 使用 `/`，Windows 路径会先规范化。
- 外部检查器在 Windows 上会额外设置 Python UTF-8 变量。
- 非 TTY 不启动 TUI。
- CoreML 失败自动回退 CPU。

---

## 11. 开发与测试

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --features semantic          # CPU ONNX 集成测试
cargo test --features semantic-directml # Windows DirectML 测试
```

Windows 打包：

```powershell
pwsh -File scripts/package-windows.ps1
```

测试覆盖规则正反例、Markdown 报告、docx 定位、HTML 转义、CSV、目录聚合、忽略、规则集、外部检查器、取消、TUI 渲染、语义推理与 GPU 降级。

---

## 12. 版本管理

- 语义化版本 `MAJOR.MINOR.PATCH`，唯一来源是 `Cargo.toml`。
- `build.rs` 嵌入 `git describe`，可通过 `errator version --json` 查看。
- 报告数据契约独立版本化：当前 `schema_version = "1.0"`。
- 发布以 annotated Git tag（当前 `v0.7.2`）标记，变更见 [CHANGELOG.md](CHANGELOG.md)。
- 规则 ID 发布后不重命名；废弃时保留 ID 并标记。
- 发布流程见 [docs/versioning.md](docs/versioning.md)。

---

## 13. 隐私与安全

- 默认零网络；仅显式 `models download` 和用户配置的外部检查器会触发额外网络/进程行为。
- HTML 报告无脚本、无外部资源，所有动态文本转义。
- TUI/CLI 都不修改源文件。
- 外部检查器不继承完整环境变量：只传 `PATH`（Windows 另加系统必要变量）和显式 `env`；无沙箱，只运行可信脚本。
- 模型文件不由 Errator 分发，版权与许可证由模型提供方决定。

---

## 14. 已知限制

- 语义层 当前版本仅支持 BERT WordPiece；SentencePiece 尚未支持。
- 单次 ONNX 推理不可内部抢占，取消在一次推理结束后生效；长序列 GPU 的 2 秒指标取决于模型与硬件。
- `errator models download` 不内置模型地址，需要显式 URL 与可选 SHA-256。
- 通过 `ruleset add` 安装的规则集建议自包含；相对词典/脚本路径不会随 TOML 复制。
- TUI 不编辑源文件。
- 批量目录报告在 issue 上增加可选 `file` 字段；单文件报告保持原 schema。
- 当前版本不做 Word 实时嵌入、在线服务、全文改写或 WASM 插件。

---

## 文档索引

- [docs/design.md](docs/design.md)：系统设计、数据契约、关键流程与取舍
- [docs/architecture.md](docs/architecture.md)：模块边界与扩展点速查
- [docs/configuration.md](docs/configuration.md)：完整配置字段
- [docs/rules.md](docs/rules.md)：内置规则、自定义规则、新增规则流程
- [docs/rulesets.md](docs/rulesets.md)：规则集打包与继承
- [docs/external-checkers.md](docs/external-checkers.md)：L2 子进程协议
- [docs/semantic.md](docs/semantic.md)：ONNX、DirectML、CoreML 与降级
- [docs/versioning.md](docs/versioning.md)：发布流程
- [docs/THIRD_PARTY_NOTICES.md](docs/THIRD_PARTY_NOTICES.md)：第三方组件

## 许可证

MIT，见 [LICENSE](LICENSE)。

