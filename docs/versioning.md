# 版本管理

## 版本来源

- 应用版本唯一来源：`Cargo.toml` 的 `[package] version`。
- 报告数据契约：`model::SCHEMA_VERSION`（当前 `1.0`），与应用版本解耦。
- 构建时通过 `build.rs` 嵌入 `git describe --tags --always --dirty`，供 `errator version --json` 与问题排查使用。源码包/无 Git 环境显示 `unknown`。
- 忽略库 schema：`ignore::IGNORE_SCHEMA_VERSION`（当前 `1`）。
- 外部检查器协议：请求字段 `protocol_version = "1.0"`。

## 语义化版本

- `PATCH`：缺陷修复、规则误报修正、文档；不改变配置/报告契约。
- `MINOR`：向后兼容的新规则、新配置字段、新命令、新导出格式。
- `MAJOR`：破坏性 CLI/配置/报告结构变更。

规则 ID 一经发布不重命名；废弃规则保留 ID 并标记 deprecated。报告新增可选字段属于 MINOR；删除、重命名或改变语义需要提升 `schema_version`（必要时发 MAJOR）。

## 发布流程

1. 更新 `CHANGELOG.md`，把 `[Unreleased]` 改为目标版本与日期。
2. 确认 `Cargo.toml` 版本一致。
3. 执行门禁：
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test --all-targets
   cargo test --features semantic
   ```
   Windows 另外运行 `cargo test --features semantic-directml` 与 `scripts/package-windows.ps1`。
4. 提交 `chore(release): vX.Y.Z`。
5. 创建 annotated tag：
   ```bash
   git tag -a vX.Y.Z -m "Errator vX.Y.Z"
   git push origin main --tags
   ```
6. 发布 Windows 包（zip + `.sha256`），附 `CHANGELOG` 摘要。
7. 开始下一轮开发时，在 CHANGELOG 加回 `[Unreleased]`。

## Git 约定

- 主分支 `main`。
- 提交信息采用 Conventional Commits 风格：`feat:`、`fix:`、`docs:`、`test:`、`refactor:`、`chore:`。
- 版本标签为 annotated tag，格式 `vMAJOR.MINOR.PATCH`。
- 不提交构建产物、临时目录、用户本地 spec 文件（见 `.gitignore`）。
- `Cargo.lock` 对应用（二进制）提交，保证可复现构建。

## Windows 分发内容

`scripts/package-windows.ps1` 生成：

```text
dist/errator-0.7.2-windows-x86_64/
  errator.exe
  DirectML.dll          # 仅在启用 semantic-directml 且构建产物存在时
  README.md
  CHANGELOG.md
  LICENSE
  THIRD_PARTY_NOTICES.md
  docs/                 # 设计、配置、规则、协议、模型与版本说明
  VERSION.txt
errator-0.7.2-windows-x86_64.zip
errator-0.7.2-windows-x86_64.zip.sha256
```

包内 `VERSION.txt` 由 `errator version` 生成，包含 Git 描述、平台和 schema 版本。
