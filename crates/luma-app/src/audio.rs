//! Quiet, original sound effects synthesized at runtime (no sample files): soft sine and
//! triangle tones with short envelopes. The audio callback never blocks the game: cues are
//! handed over with `try_lock`, and a failure to open audio simply disables sound.

use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use std::sync::{Arc, Mutex};

const RATE: i32 = 48_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    Move,
    Rotate,
    Lock,
    HardDrop,
    Clear(u8),
    Special,
    Hold,
    Invalid,
    MenuMove,
    MenuSelect,
    LevelUp,
    GameOver,
    Recognized,
}

#[derive(Clone, Copy)]
struct Voice {
    f0: f32,
    f1: f32,
    dur: f32,
    t: f32,
    gain: f32,
    delay: f32,
    tri: bool,
    phase: f32,
}

impl Voice {
    fn new(f0: f32, f1: f32, dur: f32, gain: f32, delay: f32, tri: bool) -> Voice {
        Voice {
            f0,
            f1,
            dur,
            t: 0.0,
            gain,
            delay,
            tri,
            phase: 0.0,
        }
    }
}

fn voices(s: Sfx) -> Vec<Voice> {
    match s {
        Sfx::Move => vec![Voice::new(660.0, 640.0, 0.035, 0.10, 0.0, false)],
        Sfx::Rotate => vec![Voice::new(520.0, 780.0, 0.06, 0.12, 0.0, false)],
        Sfx::Lock => vec![Voice::new(180.0, 140.0, 0.07, 0.22, 0.0, true)],
        Sfx::HardDrop => vec![
            Voice::new(240.0, 120.0, 0.09, 0.24, 0.0, true),
            Voice::new(880.0, 880.0, 0.03, 0.05, 0.0, false),
        ],
        Sfx::Clear(n) => {
            let notes = [523.25, 659.25, 783.99, 1046.5];
            (0..n.clamp(1, 4) as usize)
                .map(|i| Voice::new(notes[i], notes[i], 0.18, 0.11, i as f32 * 0.045, false))
                .collect()
        }
        Sfx::Special => [587.33, 739.99, 880.0, 1174.66]
            .iter()
            .enumerate()
            .map(|(i, f)| Voice::new(*f, *f, 0.28, 0.10, i as f32 * 0.06, false))
            .collect(),
        Sfx::Hold => vec![Voice::new(440.0, 330.0, 0.08, 0.10, 0.0, false)],
        Sfx::Invalid => vec![Voice::new(150.0, 140.0, 0.05, 0.10, 0.0, true)],
        Sfx::MenuMove => vec![Voice::new(880.0, 880.0, 0.025, 0.06, 0.0, false)],
        Sfx::MenuSelect => vec![
            Voice::new(660.0, 660.0, 0.05, 0.08, 0.0, false),
            Voice::new(990.0, 990.0, 0.07, 0.07, 0.04, false),
        ],
        Sfx::LevelUp => [440.0, 554.37, 659.25]
            .iter()
            .enumerate()
            .map(|(i, f)| Voice::new(*f, *f, 0.16, 0.09, i as f32 * 0.07, false))
            .collect(),
        Sfx::GameOver => [392.0, 329.63, 261.63]
            .iter()
            .enumerate()
            .map(|(i, f)| Voice::new(*f, *f * 0.98, 0.35, 0.10, i as f32 * 0.18, false))
            .collect(),
        Sfx::Recognized => vec![Voice::new(1320.0, 1320.0, 0.04, 0.05, 0.0, false)],
    }
}

struct Mixer {
    incoming: Arc<Mutex<Vec<Voice>>>,
    active: Vec<Voice>,
    volume: Arc<Mutex<f32>>,
    vol: f32,
}

impl AudioCallback for Mixer {
    type Channel = f32;
    fn callback(&mut self, out: &mut [f32]) {
        if let Ok(mut q) = self.incoming.try_lock() {
            self.active.append(&mut q);
        }
        if let Ok(v) = self.volume.try_lock() {
            self.vol = *v;
        }
        self.active.truncate(24);
        let dt = 1.0 / RATE as f32;
        for frame in out.chunks_mut(2) {
            let mut s = 0.0;
            for v in self.active.iter_mut() {
                if v.delay > 0.0 {
                    v.delay -= dt;
                    continue;
                }
                let p = (v.t / v.dur).min(1.0);
                let f = v.f0 + (v.f1 - v.f0) * p;
                v.phase = (v.phase + f * dt).fract();
                let w = if v.tri {
                    4.0 * (v.phase - 0.5).abs() - 1.0
                } else {
                    (v.phase * std::f32::consts::TAU).sin()
                };
                // 4 ms attack, exponential-ish release over the rest.
                let env = (v.t / 0.004).min(1.0) * (1.0 - p).powf(2.2);
                s += w * env * v.gain;
                v.t += dt;
            }
            self.active.retain(|v| v.t < v.dur);
            let s = (s * self.vol).clamp(-0.9, 0.9);
            for c in frame.iter_mut() {
                *c = s;
            }
        }
    }
}

pub struct Audio {
    _dev: Option<AudioDevice<Mixer>>,
    incoming: Arc<Mutex<Vec<Voice>>>,
    volume: Arc<Mutex<f32>>,
    pub error: Option<String>,
}

impl Audio {
    pub fn open(sdl: &sdl2::Sdl) -> Audio {
        let incoming = Arc::new(Mutex::new(Vec::new()));
        let volume = Arc::new(Mutex::new(0.6));
        let r = (|| -> Result<AudioDevice<Mixer>, String> {
            let a = sdl.audio()?;
            let spec = AudioSpecDesired {
                freq: Some(RATE),
                channels: Some(2),
                samples: Some(1024),
            };
            let dev = a.open_playback(None, &spec, |got| {
                log::info!(
                    "audio: {} Hz x{} ({} samples)",
                    got.freq,
                    got.channels,
                    got.samples
                );
                Mixer {
                    incoming: incoming.clone(),
                    active: Vec::new(),
                    volume: volume.clone(),
                    vol: 0.6,
                }
            })?;
            dev.resume();
            Ok(dev)
        })();
        match r {
            Ok(d) => Audio {
                _dev: Some(d),
                incoming,
                volume,
                error: None,
            },
            Err(e) => {
                log::warn!("audio unavailable: {e}");
                Audio {
                    _dev: None,
                    incoming,
                    volume,
                    error: Some(e),
                }
            }
        }
    }

    pub fn set_volume(&self, steps: u32, mute: bool) {
        let v = if mute {
            0.0
        } else {
            (steps.min(10) as f32 / 10.0).powf(1.6)
        };
        if let Ok(mut g) = self.volume.lock() {
            *g = v;
        }
    }

    pub fn play(&self, s: Sfx) {
        if self._dev.is_none() {
            return;
        }
        if let Ok(mut q) = self.incoming.try_lock() {
            if q.len() < 32 {
                q.extend(voices(s));
            }
        }
    }
}
