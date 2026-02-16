//! Voice activity detection — energy-based VAD for speech start/end detection

use tracing::debug;

use super::AudioChunk;

/// Voice activity detection state
#[derive(Debug, Clone, PartialEq)]
pub enum VadState {
    Silence,
    Speaking,
}

/// Simple energy-based voice activity detector
pub struct EnergyVad {
    threshold: f32,
    silence_duration_ms: u64,
    state: VadState,
    silence_accumulated_ms: u64,
    speech_accumulated_ms: u64,
    min_speech_ms: u64,
}

impl EnergyVad {
    pub fn new(threshold: f32, silence_duration_ms: u64) -> Self {
        Self {
            threshold,
            silence_duration_ms,
            state: VadState::Silence,
            silence_accumulated_ms: 0,
            speech_accumulated_ms: 0,
            min_speech_ms: 300, // minimum 300ms of speech to trigger
        }
    }

    /// Process an audio chunk and return the new state.
    /// Returns `Some(VadState)` if the state changed, `None` if unchanged.
    pub fn process(&mut self, chunk: &AudioChunk) -> Option<VadState> {
        let energy = chunk.rms_energy();
        let chunk_duration = chunk.duration_ms();
        let is_speech = energy > self.threshold;

        match self.state {
            VadState::Silence => {
                if is_speech {
                    self.speech_accumulated_ms += chunk_duration;
                    if self.speech_accumulated_ms >= self.min_speech_ms {
                        self.state = VadState::Speaking;
                        self.silence_accumulated_ms = 0;
                        debug!("VAD: speech detected (energy={:.4}, threshold={:.4})", energy, self.threshold);
                        return Some(VadState::Speaking);
                    }
                } else {
                    self.speech_accumulated_ms = 0;
                }
            }
            VadState::Speaking => {
                if !is_speech {
                    self.silence_accumulated_ms += chunk_duration;
                    if self.silence_accumulated_ms >= self.silence_duration_ms {
                        self.state = VadState::Silence;
                        self.speech_accumulated_ms = 0;
                        debug!(
                            "VAD: silence detected after {}ms (threshold={}ms)",
                            self.silence_accumulated_ms, self.silence_duration_ms
                        );
                        return Some(VadState::Silence);
                    }
                } else {
                    self.silence_accumulated_ms = 0;
                }
            }
        }

        None
    }

    pub fn state(&self) -> &VadState {
        &self.state
    }

    pub fn reset(&mut self) {
        self.state = VadState::Silence;
        self.silence_accumulated_ms = 0;
        self.speech_accumulated_ms = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(energy_level: f32, duration_ms: u64, sample_rate: u32) -> AudioChunk {
        let num_samples = (sample_rate as u64 * duration_ms / 1000) as usize;
        let samples = vec![energy_level; num_samples];
        AudioChunk::new(samples, sample_rate)
    }

    #[test]
    fn test_vad_initial_state() {
        let vad = EnergyVad::new(0.01, 1500);
        assert_eq!(*vad.state(), VadState::Silence);
    }

    #[test]
    fn test_vad_detects_speech() {
        let mut vad = EnergyVad::new(0.01, 1500);

        // Send enough speech chunks to exceed min_speech_ms (300ms)
        let loud = make_chunk(0.5, 100, 16000);
        assert_eq!(vad.process(&loud), None); // 100ms < 300ms min
        assert_eq!(vad.process(&loud), None); // 200ms < 300ms min
        let result = vad.process(&loud); // 300ms >= 300ms min
        assert_eq!(result, Some(VadState::Speaking));
    }

    #[test]
    fn test_vad_detects_silence_after_speech() {
        let mut vad = EnergyVad::new(0.01, 500);

        // First, get into speaking state
        let loud = make_chunk(0.5, 400, 16000);
        vad.process(&loud); // 400ms > 300ms min → Speaking

        // Now send silence
        let quiet = make_chunk(0.001, 300, 16000);
        assert_eq!(vad.process(&quiet), None); // 300ms < 500ms threshold
        let result = vad.process(&quiet); // 600ms >= 500ms threshold
        assert_eq!(result, Some(VadState::Silence));
    }

    #[test]
    fn test_vad_speech_resets_silence_counter() {
        let mut vad = EnergyVad::new(0.01, 1000);

        // Get into speaking state
        let loud = make_chunk(0.5, 400, 16000);
        vad.process(&loud);

        // Some silence
        let quiet = make_chunk(0.001, 500, 16000);
        vad.process(&quiet); // 500ms silence

        // Speech again — should reset silence counter
        vad.process(&loud);

        // More silence — should need full 1000ms again
        assert_eq!(vad.process(&quiet), None); // 500ms < 1000ms
        assert_eq!(vad.process(&quiet), Some(VadState::Silence)); // 1000ms
    }

    #[test]
    fn test_vad_reset() {
        let mut vad = EnergyVad::new(0.01, 1500);
        let loud = make_chunk(0.5, 400, 16000);
        vad.process(&loud);
        assert_eq!(*vad.state(), VadState::Speaking);

        vad.reset();
        assert_eq!(*vad.state(), VadState::Silence);
    }

    #[test]
    fn test_vad_noise_below_threshold() {
        let mut vad = EnergyVad::new(0.1, 1500);
        let quiet = make_chunk(0.05, 1000, 16000);

        // Should stay in silence
        assert_eq!(vad.process(&quiet), None);
        assert_eq!(*vad.state(), VadState::Silence);
    }
}
