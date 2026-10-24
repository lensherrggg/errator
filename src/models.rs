use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::paths;

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub url: String,
    pub name: Option<String>,
    pub sha256: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

/// Explicit, user-initiated model download. The regular `check` path never
/// touches the network. A caller-provided URL and (optionally) SHA-256 keep the
/// core distribution independent of a particular model host.
pub fn download_model(
    options: &DownloadOptions,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<DownloadResult> {
    let url = options.url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("模型 URL 必须以 http:// 或 https:// 开头");
    }
    let directory = paths::models_dir()?;
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("无法创建模型目录 {}", directory.display()))?;
    let filename = resolve_filename(options, url)?;
    let target = directory.join(&filename);
    if target.exists() && !options.force {
        bail!("模型文件已存在：{}（使用 --force 覆盖）", target.display());
    }

    let temporary = directory.join(format!("{filename}.part"));
    let response = ureq::get(url)
        .header("User-Agent", concat!("errator/", env!("CARGO_PKG_VERSION")))
        .call()
        .with_context(|| format!("模型下载失败：{url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("模型下载失败：HTTP {status}");
    }
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(&temporary)
        .with_context(|| format!("无法创建临时模型文件 {}", temporary.display()))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let download_result = (|| -> Result<()> {
        loop {
            let read = reader.read(&mut buffer).context("读取模型下载数据失败")?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .context("写入模型文件失败")?;
            hasher.update(&buffer[..read]);
            received += read as u64;
            progress(received, total);
        }
        file.flush().context("刷新模型文件失败")?;
        Ok(())
    })();

    if let Err(error) = download_result {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);

    let digest = hex_digest(hasher.finalize().as_slice());
    if let Some(expected) = &options.sha256 {
        let expected = expected.trim().to_ascii_lowercase();
        if expected != digest {
            let _ = std::fs::remove_file(&temporary);
            bail!("模型 SHA-256 校验失败：期望 {expected}，实际 {digest}");
        }
    }
    if target.exists() {
        std::fs::remove_file(&target)
            .with_context(|| format!("无法替换模型文件 {}", target.display()))?;
    }
    std::fs::rename(&temporary, &target)
        .with_context(|| format!("无法保存模型文件 {}", target.display()))?;

    Ok(DownloadResult {
        path: target,
        bytes: received,
        sha256: digest,
    })
}

fn resolve_filename(options: &DownloadOptions, url: &str) -> Result<String> {
    let candidate = options
        .name
        .clone()
        .or_else(|| {
            let without_query = url.split(['?', '#']).next().unwrap_or(url);
            without_query
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .map(percent_decode_lossy)
        })
        .unwrap_or_else(|| "model.onnx".to_owned());
    let candidate = candidate.trim().to_owned();
    if candidate.contains(['/', '\\']) || candidate == "." || candidate == ".." {
        bail!("模型文件名不合法：{candidate}");
    }
    if !candidate.to_ascii_lowercase().ends_with(".onnx") {
        bail!("模型文件名必须以 .onnx 结尾：{candidate}");
    }
    if candidate.chars().any(|character| character.is_control()) {
        bail!("模型文件名包含控制字符");
    }
    Ok(candidate)
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                result.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        result.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn list_models() -> Result<Vec<(String, u64)>> {
    let directory = paths::models_dir()?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut models = Vec::new();
    for entry in std::fs::read_dir(&directory)
        .with_context(|| format!("无法读取模型目录 {}", directory.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("onnx"))
            .unwrap_or(false)
        {
            let size = entry.metadata()?.len();
            models.push((
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                size,
            ));
        }
    }
    models.sort();
    Ok(models)
}

pub fn default_model_url() -> Option<String> {
    std::env::var("ERRATOR_MODEL_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn ensure_models_dir() -> Result<PathBuf> {
    let directory = paths::models_dir()?;
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("无法创建模型目录 {}", directory.display()))?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_filename_is_sanitized() {
        let options = DownloadOptions {
            url: "https://example.com/models/macbert%20csc.onnx?token=x".to_owned(),
            name: None,
            sha256: None,
            force: false,
        };
        assert_eq!(
            resolve_filename(&options, &options.url).unwrap(),
            "macbert csc.onnx"
        );
        let bad = DownloadOptions {
            name: Some("../escape.onnx".to_owned()),
            ..options
        };
        assert!(resolve_filename(&bad, &bad.url).is_err());
    }

    #[test]
    fn hex_digest_formats_lowercase() {
        assert_eq!(hex_digest(&[0x00, 0xab, 0xff]), "00abff");
    }
}
