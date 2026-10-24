# 配置参考

Errator 的配置为 TOML。加载顺序：

1. `--config <FILE>`；
2. 当前工作目录的 `errator.toml`；
3. `~/.errator/config.toml`。

`ERRATOR_HOME` 可覆盖 `~/.errator` 数据目录（配置、忽略库、模型、规则集），用于便携安装与测试。显式配置存在但无法读取时会报错并返回退出码 2；默认配置不存在则使用内置默认值。

## 最小示例

```toml
[general]
min_severity = "info"
context = true
rulesets = ["academic-zh"]

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

[consistency]
use_builtin = true
groups = [["账号", "帐号"]]

[semantic]
mode = "auto"
device = "auto"
allow_gpu_fallback = true
max_text_length = 400
```

## `[general]`

| 字段 | 默认值 | 说明 |
|---|---|---|
| `min_severity` | `"info"` | 全局最低显示级别，可被 CLI 覆盖 |
| `context` | `true` | 是否在 issue 中记录上下文 |
| `rulesets` | `[]` | 自动加载的已安装规则集名称 |

## `[rules]`

| 字段 | 说明 |
|---|---|
| `disabled` | 全局禁用的规则 ID 列表 |
| `severity_overrides` | 规则 ID → `error` / `warning` / `info` / `off` / `auto`。旧键名 `severity` 仍被接受 |
| `by_file_type.<txt\|md\|docx>` | 按文件类型覆盖级别 |
| `by_path."<glob>"` | 按路径 glob 覆盖级别，路径使用 `/` |
| `custom` | 声明式规则数组 |

优先级：`by_path` > `by_file_type` > `severity_overrides` > 内置默认。`auto` 表示停止向下查找并采用规则自身默认（例如 `CJK_SPACE_MISSING` 在 docx 中为 `info`）。`off` 关闭该规则。

路径 glob 支持 `*`（不跨越 `/`）、`**`（跨越目录）、`?`。匹配不区分大小写，Windows 路径会先统一为 `/`。

### 声明式规则

共同字段：

| 字段 | 必填 | 说明 |
|---|---|---|
| `id` | 是 | 全局唯一，建议 `CUSTOM_` 前缀 |
| `type` | 是 | `pair` 或 `regex`（旧配置可省略，按字段推断） |
| `category` | 否 | 默认 `custom` |
| `severity` | 否 | 默认 `warning` |
| `message` | 否 | 空时自动生成 |
| `enabled` | 否 | 默认 `true` |
| `include_excluded` | 否 | 默认 `false`；为 `true` 时也检查代码块/URL 等屏蔽区 |
| `tests` | 否 | 正例/反例数组，供 `errator rules test` |

词对规则：

```toml
[[rules.custom]]
id = "CUSTOM_LOGIN"
type = "pair"
category = "typo"
severity = "warning"
wrong = "登陆"
right = "登录"
case_sensitive = false
window = 8
when_prev = ["账号", "系统"]
when_next = []
when_prev_regex = ["系统$"]
when_next_regex = []
message = "账号场景应使用「登录」"

[[rules.custom.tests]]
input = "系统登陆异常"
expect = ["CUSTOM_LOGIN"]

[[rules.custom.tests]]
input = "船只登陆海滩"
expect = []
```

- 英文词自动要求 ASCII 边界；中文按字符边界。
- `when_prev` / `when_next` 在前/后 `window` 个字符内任一命中即可。
- `when_prev_regex` / `when_next_regex` 全部匹配才通过。
- 所有已提供的条件必须同时满足。

正则规则：

```toml
[[rules.custom]]
id = "CUSTOM_TODO"
type = "regex"
pattern = 'TODO\((\w+)\)'
replacement = "TODO($1)"
message = "TODO 标记格式不规范"
```

`replacement` 省略时仍报告，但 `suggestion` 为空。

## `[dictionary]` / `[dict]`

后者是前者的别名。

| 字段 | 说明 |
|---|---|
| `words` | 正确项目词，用于降低误报；按原文精确匹配加入白名单 |
| `whitelist` | 永不报告的原文 |
| `ignore_words` | 永不报告的词 |
| `extra` / `confusions` | 错词 → 正确词数组，可单独指定 `severity` |
| `files` | 外部词典 TOML 文件，相对于当前配置文件；文件内不得再引用 `files` |

外部词典支持两种格式：根级 `[dict]` 表，或与 `[dictionary]` 段相同的字段：

```toml
[dict]
words = ["Errator"]
ignore_words = ["错误词"]

[[dict.confusions]]
wrong = "勘误师"
right = "勘误执行者"
severity = "info"
```

白名单只按报告原文精确比对，不会因为同一行出现某个正确项目词而压制其他问题。

## `[consistency]`

| 字段 | 默认值 | 说明 |
|---|---|---|
| `use_builtin` | `true` | 启停内置术语一致组 |
| `groups` | `[]` | 额外一致组，第一个词为推荐写法 |

只有同一文档中同时出现至少两个变体时才报告；只使用非推荐写法不报告。

## `[external.<name>]`

| 字段 | 默认值 | 说明 |
|---|---|---|
| `command` | — | 必填；不经过 shell，按引号规则拆分 |
| `timeout_ms` | `5000` | 超时后 kill |
| `enabled` | `true` | 是否执行 |
| `env` | `{}` | 额外环境变量 |
| `cwd` | 当前目录 | 工作目录，相对配置文件解析 |

协议见 [external-checkers.md](external-checkers.md)。

## `[semantic]`

| 字段 | 默认值 | 说明 |
|---|---|---|
| `mode` | `"auto"` | `auto` / `on` / `off`；CLI `--semantic` 优先 |
| `device` | `"auto"` | `auto` / `cpu` / `dml` / `coreml`；CLI `--device` 优先 |
| `allow_gpu_fallback` | `true` | GPU 初始化或推理失败时是否回退 CPU；为 `false` 时禁用可选语义层，不阻塞规则检查 |
| `model_path` | `None` | 显式模型；相对配置文件。为空时取 `~/.errator/models/` 中最新的 `.onnx` |
| `vocab_path` | `None` | BERT WordPiece 词表；默认模型同目录 `vocab.txt` |
| `tokenizer` | `"bert-base-chinese"` | v0.5 仅支持 BERT WordPiece |
| `input_names` | `["input_ids","attention_mask","token_type_ids"]` | 模型输入名，可按需只列前两个 |
| `output_name` | `None` | 输出名；为空时优先 `logits`，否则第一个输出 |
| `min_confidence` | `0.5` | 模型建议最低置信度 |
| `max_text_length` | `400` | 每个推理分块的最大字符数（内部上限 500） |

详见 [semantic.md](semantic.md)。

## 忽略库

持久化忽略项保存在 `~/.errator/ignores.json`，schema 版本 `1`。匹配键为 `rule_id + original`（原文精确匹配），不区分文件。可通过 CLI 或 TUI 管理。

## 规则集

见 [rulesets.md](rulesets.md)。
