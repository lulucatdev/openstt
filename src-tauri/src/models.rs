use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub size: String,
    pub description: String,
    pub download_url: String,
    pub downloaded: bool,
    pub local_path: Option<String>,
    pub engine: String,
    pub provider: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModelEngine {
    Whisper,
    Mlx,
    Cloud,
}

impl ModelEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelEngine::Whisper => "whisper",
            ModelEngine::Mlx => "mlx",
            ModelEngine::Cloud => "cloud",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    BigModel,
    ElevenLabs,
}

impl CloudProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            CloudProvider::BigModel => "bigmodel",
            CloudProvider::ElevenLabs => "elevenlabs",
        }
    }
}

#[derive(Clone, Copy)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub size: &'static str,
    pub description: &'static str,
    pub filename: &'static str,
    pub download_url: &'static str,
    pub engine: ModelEngine,
    pub storage_dir: &'static str,
    pub remote_model: Option<&'static str>,
    pub provider: Option<CloudProvider>,
}

const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "large-v3-turbo",
        name: "Large V3 Turbo",
        size: "1.6GB",
        description: "Best quality, optimized speed",
        filename: "ggml-large-v3-turbo.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "large-v3",
        name: "Large V3",
        size: "3.1GB",
        description: "Highest accuracy",
        filename: "ggml-large-v3.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "medium",
        name: "Medium",
        size: "1.5GB",
        description: "Balanced quality and speed",
        filename: "ggml-medium.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "small",
        name: "Small",
        size: "466MB",
        description: "Fast with good accuracy",
        filename: "ggml-small.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "base",
        name: "Base",
        size: "142MB",
        description: "Fast, moderate accuracy",
        filename: "ggml-base.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "tiny",
        name: "Tiny",
        size: "75MB",
        description: "Fastest, basic accuracy",
        filename: "ggml-tiny.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        engine: ModelEngine::Whisper,
        storage_dir: "whisper",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "glm-asr-nano-4bit",
        name: "GLM-ASR Nano (MLX 4-bit)",
        size: "~1.5GB",
        description: "Apple Silicon MLX sidecar",
        filename: "glm-asr-nano-4bit.ready",
        download_url: "mlx-community/GLM-ASR-Nano-2512-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "glm-asr-nano-8bit",
        name: "GLM-ASR Nano (MLX 8-bit)",
        size: "~2.5GB",
        description: "Apple Silicon MLX sidecar",
        filename: "glm-asr-nano-8bit.ready",
        download_url: "mlx-community/GLM-ASR-Nano-2512-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "qwen3-asr-0.6b-4bit",
        name: "Qwen3-ASR 0.6B (MLX 4-bit)",
        size: "~400MB",
        description: "Fast multilingual ASR, 52 languages",
        filename: "qwen3-asr-0.6b-4bit.ready",
        download_url: "mlx-community/Qwen3-ASR-0.6B-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "qwen3-asr-0.6b-8bit",
        name: "Qwen3-ASR 0.6B (MLX 8-bit)",
        size: "~700MB",
        description: "Fast multilingual ASR, 52 languages",
        filename: "qwen3-asr-0.6b-8bit.ready",
        download_url: "mlx-community/Qwen3-ASR-0.6B-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "qwen3-asr-1.7b-4bit",
        name: "Qwen3-ASR 1.7B (MLX 4-bit)",
        size: "~1.2GB",
        description: "Best open-source ASR, 52 languages",
        filename: "qwen3-asr-1.7b-4bit.ready",
        download_url: "mlx-community/Qwen3-ASR-1.7B-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "qwen3-asr-1.7b-8bit",
        name: "Qwen3-ASR 1.7B (MLX 8-bit)",
        size: "~2GB",
        description: "Best open-source ASR, 52 languages",
        filename: "qwen3-asr-1.7b-8bit.ready",
        download_url: "mlx-community/Qwen3-ASR-1.7B-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-tiny-4bit",
        name: "Whisper Tiny (MLX 4-bit)",
        size: "~22MB",
        description: "Fastest, basic accuracy",
        filename: "whisper-tiny-4bit.ready",
        download_url: "mlx-community/whisper-tiny-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-tiny-8bit",
        name: "Whisper Tiny (MLX 8-bit)",
        size: "~40MB",
        description: "Fastest, basic accuracy",
        filename: "whisper-tiny-8bit.ready",
        download_url: "mlx-community/whisper-tiny-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-base-4bit",
        name: "Whisper Base (MLX 4-bit)",
        size: "~42MB",
        description: "Fast, moderate accuracy",
        filename: "whisper-base-4bit.ready",
        download_url: "mlx-community/whisper-base-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-base-8bit",
        name: "Whisper Base (MLX 8-bit)",
        size: "~78MB",
        description: "Fast, moderate accuracy",
        filename: "whisper-base-8bit.ready",
        download_url: "mlx-community/whisper-base-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-small-4bit",
        name: "Whisper Small (MLX 4-bit)",
        size: "~139MB",
        description: "Fast with good accuracy",
        filename: "whisper-small-4bit.ready",
        download_url: "mlx-community/whisper-small-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-small-8bit",
        name: "Whisper Small (MLX 8-bit)",
        size: "~258MB",
        description: "Fast with good accuracy",
        filename: "whisper-small-8bit.ready",
        download_url: "mlx-community/whisper-small-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-medium-4bit",
        name: "Whisper Medium (MLX 4-bit)",
        size: "~436MB",
        description: "Balanced quality and speed",
        filename: "whisper-medium-4bit.ready",
        download_url: "mlx-community/whisper-medium-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-medium-8bit",
        name: "Whisper Medium (MLX 8-bit)",
        size: "~830MB",
        description: "Balanced quality and speed",
        filename: "whisper-medium-8bit.ready",
        download_url: "mlx-community/whisper-medium-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-large-v3-4bit",
        name: "Whisper Large V3 (MLX 4-bit)",
        size: "~878MB",
        description: "Highest accuracy",
        filename: "whisper-large-v3-4bit.ready",
        download_url: "mlx-community/whisper-large-v3-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-large-v3-8bit",
        name: "Whisper Large V3 (MLX 8-bit)",
        size: "~1.6GB",
        description: "Highest accuracy",
        filename: "whisper-large-v3-8bit.ready",
        download_url: "mlx-community/whisper-large-v3-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-large-v3-turbo-4bit",
        name: "Whisper Large V3 Turbo (MLX 4-bit)",
        size: "~463MB",
        description: "Best quality, optimized speed",
        filename: "whisper-large-v3-turbo-4bit.ready",
        download_url: "mlx-community/whisper-large-v3-turbo-4bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "whisper-large-v3-turbo-8bit",
        name: "Whisper Large V3 Turbo (MLX 8-bit)",
        size: "~864MB",
        description: "Best quality, optimized speed",
        filename: "whisper-large-v3-turbo-8bit.ready",
        download_url: "mlx-community/whisper-large-v3-turbo-8bit",
        engine: ModelEngine::Mlx,
        storage_dir: "mlx",
        remote_model: None,
        provider: None,
    },
    CatalogEntry {
        id: "glm-asr-2512",
        name: "GLM-ASR-2512 (Cloud)",
        size: "Cloud",
        description: "Zhipu GLM-ASR-2512 API",
        filename: "glm-asr-2512.cloud",
        download_url: "",
        engine: ModelEngine::Cloud,
        storage_dir: "cloud",
        remote_model: Some("glm-asr-2512"),
        provider: Some(CloudProvider::BigModel),
    },
    CatalogEntry {
        id: "elevenlabs-scribe-v2",
        name: "ElevenLabs Scribe v2",
        size: "Cloud",
        description: "ElevenLabs speech-to-text",
        filename: "elevenlabs-scribe-v2.cloud",
        download_url: "https://api.elevenlabs.io/v1/speech-to-text",
        engine: ModelEngine::Cloud,
        storage_dir: "cloud",
        remote_model: Some("scribe_v2"),
        provider: Some(CloudProvider::ElevenLabs),
    },
];

fn storage_path(models_root: &Path, entry: CatalogEntry) -> PathBuf {
    models_root.join(entry.storage_dir).join(entry.filename)
}

pub fn list_models(models_dir: &Path) -> Vec<ModelInfo> {
    CATALOG
        .iter()
        .map(|entry| {
            let path = storage_path(models_dir, *entry);
            let downloaded = if entry.engine == ModelEngine::Cloud {
                true
            } else {
                path.exists()
            };
            ModelInfo {
                id: entry.id.to_string(),
                name: entry.name.to_string(),
                size: entry.size.to_string(),
                description: entry.description.to_string(),
                download_url: entry.download_url.to_string(),
                downloaded,
                local_path: if entry.engine == ModelEngine::Cloud {
                    None
                } else {
                    downloaded.then(|| path.to_string_lossy().to_string())
                },
                engine: entry.engine.as_str().to_string(),
                provider: entry.provider.map(|provider| provider.as_str().to_string()),
            }
        })
        .collect()
}

pub fn model_entry(model_id: &str) -> Option<CatalogEntry> {
    CATALOG.iter().copied().find(|entry| entry.id == model_id)
}

pub fn model_path(models_dir: &Path, model_id: &str) -> Option<PathBuf> {
    let entry = model_entry(model_id)?;
    if entry.engine == ModelEngine::Cloud {
        return None;
    }
    Some(storage_path(models_dir, entry))
}

pub fn cloud_model_name(model_id: &str) -> Option<&'static str> {
    model_entry(model_id).and_then(|entry| entry.remote_model)
}

pub fn cloud_provider(model_id: &str) -> Option<CloudProvider> {
    model_entry(model_id).and_then(|entry| entry.provider)
}
