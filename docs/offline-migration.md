# 离线 / 跨机器迁移

Errator 本身不需要安装，也不需要联网即可运行规则检查、TUI 和报告导出。要迁移到无法联网的机器，只需要提前把**程序、模型和可选用户数据**复制过去。

## 需要复制什么

| 内容 | 默认位置 | 是否必需 |
|---|---|---|
| 程序目录 | `D:\tools\errator` 或版本化目录 | 必需 |
| 模型 + 词表 | `%USERPROFILE%\.errator\models\` | 可选（不用语义检查可不带） |
| 配置 | `%USERPROFILE%\.errator\config.toml` | 可选 |
| 忽略库 | `%USERPROFILE%\.errator\ignores.json` | 可选 |
| 规则集 | `%USERPROFILE%\.errator\rulesets\` | 可选 |
| 外部检查器脚本/依赖 | 配置中的 `command`/`cwd` | 可选，需单独复制 |

程序目录至少应包含：

```text
errator.exe
DirectML.dll
```

`DirectML.dll` 必须与 `errator.exe` 放在同一目录；否则 GPU 无法启用，但 CPU 规则检查仍可运行。

## 推荐：便携目录布局

在 U 盘或目标机器上建立：

```text
ErratorPortable\
  errator.exe
  DirectML.dll
  vcruntime140.dll
  vcruntime140_1.dll
  msvcp140.dll
  msvcp140_1.dll
  run-errator.cmd
  data\
    models\
      macbert4csc.onnx
      vocab.txt
      MODEL_INFO.txt
```

四个 `vcruntime` / `msvcp` DLL 是 VC++ 运行库。Windows 10/11 通常已安装，但在全新的离线机器上可能缺失；建议随包携带（app-local 放置）。

`run-errator.cmd`：

```bat
@echo off
setlocal
set "ERRATOR_HOME=%~dp0data"
"%~dp0errator.exe" %*
```

这样所有配置、忽略项、模型和规则集都保存在便携目录的 `data\` 下，不依赖用户目录，也不会写入系统。

双击 `run-errator.cmd` 启动 TUI；命令行使用：

```bat
run-errator.cmd check doc.md
run-errator.cmd devices
run-errator.cmd version
```

## 直接复制到目标机用户目录

如果不想使用便携目录，也可以：

```powershell
# 目标机上创建数据目录
New-Item -ItemType Directory -Force "$env:USERPROFILE\.errator\models"

# 从 U 盘复制
Copy-Item "E:\ErratorPortable\errator.exe" "D:\tools\errator\" -Force
Copy-Item "E:\ErratorPortable\DirectML.dll" "D:\tools\errator\" -Force
Copy-Item "E:\ErratorPortable\models\*" "$env:USERPROFILE\.errator\models\" -Force
```

然后直接运行 `D:\tools\errator\errator.exe`。默认会自动发现 `%USERPROFILE%\.errator\models\` 中的模型。

如果有配置或忽略项，也一并复制：

```powershell
Copy-Item "E:\ErratorData\config.toml" "$env:USERPROFILE\.errator\" -Force
Copy-Item "E:\ErratorData\ignores.json" "$env:USERPROFILE\.errator\" -Force
Copy-Item "E:\ErratorData\rulesets" "$env:USERPROFILE\.errator\" -Recurse -Force
```

## 从当前机器制作迁移包

在可联网机器上：

```powershell
$src = "D:\tools\errator"
$dst = "E:\ErratorPortable"

New-Item -ItemType Directory -Force "$dst\data\models" | Out-Null
Copy-Item "$src\*" $dst -Recurse -Force
Copy-Item "$env:USERPROFILE\.errator\models\macbert4csc.onnx" "$dst\data\models\" -Force
Copy-Item "$env:USERPROFILE\.errator\models\vocab.txt" "$dst\data\models\" -Force
```

再创建上面的 `run-errator.cmd`。把整个 `E:\ErratorPortable` 复制到 U 盘即可。

如果使用仓库的 [package-portable.ps1](../scripts/package-portable.ps1)，会自动完成上述步骤并生成 zip 和 SHA-256。

## 离线验证

目标机上：

```powershell
.\run-errator.cmd version
.\run-errator.cmd devices
.\run-errator.cmd check .\test.md --semantic off
.\run-errator.cmd check .\test.md --semantic on --device cpu
.\run-errator.cmd check .\test.md --semantic on --device auto
```

预期：

- `version` 显示 `git: v0.7.x`；
- `devices` 显示“本地模型：...\data\models\macbert4csc.onnx”；
- `--semantic off` 仅规则检查，不需要 ONNX；
- `--semantic on --device cpu` 使用 CPU 语义检查；
- DirectML 可用时 `--device auto` 显示 `GPU · DirectML`；不可用会回退 CPU，不影响规则检查。

## 离线机器上的限制

- `errator models download` 需要联网，离线环境不能使用；请提前复制模型。
- 外部检查器如果依赖 `python`、Node 包或网络服务，需要把对应运行时和依赖也复制并配置。
- 安装/更新 VC++ 运行库需要离线安装包；便携包已携带常用 app-local DLL。
- DirectML 需要 Windows + DirectX 12 GPU；无 GPU 时仍可 CPU 运行。
- 跨操作系统迁移（Windows → macOS/Linux）不能直接复制 exe；需要为目标平台构建对应二进制，数据和模型格式可复用。
- 模型权重由第三方提供，迁移时注意其许可证。

## 常见故障

| 现象 | 原因 | 处理 |
|---|---|---|
| 缺少 `VCRUNTIME140.dll`/`MSVCP140.dll` | 目标机无 VC++ 运行库 | 复制 app-local DLL，或离线安装 VC++ 2015-2022 x64 |
| GPU 不可用 | 缺少 `DirectML.dll`、无 DX12 或驱动过旧 | 保持 DLL 同目录；否则继续用 CPU |
| `未发现本地模型` | 模型不在 `ERRATOR_HOME\models` 或配置路径失效 | 复制模型；检查 `config.toml` 的绝对路径 |
| TUI 乱码 | 终端字体/编码问题 | 使用 Windows Terminal，UTF-8 终端 |
| 配置中的旧绝对路径失效 | 盘符/用户名变化 | 改为相对路径，或用便携 `ERRATOR_HOME` |
