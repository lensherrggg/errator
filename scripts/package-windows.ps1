#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$AllowDirty
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$dirty = & git describe --tags --always --dirty
if (-not $AllowDirty -and $dirty -match 'dirty') {
    throw "工作区有未提交修改（$dirty）。请先提交，或显式使用 -AllowDirty。"
}

$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'errator' } | Select-Object -First 1
if (-not $package) { throw '无法从 cargo metadata 读取 errator 版本。' }
$version = $package.version
$targetDir = $metadata.target_directory
$distRoot = Join-Path $root 'dist'
$bundleName = "errator-$version-windows-x86_64"
$bundleDir = Join-Path $distRoot $bundleName
$zipPath = Join-Path $distRoot "$bundleName.zip"

if (-not $SkipTests) {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt 检查失败。' }
    cargo test --all-targets
    if ($LASTEXITCODE -ne 0) { throw '测试失败。' }
    cargo test --features semantic-directml
    if ($LASTEXITCODE -ne 0) { throw 'DirectML 语义测试失败。' }
}

$env:ERRATOR_GIT_COMMIT = $dirty
cargo build --release --features semantic-directml
if ($LASTEXITCODE -ne 0) { throw 'release 构建失败。' }

if (Test-Path $bundleDir) { Remove-Item $bundleDir -Recurse -Force }
New-Item -ItemType Directory -Path $bundleDir -Force | Out-Null

$exe = Join-Path $targetDir 'release/errator.exe'
if (-not (Test-Path $exe)) { throw "找不到构建产物：$exe" }
Copy-Item $exe (Join-Path $bundleDir 'errator.exe')

# ONNX Runtime is linked statically by ort's downloaded distribution, while the
# DirectML EP ships as a separate redistributable DLL.
foreach ($name in @('DirectML.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    $candidate = Join-Path $targetDir "release/$name"
    if (Test-Path $candidate) {
        Copy-Item $candidate (Join-Path $bundleDir $name)
    }
}

Copy-Item (Join-Path $root 'README.md') $bundleDir
Copy-Item (Join-Path $root 'CHANGELOG.md') $bundleDir
Copy-Item (Join-Path $root 'LICENSE') $bundleDir
Copy-Item (Join-Path $root 'docs/THIRD_PARTY_NOTICES.md') $bundleDir

$versionText = & $exe version
if ($LASTEXITCODE -ne 0) { throw '运行打包后的 errator version 失败。' }
$versionText | Set-Content -Path (Join-Path $bundleDir 'VERSION.txt') -Encoding utf8

if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path $bundleDir -DestinationPath $zipPath -CompressionLevel Optimal
$hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
$hashFile = "$zipPath.sha256"
"$hash  $(Split-Path -Leaf $zipPath)" | Set-Content -Path $hashFile -Encoding ascii

Write-Host ''
Write-Host "Windows 包：$zipPath"
Write-Host "SHA-256：$hash"
Write-Host "校验文件：$hashFile"
Get-ChildItem $bundleDir | Format-Table Name, Length -AutoSize
