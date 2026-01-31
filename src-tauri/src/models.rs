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
    GlmMlx,
    Cloud,
}

impl ModelEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelEngine::Whisper => "whisper",
            ModelEngine::GlmMlx => "glm-mlx",
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
        engine: ModelEngine::GlmMlx,
        storage_dir: "glm",
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
