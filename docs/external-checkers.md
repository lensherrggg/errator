# 外部检查器协议（L2）

外部检查器是用户显式配置的任意语言子进程。Errator 通过 stdin 发送 UTF-8 JSON 请求，从 stdout 读取 JSON 结果；日志必须写 stderr。默认协议版本 `1.0`。

## 配置

```toml
[external.terminology]
command = "python checkers/terminology.py --project docs"
timeout_ms = 5000
enabled = true
cwd = "."
[external.terminology.env]
PROJECT = "docs"
```

- `command` 不经过 shell，按单/双引号规则拆分；支持带空格路径。
- `cwd` 缺省为启动 Errator 的目录；相对路径相对于配置文件所在目录解析。
- 超时默认 5000ms；超时会终止进程并记录警告。
- 退出码非 `0`、stdout 非法 JSON、超时均只记录警告，不影响内置规则与最终退出的问题判定。
- 每个文档执行一次；多个检查器按名称顺序运行。

## 请求（Errator → 检查器，stdin）

```json
{
  "protocol_version": "1.0",
  "source": "docs/example.md",
  "kind": "text",
  "text": "全文内容……",
  "segments": [
    { "line": 1, "column": 1, "text": "第一行" },
    { "line": 2, "column": 1, "text": "第二行" }
  ]
}
```

- `kind` 为 `text`（`.txt` 与 `.md`）或 `docx`。
- `text` 是完整提取文本。
- 文本 segments 使用 `line`/`column`；docx segments 使用 `paragraph`/`char_offset`：
  ```json
  { "paragraph": 3, "char_offset": 1, "text": "第三段" }
  ```
- `line`、`column`、`paragraph`、`char_offset` 均为 1-based。
- 为兼容 Windows 上使用系统代码页读取 stdin 的脚本，请求 JSON 中的非 ASCII 字符统一转义为 `\uXXXX`，字节流本身是纯 ASCII，仍是合法 UTF-8 JSON。

## 响应（检查器 → Errator，stdout）

```json
{
  "issues": [
    {
      "rule_id": "EXT_TERM_001",
      "category": "terminology",
      "severity": "warning",
      "location": { "kind": "text", "line": 3, "column": 8 },
      "original": "API 网管",
      "suggestion": "API 网关",
      "message": "术语错误",
      "confidence": 0.95
    }
  ]
}
```

- 只需输出 `{"issues": [...]}`；没有问题时输出 `{"issues": []}`。
- `rule_id` 与 `severity` 必填。`severity` 为 `error` / `warning` / `info`。
- `location.kind` 使用 `text` 或 `docx`。text 需要 `line`/`column`；docx 需要 `paragraph`/`char_offset`。
- `category` 缺省 `external`；`suggestion`/`message` 可为空；`confidence` 缺省 `1.0`，超出范围会被截断。
- Errator 会根据位置补全上下文，并在目录批量报告中自动补 `file` 字段。
- 外部 issue 的 `source` 固定记为 `rule`（协议不扩展稳定 Source 枚举）。
- 用户可用 L0 配置按 `rule_id` 覆盖外部规则的级别，例如：
  ```toml
  [rules.severity_overrides]
  EXT_TERM_001 = "error"
  ```

## 环境与安全

- 不继承完整环境变量：只传递 `PATH`；Windows 额外传 `SystemRoot`、`PATHEXT`、`COMSPEC`；再加上配置的 `env`。
- 为跨平台确定性，额外设置 `PYTHONUTF8=1` 与 `PYTHONIOENCODING=utf-8`。非 Python 检查器可忽略。
- 不提供网络或文件沙箱。检查器以当前用户权限运行；只配置可信脚本。
- TUI/CLI 取消时会请求 kill 子进程。Windows 上 `kill` 不会自动终止子进程树，检查器自身应避免派生长时间子进程。
- 协议只读 stdin/stdout，不支持修改源文件。

## 示例：Python

以下示例针对 `kind = "text"` 的输入；docx 需要从 `paragraph` / `char_offset` 构造 `{"kind":"docx","paragraph":...,"char_offset":...}`。

```python
#!/usr/bin/env python3
import json, sys

request = json.load(sys.stdin)
issues = []
for segment in request["segments"]:
    if "API 网管" in segment["text"]:
        index = segment["text"].index("API 网管")
        issues.append({
            "rule_id": "EXT_TERM_001",
            "category": "terminology",
            "severity": "warning",
            "location": {
                "kind": request["kind"],
                "line": segment.get("line", 1),
                "column": segment.get("column", 1) + index,
            },
            "original": "API 网管",
            "suggestion": "API 网关",
            "message": "术语错误",
            "confidence": 0.95,
        })
json.dump({"issues": issues}, sys.stdout, ensure_ascii=False)
```

在 Windows 上，Errator 已设置 Python UTF-8 环境变量；若单独运行脚本调试，请自行设置 `PYTHONUTF8=1`。

## 调试建议

1. 将日志写 stderr，不要混入 stdout。
2. 用 `timeout_ms` 控制慢检查器。
3. 在 CI 中先单独测试脚本的 stdin/stdout，再加入 `[external.*]`。
4. 退出码 `0` 但 stdout 为空会被视为失败并记录警告。
