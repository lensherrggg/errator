# 规则集

规则集把一组自定义规则、词典和级别覆盖打包为可分享的 TOML。规则集与本地配置使用同一套 `[[rules.custom]]`、`[dictionary]`、`[rules]` 结构，额外增加 `[ruleset]` 元信息。

## 格式

```toml
[ruleset]
name = "academic-zh"
version = "0.1.0"
description = "中文学术论文勘误规则集"
extends = ["base"]

[[rules.custom]]
id = "RULESET_ACADEMIC_CITATION_001"
type = "regex"
category = "format"
severity = "warning"
pattern = '\[\d+\]'
replacement = "[$0]"
message = "示例规则"

[[dictionary.confusions]]
wrong = "勘误师"
right = "勘误执行者"
severity = "info"
```

- `name` 可省略，缺省使用文件名。
- `extends` 支持已安装规则集名称或显式文件路径；`base` 为保留的内置基础名，可省略。
- 循环依赖、非法名称、无效正则会在加载时失败。
- 后加载的规则覆盖先加载的同名自定义规则，并输出警告；依赖先于当前规则集加载。

## 命令

```bash
errator ruleset add ./rulesets/academic-zh.toml
errator ruleset list
errator check paper.md --ruleset academic-zh
errator rules test --ruleset academic-zh
errator ruleset remove academic-zh
```

已安装规则集保存到 `~/.errator/rulesets/<name>.toml`。也支持直接传文件路径：

```bash
errator check paper.md --ruleset ./rulesets/academic-zh.toml
```

`errator.toml` 可通过 `[general] rulesets = ["academic-zh"]` 自动加载；命令行 `--ruleset` 会追加，不会替换配置中的列表。

## 合并规则

- `rules.disabled`：并集。
- `rules.severity_overrides`、`by_file_type`、`by_path`：后加载覆盖同键并警告。
- `rules.custom`：同 ID 后加载覆盖；否则追加。
- 词典 words/ignore_words/whitelist：去重并集；错词数组追加。
- 一致性分组：去重并集。
- 外部检查器：同名后加载覆盖。

## 安装注意事项

`ruleset add` 只复制规则集 TOML 本身，不复制外部词典或脚本。若规则集使用相对路径（`dictionary.files`、外部检查器脚本），建议：

- 直接使用 `--ruleset <path>` 从原始目录加载；或
- 在规则集中使用绝对路径；或
- 把规则集做成完全自包含（只使用内联规则/词典）。

这一限制计划在后续版本通过规则集资源打包解决。
