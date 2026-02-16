//! Audio pipeline — STT, TTS, and voice activity detection
//!
//! Provides speech-to-text, text-to-speech, and voice activity detection
//! for hands-free interaction with Meepo.

pub mod stt;
pub mod tts;
pub mod vad;

use serde::{Deserialize, Serialize};

/// Audio configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub enabled: bool,
    pub stt_provider: SttProvider,
    pub tts_provider: TtsProvider,
    pub elevenlabs_api_key: String,
    pub elevenlabs_voice_id: String,
    pub openai_api_key: String,
    pub wake_word: String,
    pub wake_enabled: bool,
    pub sample_rate: u32,
    pub silence_threshold: f32,
    pub silence_duration_ms: u64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stt_provider: SttProvider::WhisperApi,
            tts_provider: TtsProvider::MacosSay,
            elevenlabs_api_key: String::new(),
            elevenlabs_voice_id: "default".to_string(),
            openai_api_key: String::new(),
            wake_word: "hey meepo".to_string(),
            wake_enabled: false,
            sample_rate: 16000,
            silence_threshold: 0.01,
            silence_duration_ms: 1500,
        }
    }
}

/// Speech-to-text provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SttProvider {
    WhisperApi,
    WhisperLocal,
}

/// Text-to-speech provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TtsProvider {
    Elevenlabs,
    MacosSay,
    OpenaiTts,
}

/// Result of a speech-to-text transcription
#[derive(Debug, Clone)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: u64,
}

/// Audio chunk for streaming
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioChunk {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.samples.len() as u64 * 1000) / (self.sample_rate as u64 * self.channels as u64)
    }

    pub fn rms_energy(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.samples.iter().map(|s| s * s).sum();
        (sum / self.samples.len() as f32).sqrt()
    }
}

/// Encode f32 PCM samples to WAV bytes (16-bit, mono)
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = num_samples * 2; // 16-bit = 2 bytes per sample
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_size as u32).to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&i16_val.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.stt_provider, SttProvider::WhisperApi);
        assert_eq!(config.tts_provider, TtsProvider::MacosSay);
        assert_eq!(config.sample_rate, 16000);
    }

    #[test]
    fn test_audio_chunk_duration() {
        let chunk = AudioChunk::new(vec![0.0; 16000], 16000);
        assert_eq!(chunk.duration_ms(), 1000);
    }

    #[test]
    fn test_audio_chunk_rms_energy() {
        let chunk = AudioChunk::new(vec![0.5, -0.5, 0.5, -0.5], 16000);
        let rms = chunk.rms_energy();
        assert!((rms - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_audio_chunk_empty() {
        let chunk = AudioChunk::new(vec![], 16000);
        assert_eq!(chunk.duration_ms(), 0);
        assert_eq!(chunk.rms_energy(), 0.0);
    }

    #[test]
    fn test_audio_chunk_zero_sample_rate() {
        let chunk = AudioChunk::new(vec![0.0; 100], 0);
        assert_eq!(chunk.duration_ms(), 0);
    }

    #[test]
    fn test_encode_wav() {
        let samples = vec![0.0f32; 100];
        let wav = encode_wav(&samples, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 200); // header + 100 samples * 2 bytes
    }

    #[test]
    fn test_encode_wav_clamps() {
        let samples = vec![2.0, -2.0]; // out of range
        let wav = encode_wav(&samples, 16000);
        // Should not panic, values clamped to [-1, 1]
        assert_eq!(wav.len(), 44 + 4);
    }
}
