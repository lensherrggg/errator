# 规则参考

规则输出遵循统一 `Issue` 结构：`rule_id`、`category`、`severity`、`location`、`original`、`suggestion`、`message`、`source`、`confidence`。规则 ID 发布后不重命名。

## 内置规则

| Rule ID | Category | 默认级别 | 说明 |
|---|---|---|---|
| `CJK_SPACE_MISSING` | spacing | warning；docx 下 info | 中文与英文/数字之间缺少空格，例如 `使用Rust开发` → `使用 Rust 开发` |
| `CJK_SPACE_EXTRA` | spacing | info | 两个汉字之间的多余 ASCII 空格 |
| `PUNCT_HALF_WIDTH` | punctuation | warning | 中文语境的半角标点，如 `,` → `，` |
| `PUNCT_FULL_WIDTH` | punctuation | info | 英文语境的成对全角标点，如 `（Rust）` 建议半角 |
| `PUNCT_DUPLICATE` | punctuation | info | 连续重复中文标点，如 `。。` |
| `TYPO_CONFUSION` | typography | warning | 内置常见错别字/成语误写词典 |
| `CHAR_DUPLICATE` | duplication | warning | 高风险汉字连续输入，如 `的的` |
| `TERM_VARIANT` | consistency | info | 同一文件内术语混用，如 `账号/帐号`；只有混用才报告 |
| `CUSTOM_DICTIONARY` | dictionary | warning | `[dictionary.extra]` / `[dictionary.confusions]` |
| `SEMANTIC_CSC` | semantic | warning | 本地 ONNX 模型给出的上下文用字建议 |
| `EXT_*`（用户定义） | 用户定义 | 用户定义 | 外部检查器返回的问题 |

### 屏蔽区

默认规则不检查以下区域：

- Markdown fenced code block、行内代码、front matter；
- Markdown 链接目标；
- URL；
- HTML 注释。

`[[rules.custom]]` 设置 `include_excluded = true` 可检查代码/URL。`PUNCT_*` 和内置规则始终跳过屏蔽区。

### 空格规则边界

- 只处理同一行内紧邻的中英文/数字；标点、Markdown 语法会切断 token。
- 中英文边界按连续 token 合并为一个问题，例如 `使用Rust开发` 一次报告并建议两处空格。
- 纯数字、英文、URL 不触发。

### 标点规则边界

- 不处理 `1,000`、`3.14`、`e.g.`、URL 或代码。
- `.` 在中文上下文句末/句中才建议全角；避免误伤英文缩写。
- 全角转半角只在两侧都是英文/数字时报告，默认 info。

### 术语一致性

内置组包括：登录/登陆、账号/帐号、其他/其它、成分/成份、笔画/笔划、报道/报导、唯一/惟一、做出/作出、林荫/林阴。仅当同文件混用时报告，推荐写法为组内第一项。

## L0：按文件/路径分级

```toml
[rules.severity_overrides]
CJK_SPACE_MISSING = "warning"

[rules.by_file_type.docx]
CJK_SPACE_MISSING = "auto"

[rules.by_path."docs/legacy/**"]
PUNCT_HALF_WIDTH = "off"
```

`off` 与 `disabled` 都关闭规则；区别是 `by_path`/`by_file_type` 仅影响匹配文件。详见 [configuration.md](configuration.md)。

## L1：自定义规则

### 词对规则

```toml
[[rules.custom]]
id = "CUSTOM_LOGIN"
type = "pair"
wrong = "登陆"
right = "登录"
when_prev = ["账号", "系统"]
```

`when_prev` / `when_next` 在窗口内做包含匹配；`*_regex` 使用 Rust regex，全部提供的条件都需满足。英文词自动加 ASCII 边界。

### 正则规则

```toml
[[rules.custom]]
id = "CUSTOM_TODO"
type = "regex"
pattern = 'TODO\((\w+)\)'
replacement = "TODO($1)"
include_excluded = false
```

### 词典

```toml
[[dictionary.confusions]]
wrong = "勘误师"
right = "勘误执行者"
severity = "info"
```

### 测试

给规则提供正例与反例，CI 可执行：

```bash
errator rules test --config errator.toml
```

每条 test 会运行所有启用的自定义规则，并比较实际命中的规则 ID 集合与 `expect`（精确相等）。

## L2：外部检查器

外部检查器通过 JSON 子进程协议报告任意规则 ID（建议 `EXT_` 前缀）。退出码非 0、超时或 JSON 非法只记警告，不阻塞其他检查。见 [external-checkers.md](external-checkers.md)。

## L3：Rust 原生规则

添加一个内置规则：

1. 在 `src/rules/` 新模块实现 `Rule`；
2. 在 `RuleRegistry::build` 注册；
3. 在 `tests/` 增加至少 3 个正例、3 个反例；
4. 更新本文件与 CHANGELOG；
5. 运行 `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。

`errator::sdk` 重导出 `Rule`、`RuleContext`、`Report`、`Exporter` 等类型。原生插件（动态库）不受支持，优先编译期集成。

## 严重级别与退出码

- `error` 问题使 `check` 退出码为 `1`（只要未被过滤/忽略）。
- `warning`、`info` 不影响退出码。
- 取消返回 `3`，运行错误返回 `2`。
- `--min-severity` 与忽略项在统计和退出码之前生效。
