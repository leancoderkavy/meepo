//! Text-to-speech providers — ElevenLabs, macOS say, OpenAI TTS

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use tracing::{debug, info};

use super::{AudioConfig, TtsProvider};

/// Text-to-speech trait
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>>;
    fn name(&self) -> &str;
}

/// Create a TTS provider from config
pub fn create_tts(config: &AudioConfig) -> Result<Box<dyn TextToSpeech>> {
    match config.tts_provider {
        TtsProvider::Elevenlabs => {
            if config.elevenlabs_api_key.is_empty() {
                return Err(anyhow!("ElevenLabs API key required for ElevenLabs TTS"));
            }
            Ok(Box::new(ElevenLabsTts::new(
                config.elevenlabs_api_key.clone(),
                config.elevenlabs_voice_id.clone(),
            )))
        }
        TtsProvider::MacosSay => Ok(Box::new(MacosSayTts::new())),
        TtsProvider::OpenaiTts => {
            let api_key = if !config.openai_api_key.is_empty() {
                config.openai_api_key.clone()
            } else {
                return Err(anyhow!("OpenAI API key required for OpenAI TTS"));
            };
            Ok(Box::new(OpenAiTts::new(api_key)))
        }
    }
}

/// ElevenLabs text-to-speech
pub struct ElevenLabsTts {
    client: reqwest::Client,
    api_key: String,
    voice_id: String,
    model_id: String,
}

impl ElevenLabsTts {
    pub fn new(api_key: String, voice_id: String) -> Self {
        let voice_id = if voice_id == "default" || voice_id.is_empty() {
            "21m00Tcm4TlvDq8ikWAM".to_string() // Rachel
        } else {
            voice_id
        };
        Self {
            client: reqwest::Client::new(),
            api_key,
            voice_id,
            model_id: "eleven_monolingual_v1".to_string(),
        }
    }
}

#[async_trait]
impl TextToSpeech for ElevenLabsTts {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let truncated = if text.len() > 5000 { &text[..5000] } else { text };

        debug!("ElevenLabs TTS: synthesizing {} chars", truncated.len());

        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            self.voice_id
        );

        let body = serde_json::json!({
            "text": truncated,
            "model_id": self.model_id,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        });

        let response = self
            .client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "audio/mpeg")
            .json(&body)
            .send()
            .await
            .context("Failed to send ElevenLabs TTS request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("ElevenLabs TTS error {}: {}", status, error_text));
        }

        let bytes = response.bytes().await
            .context("Failed to read ElevenLabs TTS response")?;

        info!("ElevenLabs TTS: synthesized {} bytes of audio", bytes.len());
        Ok(bytes.to_vec())
    }

    fn name(&self) -> &str {
        "elevenlabs"
    }
}

/// macOS `say` command text-to-speech
pub struct MacosSayTts {
    voice: String,
    rate: u32,
}

impl MacosSayTts {
    pub fn new() -> Self {
        Self {
            voice: "Samantha".to_string(),
            rate: 200,
        }
    }

    pub fn with_voice(mut self, voice: String) -> Self {
        self.voice = voice;
        self
    }
}

impl Default for MacosSayTts {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TextToSpeech for MacosSayTts {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if text.is_empty() {
            return Ok(Vec::new());
        }

        debug!("macOS say: synthesizing {} chars with voice '{}'", text.len(), self.voice);

        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join(format!("meepo_tts_{}.aiff", uuid::Uuid::new_v4()));

        let output = tokio::process::Command::new("say")
            .arg("-v")
            .arg(&self.voice)
            .arg("-r")
            .arg(self.rate.to_string())
            .arg("-o")
            .arg(output_path.to_str().unwrap_or("/tmp/meepo_tts.aiff"))
            .arg(text)
            .output()
            .await
            .context("Failed to run macOS say command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Clean up temp file on error
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(anyhow!("macOS say failed: {}", stderr));
        }

        let bytes = tokio::fs::read(&output_path).await
            .context("Failed to read say output file")?;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&output_path).await;

        info!("macOS say: synthesized {} bytes of audio", bytes.len());
        Ok(bytes)
    }

    fn name(&self) -> &str {
        "macos_say"
    }
}

/// OpenAI text-to-speech
pub struct OpenAiTts {
    client: reqwest::Client,
    api_key: String,
    model: String,
    voice: String,
}

impl OpenAiTts {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
        }
    }

    pub fn with_voice(mut self, voice: String) -> Self {
        self.voice = voice;
        self
    }
}

#[async_trait]
impl TextToSpeech for OpenAiTts {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let truncated = if text.len() > 4096 { &text[..4096] } else { text };

        debug!("OpenAI TTS: synthesizing {} chars", truncated.len());

        let body = serde_json::json!({
            "model": self.model,
            "input": truncated,
            "voice": self.voice,
            "response_format": "mp3"
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .context("Failed to send OpenAI TTS request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI TTS error {}: {}", status, error_text));
        }

        let bytes = response.bytes().await
            .context("Failed to read OpenAI TTS response")?;

        info!("OpenAI TTS: synthesized {} bytes of audio", bytes.len());
        Ok(bytes.to_vec())
    }

    fn name(&self) -> &str {
        "openai_tts"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tts_macos_say() {
        let config = AudioConfig::default();
        let tts = create_tts(&config).unwrap();
        assert_eq!(tts.name(), "macos_say");
    }

    #[test]
    fn test_create_tts_elevenlabs_no_key() {
        let mut config = AudioConfig::default();
        config.tts_provider = TtsProvider::Elevenlabs;
        let result = create_tts(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_tts_elevenlabs() {
        let mut config = AudioConfig::default();
        config.tts_provider = TtsProvider::Elevenlabs;
        config.elevenlabs_api_key = "test-key".to_string();
        let tts = create_tts(&config).unwrap();
        assert_eq!(tts.name(), "elevenlabs");
    }

    #[test]
    fn test_create_tts_openai_no_key() {
        let mut config = AudioConfig::default();
        config.tts_provider = TtsProvider::OpenaiTts;
        let result = create_tts(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_tts_openai() {
        let mut config = AudioConfig::default();
        config.tts_provider = TtsProvider::OpenaiTts;
        config.openai_api_key = "sk-test".to_string();
        let tts = create_tts(&config).unwrap();
        assert_eq!(tts.name(), "openai_tts");
    }

    #[test]
    fn test_elevenlabs_default_voice() {
        let tts = ElevenLabsTts::new("key".to_string(), "default".to_string());
        assert_eq!(tts.voice_id, "21m00Tcm4TlvDq8ikWAM");
    }

    #[test]
    fn test_elevenlabs_custom_voice() {
        let tts = ElevenLabsTts::new("key".to_string(), "custom-id".to_string());
        assert_eq!(tts.voice_id, "custom-id");
    }
}
