use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use anyhow::{Result, anyhow};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

const SAMPLE_RATE: i32 = 48_000;
const MAX_IMPACT_VOICES: usize = 24;
const MAX_EFFECT_VOICES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEffect {
    Movement,
    Attack,
    Damage,
    Spell,
    DoorOpen,
    Search,
    Turn,
    QuestComplete,
}

#[derive(Debug, Clone, Copy)]
enum AudioCommand {
    Impact(f32),
    Effect(SoundEffect),
}

/// Low-latency procedural tabletop audio. The game thread submits physical-die
/// contact energy and semantic game events; SDL's audio thread synthesizes and
/// layers their short resonances without blocking rendering or physics.
pub struct GameAudio {
    sender: SyncSender<AudioCommand>,
    _stream: AudioStreamWithCallback<GameAudioCallback>,
}

impl GameAudio {
    pub fn new(sdl: &sdl3::Sdl) -> Result<Self> {
        let audio = sdl.audio().map_err(|error| anyhow!(error.to_string()))?;
        let spec = AudioSpec {
            freq: Some(SAMPLE_RATE),
            channels: Some(1),
            format: Some(AudioFormat::f32_sys()),
        };
        let (sender, receiver) = sync_channel(64);
        let callback = GameAudioCallback {
            receiver,
            mixer: AudioMixer::new(SAMPLE_RATE as f32),
            output: Vec::new(),
        };
        let stream = audio
            .open_playback_stream(&spec, callback)
            .map_err(|error| anyhow!(error.to_string()))?;
        stream
            .resume()
            .map_err(|error| anyhow!(error.to_string()))?;
        log::info!(
            "procedural game audio active at {SAMPLE_RATE} Hz via {}",
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
        let _ = self
            .sender
            .try_send(AudioCommand::Impact(strength.clamp(0.0, 1.0)));
    }

    pub fn play_effect(&self, effect: SoundEffect) {
        let _ = self.sender.try_send(AudioCommand::Effect(effect));
    }
}

struct GameAudioCallback {
    receiver: Receiver<AudioCommand>,
    mixer: AudioMixer,
    output: Vec<f32>,
}

impl AudioCallback<f32> for GameAudioCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        loop {
            match self.receiver.try_recv() {
                Ok(AudioCommand::Impact(strength)) => self.mixer.trigger_impact(strength),
                Ok(AudioCommand::Effect(effect)) => self.mixer.trigger_effect(effect),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        let requested = requested.max(0) as usize;
        self.output.resize(requested, 0.0);
        self.mixer.render(&mut self.output);
        if let Err(error) = stream.put_data_f32(&self.output) {
            log::warn!("SDL could not queue game audio: {error}");
        }
    }
}

#[derive(Debug)]
struct AudioMixer {
    sample_rate: f32,
    sequence: u32,
    impact_voices: Vec<ImpactVoice>,
    effect_voices: Vec<EffectVoice>,
}

impl AudioMixer {
    fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            sequence: 0x4845_524f,
            impact_voices: Vec::new(),
            effect_voices: Vec::new(),
        }
    }

    fn next_seed(&mut self) -> u32 {
        self.sequence ^= self.sequence << 13;
        self.sequence ^= self.sequence >> 17;
        self.sequence ^= self.sequence << 5;
        self.sequence
    }

    fn trigger_impact(&mut self, strength: f32) {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 {
            return;
        }
        let seed = self.next_seed();
        let variation = (seed & 0xffff) as f32 / u16::MAX as f32;
        let pitch = 0.91 + variation * 0.18;
        if self.impact_voices.len() >= MAX_IMPACT_VOICES {
            self.impact_voices.remove(0);
        }
        self.impact_voices.push(ImpactVoice {
            age: 0,
            length: (self.sample_rate * (0.12 + strength * 0.10)) as usize,
            gain: 0.15 + strength.sqrt() * 0.34,
            pitch,
            noise_state: seed | 1,
        });
    }

    fn trigger_effect(&mut self, effect: SoundEffect) {
        let seed = self.next_seed();
        if self.effect_voices.len() >= MAX_EFFECT_VOICES {
            self.effect_voices.remove(0);
        }
        self.effect_voices
            .push(EffectVoice::new(effect, self.sample_rate, seed));
    }

    fn render(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        for sample in output.iter_mut() {
            let mut mixed = 0.0;
            for voice in &mut self.impact_voices {
                if voice.age < voice.length {
                    mixed += voice.next_sample(self.sample_rate);
                }
            }
            for voice in &mut self.effect_voices {
                if voice.age < voice.length {
                    mixed += voice.next_sample(self.sample_rate);
                }
            }
            // Soft saturation keeps simultaneous combat dice and cues weighty
            // without clipping when several events share an audio frame.
            *sample = (mixed * 1.15).tanh() * 0.82;
        }
        self.impact_voices.retain(|voice| voice.age < voice.length);
        self.effect_voices.retain(|voice| voice.age < voice.length);
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
        let noise = next_noise(&mut self.noise_state);
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

#[derive(Debug)]
struct EffectVoice {
    effect: SoundEffect,
    age: usize,
    length: usize,
    noise_state: u32,
    variation: f32,
}

impl EffectVoice {
    fn new(effect: SoundEffect, sample_rate: f32, seed: u32) -> Self {
        let seconds = match effect {
            SoundEffect::Movement => 0.16,
            SoundEffect::Attack => 0.34,
            SoundEffect::Damage => 0.30,
            SoundEffect::Spell => 0.72,
            SoundEffect::DoorOpen => 0.46,
            SoundEffect::Search => 0.34,
            SoundEffect::Turn => 0.38,
            SoundEffect::QuestComplete => 1.10,
        };
        Self {
            effect,
            age: 0,
            length: (sample_rate * seconds) as usize,
            noise_state: seed | 1,
            variation: 0.94 + ((seed >> 8) & 0xff) as f32 / 255.0 * 0.12,
        }
    }

    fn next_sample(&mut self, sample_rate: f32) -> f32 {
        let time = self.age as f32 / sample_rate;
        let progress = self.age as f32 / self.length.max(1) as f32;
        self.age += 1;
        let noise = next_noise(&mut self.noise_state);
        let tone = |hz: f32| (std::f32::consts::TAU * hz * self.variation * time).sin();
        let tail = (1.0 - progress).max(0.0);

        match self.effect {
            SoundEffect::Movement => {
                // A boot step: leather slap followed by a muted tabletop thud.
                let first = (-54.0 * time).exp();
                let second_time = (time - 0.070).max(0.0);
                let second = if time >= 0.070 {
                    (-62.0 * second_time).exp()
                } else {
                    0.0
                };
                (noise * 0.20 + tone(105.0) * 0.34) * (first + second * 0.72) * 0.48
            }
            SoundEffect::Attack => {
                // A fast weapon whoosh resolving into a short metallic strike.
                let sweep = noise * (progress * std::f32::consts::PI).sin() * tail * 0.24;
                let strike_time = (time - 0.105).max(0.0);
                let strike = if time >= 0.105 {
                    (-18.0 * strike_time).exp() * (tone(436.0) * 0.25 + tone(691.0) * 0.16)
                } else {
                    0.0
                };
                sweep + strike
            }
            SoundEffect::Damage => {
                // Low impact with a brief noisy crack, distinct from dice.
                let envelope = (-13.0 * time).exp();
                (tone(72.0) * 0.38 + tone(113.0) * 0.20 + noise * (-70.0 * time).exp() * 0.28)
                    * envelope
            }
            SoundEffect::Spell => {
                // Rising arcane shimmer with a descending resonant tail.
                let rise = (progress * 5.0).min(1.0);
                let shimmer = tone(310.0 + progress * 260.0) * 0.19
                    + tone(465.0 + progress * 390.0) * 0.13
                    + tone(775.0 - progress * 180.0) * 0.10;
                (shimmer + noise * 0.035) * rise * tail.sqrt()
            }
            SoundEffect::DoorOpen => {
                // Heavy wooden scrape, hinge creak, and latch release.
                let scrape = noise * (0.11 + 0.08 * tone(8.0)) * tail;
                let creak = tone(82.0 + tone(3.4) * 24.0) * 0.25 * tail;
                let latch = if time < 0.045 {
                    (noise * 0.24 + tone(242.0) * 0.16) * (-52.0 * time).exp()
                } else {
                    0.0
                };
                scrape + creak + latch
            }
            SoundEffect::Search => {
                // Two quick object-handling taps and a curious bright response.
                let tap = |offset: f32| {
                    let local = time - offset;
                    if local >= 0.0 {
                        (-55.0 * local).exp()
                    } else {
                        0.0
                    }
                };
                (noise * 0.20 + tone(355.0) * 0.18) * (tap(0.0) + tap(0.115) * 0.82)
                    + tone(710.0) * (-9.0 * time).exp() * 0.08
            }
            SoundEffect::Turn => {
                // Compact two-note herald used when control passes around the table.
                let second_time = (time - 0.145).max(0.0);
                tone(196.0) * (-10.0 * time).exp() * 0.20
                    + if time >= 0.145 {
                        tone(294.0) * (-13.0 * second_time).exp() * 0.24
                    } else {
                        0.0
                    }
            }
            SoundEffect::QuestComplete => {
                // A restrained three-note victory cadence rather than a UI beep.
                let note = |offset: f32, hz: f32| {
                    let local = time - offset;
                    if local >= 0.0 {
                        (std::f32::consts::TAU * hz * self.variation * local).sin()
                            * (-3.2 * local).exp()
                    } else {
                        0.0
                    }
                };
                (note(0.0, 196.0) + note(0.24, 247.0) + note(0.48, 294.0)) * 0.23
                    + tone(588.0) * tail * 0.04
            }
        }
    }
}

fn next_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_on_wood_synthesis_has_a_fast_attack_and_decaying_tail() {
        let mut mixer = AudioMixer::new(SAMPLE_RATE as f32);
        mixer.trigger_impact(0.8);
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
        assert!(mixer.impact_voices.is_empty());
    }

    #[test]
    fn simultaneous_dice_layer_without_clipping() {
        let mut mixer = AudioMixer::new(SAMPLE_RATE as f32);
        for strength in [1.0, 0.9, 0.75, 0.6, 0.45, 0.3] {
            mixer.trigger_impact(strength);
        }
        let mut samples = vec![0.0; 4_800];
        mixer.render(&mut samples);
        assert!(samples.iter().any(|sample| sample.abs() > 0.15));
        assert!(samples.iter().all(|sample| sample.abs() <= 0.82));
    }

    #[test]
    fn every_game_effect_is_audible_bounded_and_finite() {
        for effect in [
            SoundEffect::Movement,
            SoundEffect::Attack,
            SoundEffect::Damage,
            SoundEffect::Spell,
            SoundEffect::DoorOpen,
            SoundEffect::Search,
            SoundEffect::Turn,
            SoundEffect::QuestComplete,
        ] {
            let mut mixer = AudioMixer::new(SAMPLE_RATE as f32);
            mixer.trigger_effect(effect);
            let mut samples = vec![0.0; (SAMPLE_RATE as f32 * 1.2) as usize];
            mixer.render(&mut samples);
            let peak = samples
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0_f32, f32::max);
            assert!(peak > 0.025, "{effect:?} was inaudible ({peak})");
            assert!(samples.iter().all(|sample| sample.is_finite()));
            assert!(samples.iter().all(|sample| sample.abs() <= 0.82));
            assert!(mixer.effect_voices.is_empty());
        }
    }

    #[test]
    fn game_effects_have_distinct_waveforms() {
        let signature = |effect| {
            let mut mixer = AudioMixer::new(SAMPLE_RATE as f32);
            mixer.trigger_effect(effect);
            let mut samples = vec![0.0; 4_800];
            mixer.render(&mut samples);
            samples
                .iter()
                .step_by(97)
                .map(|sample| sample.abs())
                .sum::<f32>()
        };
        let movement = signature(SoundEffect::Movement);
        let attack = signature(SoundEffect::Attack);
        let spell = signature(SoundEffect::Spell);
        assert!((movement - attack).abs() > 0.05);
        assert!((attack - spell).abs() > 0.05);
        assert!((movement - spell).abs() > 0.05);
    }
}
