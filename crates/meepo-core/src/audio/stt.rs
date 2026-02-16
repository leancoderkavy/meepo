//! Speech-to-text providers — Whisper API and local whisper.cpp

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use tracing::{debug, info};

use super::{AudioChunk, AudioConfig, SttProvider, Transcription, encode_wav};

/// Speech-to-text trait
#[async_trait]
pub trait SpeechToText: Send + Sync {
    async fn transcribe(&self, audio: &AudioChunk) -> Result<Transcription>;
    fn name(&self) -> &str;
}

/// Create an STT provider from config
pub fn create_stt(config: &AudioConfig) -> Result<Box<dyn SpeechToText>> {
    match config.stt_provider {
        SttProvider::WhisperApi => {
            let api_key = if !config.openai_api_key.is_empty() {
                config.openai_api_key.clone()
            } else {
                return Err(anyhow!("OpenAI API key required for Whisper API STT"));
            };
            Ok(Box::new(WhisperApiStt::new(api_key)))
        }
        SttProvider::WhisperLocal => {
            Ok(Box::new(WhisperLocalStt::new()))
        }
    }
}

/// OpenAI Whisper API speech-to-text
pub struct WhisperApiStt {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl WhisperApiStt {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: "https://api.openai.com".to_string(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }
}

#[async_trait]
impl SpeechToText for WhisperApiStt {
    async fn transcribe(&self, audio: &AudioChunk) -> Result<Transcription> {
        let wav_bytes = encode_wav(&audio.samples, audio.sample_rate);
        let duration_ms = audio.duration_ms();

        debug!("Whisper API: transcribing {} ms of audio ({} bytes WAV)", duration_ms, wav_bytes.len());

        let part = reqwest::multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", "whisper-1")
            .text("response_format", "json");

        let url = format!("{}/v1/audio/transcriptions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .context("Failed to send Whisper API request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Whisper API error {}: {}", status, error_text));
        }

        let body: serde_json::Value = response.json().await
            .context("Failed to parse Whisper API response")?;

        let text = body["text"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        let language = body["language"].as_str().map(|s| s.to_string());

        info!("Whisper API: transcribed {} chars", text.len());

        Ok(Transcription {
            text,
            language,
            duration_ms,
        })
    }

    fn name(&self) -> &str {
        "whisper_api"
    }
}

/// Local whisper.cpp speech-to-text (stub — requires whisper-rs crate)
pub struct WhisperLocalStt;

impl WhisperLocalStt {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WhisperLocalStt {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SpeechToText for WhisperLocalStt {
    async fn transcribe(&self, audio: &AudioChunk) -> Result<Transcription> {
        // Local whisper.cpp integration would go here.
        // For now, fall back to a stub that returns an error directing the user
        // to use the API provider or install whisper-rs.
        let duration_ms = audio.duration_ms();
        debug!("WhisperLocal: {} ms audio (stub — not yet implemented)", duration_ms);

        Err(anyhow!(
            "Local Whisper STT is not yet compiled in. \
             Set stt_provider = \"whisper_api\" in config, or build with --features whisper-local"
        ))
    }

    fn name(&self) -> &str {
        "whisper_local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_stt_whisper_api() {
        let mut config = AudioConfig::default();
        config.openai_api_key = "sk-test".to_string();
        let stt = create_stt(&config).unwrap();
        assert_eq!(stt.name(), "whisper_api");
    }

    #[test]
    fn test_create_stt_whisper_api_no_key() {
        let config = AudioConfig::default();
        let result = create_stt(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_stt_whisper_local() {
        let mut config = AudioConfig::default();
        config.stt_provider = SttProvider::WhisperLocal;
        let stt = create_stt(&config).unwrap();
        assert_eq!(stt.name(), "whisper_local");
    }

    #[tokio::test]
    async fn test_whisper_local_stub() {
        let stt = WhisperLocalStt::new();
        let chunk = AudioChunk::new(vec![0.0; 16000], 16000);
        let result = stt.transcribe(&chunk).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not yet compiled"));
    }
}
