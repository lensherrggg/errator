# 架构与扩展点

> 更完整的系统设计、数据契约与设计取舍见 [design.md](design.md)。本文是维护者速查版。

Errator 是一个本地检查编排器：输入适配器 → 文档模型 → 检查器/外部检查器/语义后端 → 统一 `Report` → 导出器/终端 UI。

## 模块边界

```text
src/
  cli.rs            参数、命令分发、退出码
  config.rs         配置模型、项目配置发现、L0/L1 设置、外部词典
  document.rs       txt/md/docx 提取、位置映射、屏蔽区
  engine.rs         检查编排、过滤、忽略、取消、目录聚合
  cancel.rs         CancellationToken
  external.rs       L2 子进程协议
  ignore.rs         持久化忽略库
  model.rs          Issue/Location/Report/SemanticInfo（schema 1.0）
  models.rs         显式模型下载
  rules/            内置与声明式规则、测试运行器
  ruleset.rs        规则集安装/加载/合并
  semantic.rs       设备选择、降级策略、后端 trait
  semantic/onnx.rs  ONNX Runtime + BERT WordPiece 实现
  report/           text/json/md/html/csv + Exporter trait
  tui.rs            文件浏览、后台检查、报告、忽略、导出
  sdk.rs 重导出     原生扩展公共类型
```

依赖方向保持单向：`rules`/`document` 不依赖 `cli`/`tui`；`engine` 依赖规则、配置、语义与外部协议；UI 只消费 `Engine` 与 `Report`。

## 检查流程

1. `cli` 解析参数、加载配置与规则集。
2. `Engine::run` 收集文件（目录递归、稳定排序）。
3. 适配器读取文件：UTF-8/UTF-16、docx ZIP/XML；构建位置映射和屏蔽区。
4. 对每条规则按路径/文件类型计算有效级别；命中 `off` 跳过。
5. 规则返回 `Issue`；engine 应用级别覆盖、白名单、持久化忽略、临时 `--ignore`、最低级别、去重与排序。
6. `[external.*]` 每文件执行，结果并入同一 issue 流。
7. 可选语义后端逐块推理，生成 `SEMANTIC_CSC`；失败按策略回退 CPU。
8. 构建 `Report`；取消时仍输出部分结果。
9. CLI 导出（文件原子替换）或 TUI 渲染；TUI 在 worker 线程执行步骤 2–8。

## 位置模型

- 文本：字节偏移 → 1-based line/column（按 Unicode scalar 计列）。
- docx：只记录 1-based paragraph/char_offset，不生成虚假行号。
- 每个内部 `Issue` 带非序列化 `offset` 用于排序，JSON 中不可见。
- 批量目录报告在 issue 上增加可选 `file` 字段；单文件 JSON 完全保持 schema 1.0。

## 可取消性

`CancellationToken` 是 `Arc<AtomicBool>`。轮询点：

- 文件之间；
- 规则之间与规则内部的主循环；
- 对外部检查器等待循环（并在取消时 kill）；
- 语义分块之间与 token 扫描。

单次 ONNX `Run` 内部不可抢占，这是运行时限制，见 [semantic.md](semantic.md)。

## GPU 后端契约

`semantic::SemanticBackend`：

```rust
fn device(&self) -> DeviceKind;
fn backend_name(&self) -> &'static str;
fn analyze(&mut self, doc: &Document, token: &CancellationToken,
           cancellation_enabled: bool, max_text_length: usize) -> Result<Vec<Issue>>;
```

- 设备选择与降级由 `semantic::prepare` 负责，后端只负责给定设备上的会话。
- DirectML/CoreML 通过 `ort` execution provider builder 注册，`error_on_failure()` 确保初始化失败可被捕获并降级。
- 后端不得修改 `Report` 结构；只返回 `Issue`。
- 新增后端无需改 CLI/TUI/导出器。

## 扩展点

| 扩展点 | 实现方式 | 稳定性 |
|---|---|---|
| 规则启停/等级 | `[rules]` | 稳定 |
| 声明式规则 | `[[rules.custom]]` | 稳定 |
| 词典 | `[dictionary]` / 外部词典 | 稳定 |
| 规则集 | `ruleset` 命令 | 稳定 |
| 外部检查器 | 子进程 JSON | 稳定 |
| 导出器 | `report::Exporter` | 稳定（Rust） |
| 原生规则 | `rules::Rule` + `sdk` | 实验 |
| 语义模型 | `[semantic]` | 实验 |
| 模板导出 | 计划中 | — |
| WASM 插件 | 计划中（v1 不实现） | — |
| 动态库插件 | **不推荐/不支持** | — |

## 为什么没有动态库插件

`cdylib` + C ABI 的 Rust 插件在版本、panic 跨边界、内存分配器和签名上风险较高。当前优先编译期扩展；未来若需要沙箱化第三方规则，优先 WASM。

## 报告稳定性

- `schema_version = "1.0"`。
- 保留字段：`rule_id`、`category`、`severity`、`location`、`original`、`suggestion`、`message`、`source`、`confidence`。
- 新增可选字段（如批量 `file`）不改变单文件契约；移除/重命名字段需要提升 schema 版本。
- 内部字段（`offset`、`semantic_warning`）不序列化。

## 性能设计

- 规则一次扫描或少量扫描，Markdown 前处理只做一次。
- 正则/词典规则多遍扫描内置词条；Windows release 下 108,000 字符实测约 1.24 秒，满足 10 万字 < 3 秒目标。
- 语义按句/标点分块，单块长度受限。
- 目录检查稳定排序，不在内存中保存整批文档，只保留 issue。

## 测试策略

- 单元：位置、屏蔽区、规则正反例、配置优先级、glob、HTML 转义、CSV、TUI 渲染。
- 集成：CLI 真实二进制 + 临时目录；docx fixture；规则集安装/选择；忽略持久化；外部检查器；取消。
- 可选：CPU ONNX toy model、DirectML 真机路径。
- Markdown golden：标题、元信息、摘要表、问题代码块。
