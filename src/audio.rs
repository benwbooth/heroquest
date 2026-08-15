use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use anyhow::{Result, anyhow};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

const SAMPLE_RATE: i32 = 48_000;
const MAX_IMPACT_VOICES: usize = 24;

#[derive(Debug, Clone, Copy)]
struct ImpactCommand {
    strength: f32,
}

/// Low-latency die-on-wood audio. The game thread only submits normalized
/// Rapier contact energy; SDL's audio thread synthesizes and layers the short
/// resonances without blocking rendering or physics.
pub struct TabletopDiceAudio {
    sender: SyncSender<ImpactCommand>,
    _stream: AudioStreamWithCallback<TabletopAudioCallback>,
}

impl TabletopDiceAudio {
    pub fn new(sdl: &sdl3::Sdl) -> Result<Self> {
        let audio = sdl.audio().map_err(|error| anyhow!(error.to_string()))?;
        let spec = AudioSpec {
            freq: Some(SAMPLE_RATE),
            channels: Some(1),
            format: Some(AudioFormat::f32_sys()),
        };
        let (sender, receiver) = sync_channel(64);
        let callback = TabletopAudioCallback {
            receiver,
            mixer: ImpactMixer::new(SAMPLE_RATE as f32),
            output: Vec::new(),
        };
        let stream = audio
            .open_playback_stream(&spec, callback)
            .map_err(|error| anyhow!(error.to_string()))?;
        stream
            .resume()
            .map_err(|error| anyhow!(error.to_string()))?;
        log::info!(
            "tabletop impact audio active at {SAMPLE_RATE} Hz via {}",
            audio.current_audio_driver()
        );
        Ok(Self {
            sender,
            _stream: stream,
        })
    }

    pub fn play_impact(&self, strength: f32) {
        if !strength.is_finite() || strength <= 0.0 {
            return;
        }
        // Audio is cosmetic: a saturated queue must never stall the game loop.
        let _ = self.sender.try_send(ImpactCommand {
            strength: strength.clamp(0.0, 1.0),
        });
    }
}

struct TabletopAudioCallback {
    receiver: Receiver<ImpactCommand>,
    mixer: ImpactMixer,
    output: Vec<f32>,
}

impl AudioCallback<f32> for TabletopAudioCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        loop {
            match self.receiver.try_recv() {
                Ok(command) => self.mixer.trigger(command.strength),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        let requested = requested.max(0) as usize;
        self.output.resize(requested, 0.0);
        self.mixer.render(&mut self.output);
        if let Err(error) = stream.put_data_f32(&self.output) {
            log::warn!("SDL could not queue tabletop impact audio: {error}");
        }
    }
}

#[derive(Debug)]
struct ImpactMixer {
    sample_rate: f32,
    sequence: u32,
    voices: Vec<ImpactVoice>,
}

impl ImpactMixer {
    fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            sequence: 0x4845_524f,
            voices: Vec::new(),
        }
    }

    fn trigger(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 {
            return;
        }
        self.sequence ^= self.sequence << 13;
        self.sequence ^= self.sequence >> 17;
        self.sequence ^= self.sequence << 5;
        let variation = (self.sequence & 0xffff) as f32 / u16::MAX as f32;
        let pitch = 0.91 + variation * 0.18;
        if self.voices.len() >= MAX_IMPACT_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(ImpactVoice {
            age: 0,
            length: (self.sample_rate * (0.12 + strength * 0.10)) as usize,
            gain: 0.15 + strength.sqrt() * 0.34,
            pitch,
            noise_state: self.sequence | 1,
        });
    }

    fn render(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        for sample in output.iter_mut() {
            let mut mixed = 0.0;
            for voice in &mut self.voices {
                if voice.age < voice.length {
                    mixed += voice.next_sample(self.sample_rate);
                }
            }
            // Soft saturation keeps simultaneous combat dice weighty without
            // clipping when several hit the table on the same audio frame.
            *sample = (mixed * 1.15).tanh() * 0.82;
        }
        self.voices.retain(|voice| voice.age < voice.length);
    }
}

#[derive(Debug)]
struct ImpactVoice {
    age: usize,
    length: usize,
    gain: f32,
    pitch: f32,
    noise_state: u32,
}

impl ImpactVoice {
    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        let time = self.age as f32 / sample_rate;
        self.age += 1;
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 17;
        self.noise_state ^= self.noise_state << 5;
        let noise = (self.noise_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let phase = |hz: f32| (std::f32::consts::TAU * hz * self.pitch * time).sin();

        // A short broadband plastic click excites several damped wooden-table
        // modes. Different decay rates prevent the result from sounding like
        // a single electronic beep while remaining compact and responsive.
        let click = noise * (-105.0 * time).exp() * 0.30;
        let body = phase(184.0) * (-18.0 * time).exp() * 0.42
            + phase(317.0) * (-25.0 * time).exp() * 0.28
            + phase(503.0) * (-37.0 * time).exp() * 0.18
            + phase(811.0) * (-55.0 * time).exp() * 0.09;
        self.gain * (click + body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_on_wood_synthesis_has_a_fast_attack_and_decaying_tail() {
        let mut mixer = ImpactMixer::new(SAMPLE_RATE as f32);
        mixer.trigger(0.8);
        let mut samples = vec![0.0; SAMPLE_RATE as usize / 4];
        mixer.render(&mut samples);
        let early_peak = samples[..2_400]
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        let late_peak = samples[9_600..]
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max);
        assert!(early_peak > 0.08);
        assert!(late_peak < early_peak * 0.08);
        assert!(samples.iter().all(|sample| sample.abs() <= 0.82));
        assert!(mixer.voices.is_empty());
    }

    #[test]
    fn simultaneous_dice_layer_without_clipping() {
        let mut mixer = ImpactMixer::new(SAMPLE_RATE as f32);
        for strength in [1.0, 0.9, 0.75, 0.6, 0.45, 0.3] {
            mixer.trigger(strength);
        }
        let mut samples = vec![0.0; 4_800];
        mixer.render(&mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.15));
        assert!(samples.iter().all(|sample| sample.abs() <= 0.82));
    }
}
