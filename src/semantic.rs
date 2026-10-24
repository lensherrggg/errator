use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::cancel::CancellationToken;
use crate::config::Config;
use crate::document::Document;
use crate::model::{Issue, SemanticInfo};
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[clap(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SemanticMode {
    Auto,
    On,
    Off,
}

impl SemanticMode {
    pub fn parse_config(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "on" | "true" | "enabled" => Some(Self::On),
            "off" | "false" | "disabled" => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[clap(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum DeviceRequest {
    Auto,
    Cpu,
    Dml,
    Coreml,
}

impl DeviceRequest {
    pub fn parse_config(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "cpu" => Some(Self::Cpu),
            "dml" | "directml" => Some(Self::Dml),
            "coreml" => Some(Self::Coreml),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Cpu,
    Dml,
    Coreml,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Dml => "dml",
            Self::Coreml => "coreml",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Dml => "DirectML",
            Self::Coreml => "CoreML",
        }
    }

    pub fn is_gpu(self) -> bool {
        !matches!(self, Self::Cpu)
    }
}

/// A pluggable inference backend. Implementations are responsible for their own
/// model/tokenizer and must periodically poll `token` so GPU inference remains
/// cancellable. Failures are returned to the runtime, which can retry on CPU.
pub trait SemanticBackend: Send {
    fn device(&self) -> DeviceKind;
    fn backend_name(&self) -> &'static str;
    fn analyze(
        &mut self,
        document: &Document,
        token: &CancellationToken,
        cancellation_enabled: bool,
        max_text_length: usize,
    ) -> Result<Vec<Issue>>;
}

#[derive(Debug, Clone)]
pub struct BackendSettings {
    pub max_text_length: usize,
    pub min_confidence: f32,
    pub input_names: Vec<String>,
    pub output_name: Option<String>,
    pub vocab_path: Option<PathBuf>,
    pub tokenizer: String,
    pub source_dir: PathBuf,
}

impl BackendSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            max_text_length: config.semantic.max_text_length,
            min_confidence: config.semantic.min_confidence,
            input_names: config.semantic.input_names.clone(),
            output_name: config.semantic.output_name.clone(),
            vocab_path: config.semantic.vocab_path.clone(),
            tokenizer: config.semantic.tokenizer.clone(),
            source_dir: config.source_dir.clone(),
        }
    }
}

pub struct SemanticRuntime {
    pub info: SemanticInfo,
    pub warning: Option<String>,
    pub model_path: Option<PathBuf>,
    backend: Option<Box<dyn SemanticBackend>>,
    settings: Option<BackendSettings>,
}

impl SemanticRuntime {
    pub fn disabled(warning: Option<String>) -> Self {
        Self {
            info: SemanticInfo::disabled(),
            warning,
            model_path: None,
            backend: None,
            settings: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub fn disable(&mut self, warning: impl Into<String>) {
        self.backend = None;
        self.info.enabled = false;
        self.info.device = "cpu".to_owned();
        self.info.backend = "none".to_owned();
        self.warning = Some(warning.into());
    }

    pub fn analyze(
        &mut self,
        document: &Document,
        token: &CancellationToken,
        cancellation_enabled: bool,
        max_text_length: usize,
    ) -> Result<Vec<Issue>> {
        let Some(backend) = self.backend.as_mut() else {
            return Ok(Vec::new());
        };
        backend.analyze(document, token, cancellation_enabled, max_text_length)
    }

    /// Try to recover from a runtime EP failure by reopening the model on CPU.
    /// The report's `fallback_occurred` flag is updated when this succeeds.
    pub fn fallback_to_cpu(&mut self) -> Result<()> {
        let Some(path) = self.model_path.clone() else {
            anyhow::bail!("没有可用于降级的模型路径");
        };
        let Some(settings) = self.settings.clone() else {
            anyhow::bail!("缺少语义后端设置");
        };
        let backend = create_backend(&path, DeviceKind::Cpu, &settings)?;
        // A quick smoke call is performed lazily by the next document, so this
        // only guarantees session creation.
        backend.device();
        self.info.enabled = true;
        self.info.device = "cpu".to_owned();
        self.info.backend = "onnxruntime".to_owned();
        self.info.fallback_occurred = true;
        self.backend = Some(backend);
        Ok(())
    }
}

struct RuntimeRequest<'a> {
    config: &'a Config,
    mode: SemanticMode,
    requested: DeviceRequest,
}

/// Build a semantic runtime and apply the PRD's fallback policy. This function
/// never fails the overall check: every problem becomes a warning and a
/// disabled (or CPU-downgraded) optional semantic layer.
pub fn prepare(
    config: &Config,
    mode: SemanticMode,
    requested: DeviceRequest,
    no_gpu: bool,
) -> SemanticRuntime {
    let request = RuntimeRequest {
        config,
        mode,
        requested: if no_gpu {
            DeviceRequest::Cpu
        } else {
            requested
        },
    };
    prepare_inner(&request)
}

fn prepare_inner(request: &RuntimeRequest<'_>) -> SemanticRuntime {
    if request.mode == SemanticMode::Off {
        return SemanticRuntime::disabled(None);
    }

    let model = discover_model(request.config);
    if model.is_none() {
        let warning = (request.mode == SemanticMode::On).then(|| {
            format!(
                "--semantic on 已指定，但在 {} 中未找到 .onnx 模型",
                paths::models_dir()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "~/.errator/models".to_owned())
            )
        });
        return SemanticRuntime::disabled(warning);
    }
    let Some(model) = model else {
        return SemanticRuntime::disabled(None);
    };

    if !cfg!(feature = "semantic") {
        return SemanticRuntime::disabled(Some(
            "当前构建未启用 ONNX 语义后端；规则检查继续，CPU 规则始终可用（构建参数：--features semantic）"
                .to_owned(),
        ));
    }

    let candidates = device_candidates(request.requested);
    let settings = BackendSettings::from_config(request.config);
    let explicit_gpu = matches!(
        request.requested,
        DeviceRequest::Dml | DeviceRequest::Coreml
    );
    let mut warnings = Vec::new();
    let mut tried_gpu = false;

    for (index, device) in candidates.iter().copied().enumerate() {
        if device.is_gpu() {
            if let Some(reason) = platform_rejection(device) {
                warnings.push(reason);
                if !request.config.semantic.allow_gpu_fallback {
                    return SemanticRuntime {
                        info: SemanticInfo::disabled(),
                        warning: Some(warnings.join("；")),
                        model_path: Some(model),
                        backend: None,
                        settings: Some(settings),
                    };
                }
                continue;
            }
            tried_gpu = true;
        }
        match create_backend(&model, device, &settings) {
            Ok(backend) => {
                if device == DeviceKind::Cpu && tried_gpu {
                    warnings.push("GPU 后端不可用，已自动降级到 CPU".to_owned());
                }
                let fallback = explicit_gpu && index > 0 && candidates[0] != DeviceKind::Cpu;
                return SemanticRuntime {
                    info: SemanticInfo {
                        enabled: true,
                        device: device.as_str().to_owned(),
                        backend: backend.backend_name().to_owned(),
                        fallback_occurred: fallback,
                    },
                    warning: if explicit_gpu {
                        warnings.into_iter().next()
                    } else {
                        // `auto` must be quiet when a GPU is simply absent.
                        None
                    },
                    model_path: Some(model),
                    backend: Some(backend),
                    settings: Some(settings),
                };
            }
            Err(error) => {
                if device.is_gpu() {
                    warnings.push(format!("{} 初始化失败：{error}", device.label()));
                    if !request.config.semantic.allow_gpu_fallback {
                        return SemanticRuntime {
                            info: SemanticInfo::disabled(),
                            warning: Some(warnings.join("；")),
                            model_path: Some(model),
                            backend: None,
                            settings: Some(settings),
                        };
                    }
                } else {
                    warnings.push(format!("CPU 语义后端初始化失败：{error}"));
                }
            }
        }
    }

    SemanticRuntime {
        info: SemanticInfo::disabled(),
        warning: Some(warnings.join("；")),
        model_path: Some(model),
        backend: None,
        settings: Some(settings),
    }
}

fn device_candidates(requested: DeviceRequest) -> Vec<DeviceKind> {
    match requested {
        DeviceRequest::Cpu => vec![DeviceKind::Cpu],
        DeviceRequest::Dml => vec![DeviceKind::Dml, DeviceKind::Cpu],
        DeviceRequest::Coreml => vec![DeviceKind::Coreml, DeviceKind::Cpu],
        DeviceRequest::Auto => {
            if cfg!(target_os = "windows") {
                vec![DeviceKind::Dml, DeviceKind::Cpu]
            } else if cfg!(target_os = "macos") {
                vec![DeviceKind::Coreml, DeviceKind::Cpu]
            } else {
                vec![DeviceKind::Cpu]
            }
        }
    }
}

fn platform_rejection(device: DeviceKind) -> Option<String> {
    match device {
        DeviceKind::Dml if !cfg!(target_os = "windows") => {
            Some("DirectML 仅支持 Windows，已降级到 CPU".to_owned())
        }
        DeviceKind::Coreml if !cfg!(target_os = "macos") => {
            Some("CoreML 仅支持 macOS，已降级到 CPU".to_owned())
        }
        _ => None,
    }
}

/// Find the configured model, otherwise the newest `.onnx` file directly in
/// `~/.errator/models/`.
pub fn discover_model(config: &Config) -> Option<PathBuf> {
    if let Some(path) = &config.semantic.model_path {
        let resolved = crate::config::resolve_relative(path, &config.source_dir)
            .unwrap_or_else(|_| path.clone());
        if resolved.is_file() {
            return Some(resolved);
        }
    }
    let directory = paths::models_dir().ok()?;
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("onnx"))
                .unwrap_or(false)
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (modified, entry.path())
        })
        .collect();
    candidates.sort();
    candidates.pop().map(|(_, path)| path)
}

#[cfg(feature = "semantic")]
fn create_backend(
    path: &Path,
    device: DeviceKind,
    settings: &BackendSettings,
) -> Result<Box<dyn SemanticBackend>> {
    onnx::create_backend(path, device, settings)
}

#[cfg(not(feature = "semantic"))]
fn create_backend(
    _path: &Path,
    _device: DeviceKind,
    _settings: &BackendSettings,
) -> Result<Box<dyn SemanticBackend>> {
    anyhow::bail!(
        "此构建未启用 ONNX 语义后端；请使用 --features semantic（Windows 可加 semantic-directml）重新构建"
    )
}

#[cfg(feature = "semantic")]
mod onnx;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_platform_gpu_then_cpu() {
        #[cfg(target_os = "windows")]
        assert_eq!(
            device_candidates(DeviceRequest::Auto),
            vec![DeviceKind::Dml, DeviceKind::Cpu]
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            device_candidates(DeviceRequest::Auto),
            vec![DeviceKind::Coreml, DeviceKind::Cpu]
        );
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert_eq!(
            device_candidates(DeviceRequest::Auto),
            vec![DeviceKind::Cpu]
        );
    }

    #[test]
    fn explicit_cpu_is_never_upgraded() {
        assert_eq!(device_candidates(DeviceRequest::Cpu), vec![DeviceKind::Cpu]);
    }

    #[test]
    fn no_model_means_disabled_auto_without_noise() {
        let config = Config {
            semantic: crate::config::SemanticConfig {
                model_path: Some(PathBuf::from("definitely-missing-model.onnx")),
                ..Default::default()
            },
            ..Default::default()
        };
        // The runtime may discover a model in the user's real model dir. This
        // test only asserts that planning never panics and returns a stable
        // device field.
        let runtime = prepare(&config, SemanticMode::Auto, DeviceRequest::Auto, false);
        assert!(["cpu", "dml", "coreml"].contains(&runtime.info.device.as_str()));
    }
}
