//! The individual gesture recognizers. Each is a small explicit state machine over
//! [`Frame`]s (idle -> candidate -> committed/latched -> re-armed) whose thresholds come from
//! [`Thresholds`]. None of them knows about the game; the [`crate::engine`] arbiter decides
//! which commits are allowed through.

use crate::config::Thresholds;
use crate::frame::Frame;
use crate::math::{wrap_deg, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Gesture {
    RotateCw,
    RotateCcw,
    MoveLeft,
    MoveRight,
    HardDrop,
}

impl Gesture {
    pub fn label(self) -> &'static str {
        match self {
            Gesture::RotateCw => "Rotate clockwise",
            Gesture::RotateCcw => "Rotate anticlockwise",
            Gesture::MoveLeft => "Move left",
            Gesture::MoveRight => "Move right",
            Gesture::HardDrop => "Drop",
        }
    }
    pub fn is_rotation(self) -> bool {
        matches!(self, Gesture::RotateCw | Gesture::RotateCcw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Commit {
    pub gesture: Gesture,
    /// When the gesture's motion started (first candidate sample), µs.
    pub start_us: u64,
    /// When it was recognized, µs.
    pub t_us: u64,
    pub recognizer: &'static str,
}

impl Commit {
    /// Time the recognizer needed from motion start to decision, ms.
    pub fn recognition_ms(&self) -> f32 {
        self.t_us.saturating_sub(self.start_us) as f32 / 1000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Candidate,
    /// Committed; waiting for the motion to end / return before re-arming.
    Latched,
    /// Aborted or blocked; waiting for the motion to calm down.
    Disarmed,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Candidate => "candidate",
            Phase::Latched => "latched",
            Phase::Disarmed => "waiting",
        }
    }
}

fn sign(v: f32) -> f32 {
    if v >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

// ------------------------------------------------------------------------------------
// Wrist twist
// ------------------------------------------------------------------------------------

/// Rotation by twisting the wrist about the remote's long axis.
///
/// The twist angle is measured from a slowly adapting rest baseline. Crossing
/// `twist_enter_deg` (with a fast enough peak rate) commits one rotation and latches; the
/// latch only releases once the wrist comes back within `twist_rearm_deg` of the baseline,
/// so holding the twist never repeats and returning never rotates back. Holding a twisted
/// posture for `twist_hold_rebase_ms` adopts it as the new rest pose, and a later return to
/// the old pose is recognized as a return (not an opposite rotation).
#[derive(Debug, Clone, Default)]
pub struct Twist {
    pub phase: Phase,
    pub baseline: f32,
    have_baseline: bool,
    dir: f32,
    peak_rate: f32,
    start_us: u64,
    still_since: Option<u64>,
    return_guard: Option<(f32, f32)>,
    pub last_delta: f32,
}

impl Twist {
    pub fn reset(&mut self) {
        *self = Twist::default();
    }
    pub fn cancel(&mut self) {
        if self.phase == Phase::Candidate {
            self.phase = Phase::Idle;
        }
    }

    pub fn update(&mut self, f: &Frame, c: &Thresholds) -> Option<Commit> {
        let a = f.twist_deg?;
        if !self.have_baseline {
            self.baseline = a;
            self.have_baseline = true;
        }
        let rate = f.twist_rate_dps.unwrap_or(0.0);
        let has_rate = f.twist_rate_dps.is_some();
        let still = rate.abs() < c.still_twist_dps;
        if still {
            self.still_since.get_or_insert(f.t_us);
        } else {
            self.still_since = None;
        }
        let still_ms = self
            .still_since
            .map(|s| (f.t_us - s) as f32 / 1000.0)
            .unwrap_or(0.0);
        let d = wrap_deg(a - self.baseline);
        self.last_delta = d;
        if f.gap && self.phase == Phase::Candidate {
            self.phase = Phase::Idle;
        }
        match self.phase {
            Phase::Idle | Phase::Disarmed => {
                if still {
                    let k = (f.dt_s / c.twist_baseline_tau_s.max(0.1)).min(1.0);
                    self.baseline = wrap_deg(self.baseline + d * k);
                }
                if d.abs() > c.twist_enter_deg * 0.4 && !still {
                    self.phase = Phase::Candidate;
                    self.dir = sign(d);
                    self.peak_rate = 0.0;
                    self.start_us = f.t_us;
                }
            }
            Phase::Candidate => {
                if sign(rate) == self.dir {
                    self.peak_rate = self.peak_rate.max(rate.abs());
                }
                if sign(d) != self.dir || d.abs() < c.twist_enter_deg * 0.25 {
                    self.phase = Phase::Idle;
                } else if d.abs() >= c.twist_enter_deg {
                    if has_rate && self.peak_rate < c.twist_min_peak_dps {
                        // A slow roll is posture drift, not a gesture: adopt it silently.
                        self.baseline = a;
                        self.phase = Phase::Idle;
                        return None;
                    }
                    if let Some((old, old_dir)) = self.return_guard {
                        if self.dir == -old_dir && wrap_deg(a - old).abs() < c.twist_rearm_deg * 1.5 {
                            // Coming back to the pose held before a re-base.
                            self.baseline = old;
                            self.return_guard = None;
                            self.phase = Phase::Latched;
                            return None;
                        }
                    }
                    self.return_guard = None;
                    self.phase = Phase::Latched;
                    return Some(Commit {
                        gesture: if self.dir > 0.0 {
                            Gesture::RotateCw
                        } else {
                            Gesture::RotateCcw
                        },
                        start_us: self.start_us,
                        t_us: f.t_us,
                        recognizer: "twist",
                    });
                }
            }
            Phase::Latched => {
                if d.abs() < c.twist_rearm_deg {
                    self.phase = Phase::Idle;
                } else if still_ms >= c.twist_hold_rebase_ms {
                    self.return_guard = Some((self.baseline, self.dir));
                    self.baseline = a;
                    self.phase = Phase::Idle;
                }
            }
        }
        None
    }
}

// ------------------------------------------------------------------------------------
// Circle
// ------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CircleStats {
    pub turn_deg: f32,
    pub radius: f32,
    pub radius_cv: f32,
    pub duration_ms: f32,
    pub path_ratio: f32,
    /// Smallest point radius relative to the mean (lines pass through their centre).
    pub min_radius_ratio: f32,
    /// Largest angular step between consecutive points, degrees (centre crossings jump).
    pub max_step_deg: f32,
    /// Longest pause inside the window, ms (one circle is one continuous stroke).
    pub max_pause_ms: f32,
}

/// Rotation by drawing a compact circle with the aim point. Clockwise on screen
/// (y down) rotates clockwise.
#[derive(Debug, Clone, Default)]
pub struct Circle {
    pub phase: Phase,
    pts: VecDeque<(u64, Vec2)>,
    latched_until: u64,
    cand_start: u64,
    pub best: CircleStats,
}

impl Circle {
    pub fn reset(&mut self) {
        *self = Circle::default();
    }
    pub fn cancel(&mut self) {
        self.pts.clear();
        if self.phase == Phase::Candidate {
            self.phase = Phase::Idle;
        }
    }
    pub fn is_candidate(&self) -> bool {
        self.phase == Phase::Candidate
    }

    fn analyze(&self, from: usize) -> CircleStats {
        let n = self.pts.len() - from;
        let mut cx = 0.0;
        let mut cy = 0.0;
        for (_, p) in self.pts.iter().skip(from) {
            cx += p.x;
            cy += p.y;
        }
        let c = Vec2::new(cx / n as f32, cy / n as f32);
        let mut rsum = 0.0;
        let mut rsq = 0.0;
        let mut turn = 0.0;
        let mut path = 0.0;
        let mut rmin = f32::MAX;
        let mut pause_start: Option<u64> = None;
        let mut max_pause: u64 = 0;
        let mut prev_t: Option<(u64, Vec2)> = None;
        for (t, p) in self.pts.iter().skip(from) {
            if let Some((pt, pp)) = prev_t {
                let slow = p.sub(pp).len() / ((t - pt).max(1) as f32 / 1e6) < 0.15;
                if slow {
                    let s0 = *pause_start.get_or_insert(pt);
                    max_pause = max_pause.max(t - s0);
                } else {
                    pause_start = None;
                }
            }
            prev_t = Some((*t, *p));
        }
        let mut max_step: f32 = 0.0;
        let mut prev: Option<(Vec2, f32)> = None;
        for (_, p) in self.pts.iter().skip(from) {
            let d = p.sub(c);
            let r = d.len();
            rsum += r;
            rsq += r * r;
            rmin = rmin.min(r);
            let ang = d.y.atan2(d.x).to_degrees();
            if let Some((pp, pa)) = prev {
                let step = wrap_deg(ang - pa);
                max_step = max_step.max(step.abs());
                turn += step;
                path += p.sub(pp).len();
            }
            prev = Some((*p, ang));
        }
        let mean = rsum / n as f32;
        let var = (rsq / n as f32 - mean * mean).max(0.0);
        let dur = (self.pts.back().unwrap().0 - self.pts[from].0) as f32 / 1000.0;
        let ideal = turn.abs().to_radians() * mean;
        CircleStats {
            turn_deg: turn,
            radius: mean,
            radius_cv: if mean > 1e-6 { var.sqrt() / mean } else { 9.0 },
            duration_ms: dur,
            path_ratio: if ideal > 1e-6 { path / ideal } else { 99.0 },
            min_radius_ratio: if mean > 1e-6 { rmin / mean } else { 0.0 },
            max_step_deg: max_step,
            max_pause_ms: max_pause as f32 / 1000.0,
        }
    }

    pub fn update(&mut self, f: &Frame, c: &Thresholds) -> Option<Commit> {
        if f.gap {
            self.cancel();
        }
        if self.phase == Phase::Latched {
            let calm = f.vel.len() < c.still_speed_sh;
            if f.t_us >= self.latched_until || calm {
                self.phase = Phase::Idle;
            }
            self.pts.clear();
            return None;
        }
        let push = match self.pts.back() {
            Some((t, p)) => f.aim.sub(*p).len() > 0.002 || f.t_us - t >= 25_000,
            None => true,
        };
        if push {
            self.pts.push_back((f.t_us, f.aim));
        }
        let horizon = (c.circle_max_ms * 1000.0) as u64;
        while self.pts.len() > 2 && f.t_us - self.pts[0].0 > horizon {
            self.pts.pop_front();
        }
        while self.pts.len() > 200 {
            self.pts.pop_front();
        }
        if self.pts.len() < 8 {
            self.phase = Phase::Idle;
            return None;
        }
        let mut best = CircleStats::default();
        let mut candidate = false;
        let mut commit: Option<CircleStats> = None;
        let mut commit_from = 0;
        let last = self.pts.len() - 6;
        let mut i = 0;
        while i < last {
            let s = self.analyze(i);
            let radius_ok = s.radius >= c.circle_min_radius_sh
                && s.radius <= c.circle_max_radius_sh
                && s.min_radius_ratio >= 0.3
                && s.max_step_deg <= 75.0
                && s.max_pause_ms <= 100.0;
            if s.turn_deg.abs() > best.turn_deg.abs() && radius_ok {
                best = s;
            }
            if radius_ok
                && s.turn_deg.abs() >= c.circle_candidate_turn_deg
                && s.radius_cv <= c.circle_max_radius_cv * 1.5
            {
                candidate = true;
                if s.turn_deg.abs() >= c.circle_min_turn_deg
                    && s.radius_cv <= c.circle_max_radius_cv
                    && s.path_ratio <= c.circle_max_path_ratio
                    && s.duration_ms >= c.circle_min_ms
                    && s.duration_ms <= c.circle_max_ms
                {
                    commit = Some(s);
                    commit_from = i;
                    break;
                }
            }
            i += 2;
        }
        self.best = best;
        if let Some(s) = commit {
            let start = self.pts[commit_from].0;
            self.pts.clear();
            self.phase = Phase::Latched;
            self.latched_until = f.t_us + 250_000;
            return Some(Commit {
                gesture: if s.turn_deg > 0.0 {
                    Gesture::RotateCw
                } else {
                    Gesture::RotateCcw
                },
                start_us: start,
                t_us: f.t_us,
                recognizer: "circle",
            });
        }
        if candidate {
            if self.phase != Phase::Candidate {
                self.cand_start = f.t_us;
            }
            self.phase = Phase::Candidate;
        } else {
            self.phase = Phase::Idle;
        }
        None
    }
}

// ------------------------------------------------------------------------------------
// Horizontal flick
// ------------------------------------------------------------------------------------

/// One column per deliberate left/right flick: a short, horizontally dominant burst.
/// The opposite-direction recoil inside `flick_recoil_ms` is ignored, and a burst must
/// calm below `flick_rearm_speed` before the next flick counts. A large flick is still one
/// burst, so it moves one column.
#[derive(Debug, Clone, Default)]
pub struct Flick {
    pub phase: Phase,
    dir: f32,
    start_us: u64,
    start_x: f32,
    peak: f32,
    h0: f32,
    max_turn: f32,
    refractory_dir: f32,
    refractory_until: u64,
}

impl Flick {
    pub fn reset(&mut self) {
        *self = Flick::default();
    }
    pub fn cancel(&mut self) {
        if self.phase == Phase::Candidate {
            self.phase = Phase::Disarmed;
        }
    }

    /// `strict`: circles are enabled, so wait for the burst to end and reject curved bursts.
    pub fn update(&mut self, f: &Frame, c: &Thresholds, strict: bool) -> Option<Commit> {
        if f.gap {
            self.phase = Phase::Idle;
        }
        let vx = f.vel.x;
        let vy = f.vel.y;
        let speed = f.vel.len();
        match self.phase {
            Phase::Idle => {
                if vx.abs() > c.flick_enter_speed && vx.abs() > c.flick_dominance * vy.abs() {
                    let dir = sign(vx);
                    if dir == -self.refractory_dir && f.t_us < self.refractory_until {
                        self.phase = Phase::Disarmed; // recoil
                        return None;
                    }
                    self.phase = Phase::Candidate;
                    self.dir = dir;
                    self.start_us = f.t_us;
                    self.start_x = f.aim.x - vx * f.dt_s;
                    self.peak = vx.abs();
                    self.h0 = vy.atan2(vx).to_degrees();
                    self.max_turn = 0.0;
                }
            }
            Phase::Candidate => {
                self.peak = self.peak.max(vx * self.dir);
                if speed > c.flick_rearm_speed {
                    let h = vy.atan2(vx).to_degrees();
                    self.max_turn = self.max_turn.max(wrap_deg(h - self.h0).abs());
                }
                let travel = (f.aim.x - self.start_x) * self.dir;
                let dur = (f.t_us - self.start_us) as f32 / 1000.0;
                let along = vx * self.dir;
                if dur > c.flick_max_ms {
                    self.phase = Phase::Disarmed;
                    return None;
                }
                let ended = if strict {
                    along < c.flick_rearm_speed.max(0.3 * self.peak)
                } else {
                    along < 0.6 * self.peak
                };
                if ended {
                    if travel < c.flick_min_travel_sh
                        || (strict && self.max_turn > c.flick_max_heading_change_deg)
                    {
                        self.phase = Phase::Disarmed;
                        return None;
                    }
                    self.phase = Phase::Latched;
                    self.refractory_dir = self.dir;
                    self.refractory_until = f.t_us + (c.flick_recoil_ms * 1000.0) as u64;
                    return Some(Commit {
                        gesture: if self.dir > 0.0 {
                            Gesture::MoveRight
                        } else {
                            Gesture::MoveLeft
                        },
                        start_us: self.start_us,
                        t_us: f.t_us,
                        recognizer: "flick",
                    });
                }
            }
            Phase::Latched | Phase::Disarmed => {
                if speed < c.flick_rearm_speed {
                    self.phase = Phase::Idle;
                }
            }
        }
        None
    }
}

// ------------------------------------------------------------------------------------
// Downward hard drop
// ------------------------------------------------------------------------------------

/// A quick, deliberate downward flick of the aim. Stronger requirements than a move: a high
/// peak speed, straight-down heading throughout, minimum travel and a short duration. After a
/// commit the recognizer stays latched (across piece spawns) until a minimum time has passed
/// AND the remote has been still, so recoil never drops the next piece.
#[derive(Debug, Clone, Default)]
pub struct Drop {
    pub phase: Phase,
    start_us: u64,
    start_y: f32,
    peak: f32,
    max_dev: f32,
    latched_at: u64,
    still_since: Option<u64>,
}

impl Drop {
    pub fn reset(&mut self) {
        *self = Drop::default();
    }
    pub fn cancel(&mut self) {
        if self.phase == Phase::Candidate {
            self.phase = Phase::Disarmed;
        }
    }
    pub fn is_candidate(&self) -> bool {
        self.phase == Phase::Candidate
    }
    pub fn is_latched(&self) -> bool {
        self.phase == Phase::Latched
    }

    /// `blocked`: the arbiter forbids a drop commit right now (rotation in progress etc.).
    pub fn update(&mut self, f: &Frame, c: &Thresholds, blocked: bool) -> Option<Commit> {
        let vx = f.vel.x;
        let vy = f.vel.y;
        let speed = f.vel.len();
        if f.gap && self.phase == Phase::Candidate {
            self.phase = Phase::Idle;
        }
        match self.phase {
            Phase::Idle => {
                if vy > c.drop_enter_speed && vy > c.drop_dominance * vx.abs() {
                    self.phase = Phase::Candidate;
                    self.start_us = f.t_us;
                    self.start_y = f.aim.y - vy * f.dt_s;
                    self.peak = vy;
                    self.max_dev = 0.0;
                }
            }
            Phase::Candidate => {
                self.peak = self.peak.max(vy);
                if speed > c.drop_enter_speed * 0.5 {
                    let dev = vx.abs().atan2(vy).to_degrees();
                    self.max_dev = self.max_dev.max(dev);
                }
                let dur = (f.t_us - self.start_us) as f32 / 1000.0;
                if self.max_dev > c.drop_max_heading_dev_deg || dur > c.drop_max_ms || blocked {
                    self.phase = Phase::Disarmed;
                    return None;
                }
                if vy < 0.5 * self.peak {
                    let travel = f.aim.y - self.start_y;
                    if self.peak >= c.drop_min_peak_speed && travel >= c.drop_min_travel_sh {
                        self.phase = Phase::Latched;
                        self.latched_at = f.t_us;
                        self.still_since = None;
                        return Some(Commit {
                            gesture: Gesture::HardDrop,
                            start_us: self.start_us,
                            t_us: f.t_us,
                            recognizer: "drop",
                        });
                    }
                    self.phase = Phase::Disarmed;
                }
            }
            Phase::Latched => {
                if speed < c.still_speed_sh {
                    self.still_since.get_or_insert(f.t_us);
                } else {
                    self.still_since = None;
                }
                let latched_ms = (f.t_us - self.latched_at) as f32 / 1000.0;
                let still_ms = self
                    .still_since
                    .map(|s| (f.t_us - s) as f32 / 1000.0)
                    .unwrap_or(0.0);
                if latched_ms >= c.drop_min_latch_ms && still_ms >= c.drop_rearm_still_ms {
                    self.phase = Phase::Idle;
                }
            }
            Phase::Disarmed => {
                if speed < c.flick_rearm_speed {
                    self.phase = Phase::Idle;
                }
            }
        }
        None
    }
}
