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
    /// Narrowest over widest spread of the points (1 = round, 0 = a line). A swing and its
    /// pull back draw a flat loop that turns like a circle but is far from round.
    pub axis_ratio: f32,
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
        let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
        for (_, p) in self.pts.iter().skip(from) {
            let d = p.sub(c);
            sxx += d.x * d.x;
            syy += d.y * d.y;
            sxy += d.x * d.y;
        }
        // Eigenvalues of the 2x2 covariance: the spread along the main and the minor axis.
        let tr = sxx + syy;
        let det = sxx * syy - sxy * sxy;
        let disc = (tr * tr / 4.0 - det).max(0.0).sqrt();
        let (l1, l2) = (tr / 2.0 + disc, (tr / 2.0 - disc).max(0.0));
        let axis_ratio = if l1 > 1e-12 { (l2 / l1).sqrt() } else { 0.0 };
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
            axis_ratio,
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
            // A partial arc is naturally flatter than a full circle: candidates need less
            // roundness (just under half), commits all of it.
            if radius_ok
                && s.turn_deg.abs() >= c.circle_candidate_turn_deg
                && s.radius_cv <= c.circle_max_radius_cv * 1.5
                && s.axis_ratio >= c.circle_min_axis_ratio * 0.45
            {
                candidate = true;
                if s.turn_deg.abs() >= c.circle_min_turn_deg
                    && s.axis_ratio >= c.circle_min_axis_ratio
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
            self.latched_until = f.t_us + 500_000;
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
// Sideways strokes (Swing and Flick styles)
// ------------------------------------------------------------------------------------

/// Columns earned by a stroke of `travel` SH: the first at `swing_first_col` x
/// `swing_col_sh`, the next a full `swing_col_sh` later, and every further one
/// `swing_col_growth` longer than the one before.
pub fn swing_columns(travel: f32, c: &Thresholds) -> i32 {
    let col = c.swing_col_sh.max(0.005);
    let mut need = col * c.swing_first_col;
    let mut n = 0;
    let mut step = col;
    while travel >= need && n < c.swing_max_cols as i32 {
        n += 1;
        need += step;
        step += col * c.swing_col_growth.max(0.0);
    }
    n
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeMode {
    /// Swing: the stroke moves as many columns as it is long, live while it happens.
    #[default]
    Columns,
    /// Flick: each stroke moves exactly one column, when it ends.
    Single,
}

/// Sideways movement by quick strokes, with the natural return pull ignored.
///
/// People move a remote sideways and then bring it back. Absolute pointing turns that return
/// into a move back; this recognizer treats it like lifting a mouse off its pad:
/// * A stroke starts only above `swing_start_speed` with horizontal dominance and without a
///   wrist twist in progress, so slow drifting never moves a piece.
/// * In `Columns` mode the piece moves by [`swing_columns`] of the stroke's travel (the
///   first column early, each further one needing a little more), at most `swing_max_cols`.
///   Without circles the columns come live while the stroke happens; with circles they come
///   when it ends, because only then is it clear that it was not the side of a circle.
/// * When the stroke ends, opposite motion within `swing_return_window_ms`, up to
///   `swing_return_ratio` times the stroke's travel, is the return and moves nothing. The
///   window closes early when the remote rests or the player continues in the stroke's
///   direction (repeated strokes chain naturally).
/// * With circles enabled, a stroke must stay near-horizontal for `swing_confirm_ms` before
///   it moves anything, so the sides of a circle never drag the piece.
#[derive(Debug, Clone, Default)]
pub struct Swing {
    pub phase: Phase,
    pub mode: StrokeMode,
    dir: f32,
    start_us: u64,
    origin_x: f32,
    pub travel: f32,
    moved: i32,
    slow_since: Option<u64>,
    confirmed: bool,
    max_dev: f32,
    peak: f32,
    ret_dir: f32,
    pub ret_budget: f32,
    ret_until: u64,
    ret_still_since: Option<u64>,
    /// Where the aim last rested: a stroke's travel is measured from here, so the slow
    /// beginning of a movement still counts.
    anchor_x: Option<f32>,
    /// Columns earned by the current stroke but held until it ends (circle style).
    pending: i32,
    /// The stroke curved away at speed (part of a circle): no move and no return window.
    turned: bool,
}

impl Swing {
    pub fn reset(&mut self) {
        let mode = self.mode;
        *self = Swing::default();
        self.mode = mode;
    }

    /// Stop an in-flight stroke (another gesture owns the trajectory). Columns already moved
    /// stay; the motion that follows is treated as a return.
    pub fn cancel(&mut self, t_us: u64, c: &Thresholds) {
        if self.phase == Phase::Candidate {
            self.end_stroke(t_us, c);
            self.phase = Phase::Disarmed;
        }
    }

    pub fn is_stroke(&self) -> bool {
        self.phase == Phase::Candidate
    }

    /// A stroke that has already moved the piece: it is a sideways movement, not the start
    /// of a circle.
    pub fn is_moving_stroke(&self) -> bool {
        self.phase == Phase::Candidate && self.moved > 0
    }

    fn return_active(&self, t_us: u64) -> bool {
        self.ret_budget > 0.0 && t_us < self.ret_until
    }

    fn end_stroke(&mut self, t_us: u64, c: &Thresholds) {
        self.ret_dir = -self.dir;
        self.ret_budget = self.travel.max(0.0) * c.swing_return_ratio + 0.015;
        self.ret_until = t_us + (c.swing_return_window_ms * 1000.0) as u64;
        self.ret_still_since = None;
        self.phase = Phase::Latched;
    }

    fn commit(&self, t_us: u64) -> Commit {
        Commit {
            gesture: if self.dir > 0.0 {
                Gesture::MoveRight
            } else {
                Gesture::MoveLeft
            },
            start_us: self.start_us,
            t_us,
            recognizer: match self.mode {
                StrokeMode::Columns => "swing",
                StrokeMode::Single => "flick",
            },
        }
    }

    /// `strict`: circles are enabled. `blocked`: another gesture owns the motion.
    pub fn update(&mut self, f: &Frame, c: &Thresholds, strict: bool, blocked: bool) -> Vec<Commit> {
        let mut out = Vec::new();
        if f.gap {
            self.reset();
            return out;
        }
        let vx = f.vel.x;
        let vy = f.vel.y;
        let speed = f.vel.len();
        let yaw_dps = vx.abs() * c.deg_per_sh;
        // A wrist twist only matters when twisting rotates (not in the circle style, where
        // the roll that naturally comes with a forearm swing must not block it).
        let twisting = !strict
            && f.twist_rate_dps
                .is_some_and(|r| r.abs() > 60.0_f32.max(1.2 * yaw_dps));

        if self.phase != Phase::Candidate && (speed < c.swing_stop_speed || self.anchor_x.is_none()) {
            self.anchor_x = Some(f.aim.x);
        }
        // Return window bookkeeping.
        if self.return_active(f.t_us) {
            if speed < c.swing_stop_speed {
                let s0 = *self.ret_still_since.get_or_insert(f.t_us);
                if (f.t_us - s0) as f32 / 1000.0 >= c.swing_return_still_ms {
                    self.ret_budget = 0.0;
                }
            } else {
                self.ret_still_since = None;
            }
            if vx * self.ret_dir > 0.0 {
                self.ret_budget -= (vx * f.dt_s).abs();
            }
        }
        if self.phase == Phase::Latched && !self.return_active(f.t_us) {
            self.phase = Phase::Idle;
        }

        match self.phase {
            Phase::Idle | Phase::Latched => {
                let starts = vx.abs() > c.swing_start_speed && vx.abs() > c.swing_dominance * vy.abs();
                let dir = sign(vx);
                let is_return = self.return_active(f.t_us) && dir == self.ret_dir;
                if starts && !is_return && !blocked && !twisting {
                    self.phase = Phase::Candidate;
                    self.dir = dir;
                    self.start_us = f.t_us;
                    let back = f.aim.x - vx * f.dt_s;
                    // Use the rest point only if it lies behind the stroke.
                    self.origin_x = match self.anchor_x {
                        Some(a) if (back - a) * dir >= 0.0 && (back - a).abs() < 0.3 => a,
                        _ => back,
                    };
                    self.travel = 0.0;
                    self.moved = 0;
                    self.slow_since = None;
                    self.confirmed = !strict;
                    self.max_dev = 0.0;
                    self.peak = vx.abs();
                    self.ret_budget = 0.0;
                    self.pending = 0;
                    self.turned = false;
                }
            }
            Phase::Candidate => {
                self.travel = self.travel.max((f.aim.x - self.origin_x) * self.dir);
                if speed > c.swing_stop_speed {
                    let dev = vy.abs().atan2((vx * self.dir).max(0.0)).to_degrees();
                    if f.t_us - self.start_us <= (c.swing_confirm_ms * 1000.0) as u64 {
                        self.max_dev = self.max_dev.max(dev);
                    }
                }
                if strict && !self.confirmed {
                    if self.max_dev > c.swing_max_heading_dev_deg {
                        self.phase = Phase::Disarmed;
                        return out;
                    }
                    if (f.t_us - self.start_us) as f32 / 1000.0 >= c.swing_confirm_ms {
                        self.confirmed = true;
                    }
                }
                let along = vx * self.dir;
                self.peak = self.peak.max(along);
                // A swing slows down before it stops or turns back; the side of a circle keeps
                // its speed and curves away.
                if strict
                    && along < c.swing_stop_speed
                    && speed > 0.5 * self.peak
                    && vy.abs() > (0.75 * c.swing_start_speed).max(1.5 * vx.abs())
                {
                    self.turned = true;
                    self.phase = Phase::Disarmed;
                    return out;
                }
                if blocked || (twisting && self.moved == 0) {
                    // A held stroke that has already stopped going sideways was a swing: the
                    // loop of its pull back may look like the start of a circle.
                    if strict && self.pending > 0 && vx * self.dir < c.swing_stop_speed {
                        while self.moved < self.pending {
                            self.moved += 1;
                            out.push(self.commit(f.t_us));
                        }
                        self.end_stroke(f.t_us, c);
                        return out;
                    }
                    self.cancel(f.t_us, c);
                    return out;
                }
                // With circles enabled, the stroke's columns are held until it ends: only then
                // is it clear whether it was a sideways swing (it stops or turns back) or the
                // side of a circle (it curves away at speed).
                let deferred = strict;
                if self.confirmed && self.mode == StrokeMode::Columns {
                    let target = swing_columns(self.travel, c);
                    if deferred {
                        self.pending = self.pending.max(target);
                    } else {
                        while self.moved < target {
                            self.moved += 1;
                            out.push(self.commit(f.t_us));
                        }
                    }
                }
                // Flick: one column as soon as the stroke is clearly past its peak, before any
                // recoil begins.
                if self.mode == StrokeMode::Single
                    && !deferred
                    && self.moved == 0
                    && self.confirmed
                    && self.travel >= c.flick_min_travel_sh
                    && along < 0.6 * self.peak
                {
                    self.moved = 1;
                    out.push(self.commit(f.t_us));
                }
                let reversed = along < -c.swing_stop_speed;
                if along < c.swing_stop_speed {
                    self.slow_since.get_or_insert(f.t_us);
                } else {
                    self.slow_since = None;
                }
                let ended = reversed
                    || self
                        .slow_since
                        .is_some_and(|s0| (f.t_us - s0) as f32 / 1000.0 >= c.swing_end_ms);
                if ended {
                    if deferred && self.confirmed && self.mode == StrokeMode::Columns {
                        while self.moved < self.pending {
                            self.moved += 1;
                            out.push(self.commit(f.t_us));
                        }
                    }
                    if self.mode == StrokeMode::Single
                        && self.moved == 0
                        && self.confirmed
                        && self.travel >= c.flick_min_travel_sh
                    {
                        self.moved = 1;
                        out.push(self.commit(f.t_us));
                    }
                    self.end_stroke(f.t_us, c);
                }
            }
            Phase::Disarmed => {
                // A rejected stroke still has a pull back: keep measuring how far it went so
                // the return can be ignored like any other.
                self.travel = self.travel.max((f.aim.x - self.origin_x) * self.dir);
                if speed < c.rearm_speed {
                    if self.travel > 0.02 && !self.turned && !self.return_active(f.t_us) {
                        self.end_stroke(f.t_us, c);
                    } else {
                        self.phase = if self.return_active(f.t_us) {
                            Phase::Latched
                        } else {
                            Phase::Idle
                        };
                    }
                }
            }
        }
        out
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
                if speed < c.rearm_speed {
                    self.phase = Phase::Idle;
                }
            }
        }
        None
    }
}
