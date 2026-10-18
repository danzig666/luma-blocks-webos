//! Local, bounded gesture traces (JSON lines) and their replay through the same
//! [`GestureEngine`] the game uses. A trace records the backend, units, calibration, profile,
//! thresholds and the normalized samples; nothing else about the user or device. Traces stay
//! on the TV unless the player copies them off with the log helper; nothing is uploaded.

use crate::buttons::Button;
use crate::calib::Calibration;
use crate::config::{Profile, Thresholds};
use crate::engine::GestureEngine;
use crate::math::{Quat, Vec2, Vec3};
use crate::recognize::{Commit, Gesture};
use crate::sample::{MotionSample, Origin};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const TRACE_VERSION: u32 = 1;
pub const UNITS: &str = "t: monotonic us; q: quaternion [w,x,y,z]; g: rad/s body; a, la: m/s^2 body; c: service cursor px; pointer x,y: screen heights";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)] // one header per trace
pub enum TraceLine {
    Header {
        version: u32,
        origin: Origin,
        units: String,
        calibration: Calibration,
        profile: Profile,
        thresholds: Thresholds,
        #[serde(default)]
        note: String,
    },
    Motion {
        t: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        q: Option<[f32; 4]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        g: Option<[f32; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        a: Option<[f32; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        la: Option<[f32; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        c: Option<[f32; 2]>,
    },
    Pointer {
        t: u64,
        x: f32,
        y: f32,
    },
    Button {
        t: u64,
        b: Button,
        down: bool,
    },
    /// What the live engine recognized, for comparison with a replay.
    Recognized {
        t: u64,
        g: Gesture,
    },
    /// Pause / resume / focus change.
    Reset {
        t: u64,
    },
}

fn v3(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

impl TraceLine {
    pub fn motion(s: &MotionSample) -> TraceLine {
        TraceLine::Motion {
            t: s.t_us,
            q: s.quat.map(|q| [q.w, q.x, q.y, q.z]),
            g: s.gyro.map(v3),
            a: s.accel.map(v3),
            la: s.linear_accel.map(v3),
            c: s.cursor.map(|c| [c.0, c.1]),
        }
    }

    pub fn time(&self) -> Option<u64> {
        match self {
            TraceLine::Header { .. } => None,
            TraceLine::Motion { t, .. }
            | TraceLine::Pointer { t, .. }
            | TraceLine::Button { t, .. }
            | TraceLine::Recognized { t, .. }
            | TraceLine::Reset { t } => Some(*t),
        }
    }

    pub fn to_sample(&self) -> Option<MotionSample> {
        if let TraceLine::Motion { t, q, g, a, la, c } = self {
            let mk = |v: &Option<[f32; 3]>| v.map(|v| Vec3::new(v[0], v[1], v[2]));
            Some(
                MotionSample {
                    t_us: *t,
                    quat: q.map(|q| Quat::new(q[0], q[1], q[2], q[3])),
                    gyro: mk(g),
                    accel: mk(a),
                    linear_accel: mk(la),
                    cursor: c.map(|c| (c[0], c[1])),
                    interval_ms: None,
                }
                .sanitized(),
            )
        } else {
            None
        }
    }
}

/// Bounded in-memory recorder. Writing to disk happens elsewhere (off the frame loop).
#[derive(Debug, Clone)]
pub struct Recorder {
    pub lines: VecDeque<TraceLine>,
    pub max_lines: usize,
    pub header: Option<TraceLine>,
    pub dropped: u64,
}

impl Recorder {
    pub fn new(max_lines: usize) -> Self {
        Recorder {
            lines: VecDeque::new(),
            max_lines,
            header: None,
            dropped: 0,
        }
    }
    pub fn push(&mut self, l: TraceLine) {
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(l);
    }
    pub fn to_jsonl(&self) -> String {
        let mut s = String::new();
        for l in self.header.iter().chain(self.lines.iter()) {
            if let Ok(j) = serde_json::to_string(l) {
                s.push_str(&j);
                s.push('\n');
            }
        }
        s
    }
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Parse a JSONL trace. Malformed lines are counted and skipped.
pub fn parse(text: &str) -> (Vec<TraceLine>, usize) {
    let mut out = Vec::new();
    let mut bad = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<TraceLine>(line) {
            Ok(l) => out.push(l),
            Err(_) => bad += 1,
        }
    }
    (out, bad)
}

/// Replay a trace through a fresh engine. `profile` overrides the recorded profile (to try a
/// trace under another gesture style); thresholds/calibration come from the header unless
/// overridden.
pub fn replay(lines: &[TraceLine], profile: Option<Profile>, thresholds: Option<Thresholds>) -> Vec<Commit> {
    let (mut cal, mut prof, mut thr) = (Calibration::default(), Profile::default(), Thresholds::default());
    if let Some(TraceLine::Header {
        calibration,
        profile,
        thresholds,
        ..
    }) = lines.iter().find(|l| matches!(l, TraceLine::Header { .. }))
    {
        cal = calibration.clone();
        prof = *profile;
        thr = thresholds.clone();
    }
    if let Some(p) = profile {
        prof = p;
    }
    if let Some(t) = thresholds {
        thr = t;
    }
    let mut e = GestureEngine::new(thr, prof, cal);
    let mut out = Vec::new();
    for l in lines {
        if let Some(t) = l.time() {
            e.tick(t);
        }
        match l {
            TraceLine::Motion { .. } => {
                if let Some(s) = l.to_sample() {
                    e.motion(&s);
                }
            }
            TraceLine::Pointer { t, x, y } => e.pointer(*t, Vec2::new(*x, *y)),
            TraceLine::Button { t, down: true, .. } => e.button(*t),
            TraceLine::Reset { .. } => e.reset(),
            _ => {}
        }
        out.extend(e.take());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{gestures, Synth};

    #[test]
    fn round_trip_and_replay_matches_live() {
        let mut s = Synth::new(100.0, 3);
        let mut samples = s.hold(400.0);
        samples.extend(gestures::twist(&mut s, 1.0, 45.0, 220.0));
        let mut live = GestureEngine::new(Thresholds::default(), Profile::default(), Calibration::default());
        let mut rec = Recorder::new(10_000);
        rec.header = Some(TraceLine::Header {
            version: TRACE_VERSION,
            origin: Origin::Synthetic,
            units: UNITS.into(),
            calibration: Calibration::default(),
            profile: Profile::default(),
            thresholds: Thresholds::default(),
            note: String::new(),
        });
        let mut live_out = Vec::new();
        for m in &samples {
            live.tick(m.t_us);
            live.motion(m);
            rec.push(TraceLine::motion(m));
            live_out.extend(live.take());
        }
        let (lines, bad) = parse(&rec.to_jsonl());
        assert_eq!(bad, 0);
        let replayed = replay(&lines, None, None);
        assert_eq!(live_out.len(), 1);
        assert_eq!(replayed.len(), live_out.len());
        assert_eq!(replayed[0].gesture, live_out[0].gesture);
    }

    #[test]
    fn recorder_is_bounded() {
        let mut r = Recorder::new(3);
        for t in 0..10 {
            r.push(TraceLine::Reset { t });
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.dropped, 7);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let (l, bad) = parse(
            "{\"type\":\"reset\",\"t\":5}\nnot json\n{\"type\":\"pointer\",\"t\":6,\"x\":0.1,\"y\":0.2}\n",
        );
        assert_eq!(l.len(), 2);
        assert_eq!(bad, 1);
    }
}
