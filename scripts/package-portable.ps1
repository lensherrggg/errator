#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$ModelDir = (Join-Path $env:USERPROFILE '.errator\models'),
    [switch]$SkipTests,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'errator' } | Select-Object -First 1
if (-not $package) { throw '无法从 cargo metadata 读取 errator 版本。' }
$version = $package.version
$targetDir = $metadata.target_directory
$distRoot = Join-Path $root 'dist'
$bundleName = "errator-$version-windows-x86_64-portable"
$bundleDir = Join-Path $distRoot $bundleName
$zipPath = Join-Path $distRoot "$bundleName.zip"

if (-not $SkipTests) {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt 检查失败。' }
    cargo test --features semantic-directml
    if ($LASTEXITCODE -ne 0) { throw '测试失败。' }
}

if (-not $SkipBuild) {
    $env:ERRATOR_GIT_COMMIT = & git describe --tags --always --dirty
    cargo build --release --features semantic-directml
    if ($LASTEXITCODE -ne 0) { throw 'release 构建失败。' }
}

if (Test-Path $bundleDir) { Remove-Item $bundleDir -Recurse -Force }
New-Item -ItemType Directory -Force $bundleDir | Out-Null

$exe = Join-Path $targetDir 'release/errator.exe'
if (-not (Test-Path $exe)) { throw "找不到构建产物：$exe" }
Copy-Item $exe (Join-Path $bundleDir 'errator.exe')

$requiredDlls = @('DirectML.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'msvcp140_1.dll')
foreach ($name in $requiredDlls) {
    $releaseCandidate = Join-Path $targetDir "release/$name"
    $systemCandidate = Join-Path $env:SystemRoot "System32/$name"
    $candidate = if (Test-Path $releaseCandidate) { $releaseCandidate } else { $systemCandidate }
    if (Test-Path $candidate) {
        Copy-Item $candidate (Join-Path $bundleDir $name)
    } elseif ($name -eq 'DirectML.dll') {
        throw '缺少 DirectML.dll，无法构建包含 GPU 支持的便携包。'
    } else {
        Write-Warning "缺少 VC 运行库 $name；目标机可能需要安装 VC++ 2015-2022 x64。"
    }
}

Copy-Item (Join-Path $root 'README.md') $bundleDir
Copy-Item (Join-Path $root 'CHANGELOG.md') $bundleDir
Copy-Item (Join-Path $root 'LICENSE') $bundleDir
Copy-Item (Join-Path $root 'docs') (Join-Path $bundleDir 'docs') -Recurse
Copy-Item (Join-Path $root 'docs/offline-migration.md') (Join-Path $bundleDir 'OFFLINE-MIGRATION.md')
Copy-Item (Join-Path $root 'docs/THIRD_PARTY_NOTICES.md') $bundleDir

$dataDir = Join-Path $bundleDir 'data'
$modelsDir = Join-Path $dataDir 'models'
New-Item -ItemType Directory -Force $modelsDir | Out-Null
$onnxFiles = @()
if (Test-Path $ModelDir) {
    $onnxFiles = @(Get-ChildItem -LiteralPath $ModelDir -File -Filter '*.onnx' -ErrorAction SilentlyContinue)
    foreach ($file in $onnxFiles) {
        Copy-Item $file.FullName $modelsDir
    }
    foreach ($name in @('vocab.txt', 'MODEL_INFO.txt')) {
        $candidate = Join-Path $ModelDir $name
        if (Test-Path $candidate) { Copy-Item $candidate $modelsDir }
    }
}
if ($onnxFiles.Count -eq 0) {
    Write-Warning "模型目录没有 .onnx 文件：$ModelDir；便携包仍可用于规则检查，但语义层不可用。"
} elseif (-not (Test-Path (Join-Path $modelsDir 'vocab.txt'))) {
    Write-Warning '模型中缺少 vocab.txt；语义层可能无法初始化。'
}

$launcher = @'
@echo off
setlocal
set "ERRATOR_HOME=%~dp0data"
"%~dp0errator.exe" %*
'@
$launcher | Set-Content -Path (Join-Path $bundleDir 'run-errator.cmd') -Encoding ascii

$versionText = & (Join-Path $bundleDir 'errator.exe') version
if ($LASTEXITCODE -ne 0) { throw '运行便携包内的 errator version 失败。' }
$versionText | Set-Content -Path (Join-Path $bundleDir 'VERSION.txt') -Encoding utf8

if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path $bundleDir -DestinationPath $zipPath -CompressionLevel Optimal
$hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
$hashFile = "$zipPath.sha256"
"$hash  $(Split-Path -Leaf $zipPath)" | Set-Content -Path $hashFile -Encoding ascii

Write-Host ''
Write-Host "便携离线包：$zipPath"
Write-Host "SHA-256：$hash"
Write-Host "校验文件：$hashFile"
Write-Host "启动：run-errator.cmd（数据保存在便携目录 data\\）"
Get-ChildItem $bundleDir | Format-Table Name, Length -AutoSize
