//! Every gesture threshold in one place, with units and meaning.
//!
//! These are **initial values for tuning**, chosen from synthetic traces and typical wrist
//! motion. They are not universal physical constants; the developer screen edits them and
//! real TV traces should drive their final values.
//!
//! Units: "SH" = screen heights (1.0 = the full screen height), used for every 2D aim
//! quantity so pointer pixels and sensor angles share one scale. "dps" = degrees per second.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    // --- Aim frame -------------------------------------------------------------------
    /// Use the gyroscope for motion (recommended); orientation only when it is missing.
    pub prefer_gyro: bool,
    /// Degrees of remote yaw/pitch that correspond to one screen height of aim movement.
    pub deg_per_sh: f32,
    /// Exponential smoothing time constant for aim velocity, ms.
    pub velocity_smoothing_ms: f32,
    /// Samples older than this are treated as a gap (stream stalled), ms.
    pub max_sample_gap_ms: f32,
    /// After a reset (pause, resume, wake), wait for this much stillness before any gesture
    /// can commit, ms.
    pub rearm_still_ms: f32,
    /// "Still" for re-arming: aim speed below this (SH/s)...
    pub still_speed_sh: f32,
    /// ...and twist rate below this (dps).
    pub still_twist_dps: f32,

    // --- Wrist twist (rotation) ------------------------------------------------------
    /// Twist from the rest baseline that commits a rotation, degrees.
    pub twist_enter_deg: f32,
    /// Twist must come back within this of the baseline before the next rotation, degrees.
    pub twist_rearm_deg: f32,
    /// Peak twist speed required during the gesture (rejects slow posture drift), dps.
    /// Only enforced when the stream provides a rate (gyro or orientation).
    pub twist_min_peak_dps: f32,
    /// Rest-baseline adaptation time constant while idle and still, seconds.
    pub twist_baseline_tau_s: f32,
    /// Holding a twisted wrist this long re-bases the neutral (posture change), ms.
    pub twist_hold_rebase_ms: f32,

    // --- Circle (rotation) -----------------------------------------------------------
    /// Accumulated turning around the trajectory centre to commit, degrees.
    pub circle_min_turn_deg: f32,
    /// Turning at which a circle candidate takes ownership of the trajectory, degrees.
    pub circle_candidate_turn_deg: f32,
    /// Mean radius limits, SH.
    pub circle_min_radius_sh: f32,
    pub circle_max_radius_sh: f32,
    /// Max radius variation (std / mean): rejects scribbles and lines.
    pub circle_max_radius_cv: f32,
    /// Duration limits for one circle, ms.
    pub circle_min_ms: f32,
    pub circle_max_ms: f32,
    /// Max path length relative to an ideal arc of the same turning (scribble rejection).
    pub circle_max_path_ratio: f32,
    /// Min roundness: narrowest over widest spread of the drawn points (1 = round). Keeps
    /// the flat loop of a swing and its pull back from counting as a circle.
    pub circle_min_axis_ratio: f32,

    // --- Sideways strokes (Swing and Flick styles) -----------------------------------
    // A stroke is a quick, horizontally dominant movement of the aim. The pull that brings
    // the remote back afterwards is recognized as a return and ignored (like lifting a mouse
    // off its pad), so the piece never follows the controller back.
    /// Horizontal aim speed that starts a stroke, SH/s. Slower motion never moves a piece.
    pub swing_start_speed: f32,
    /// Below this speed (for `swing_end_ms`) the stroke has ended, SH/s.
    pub swing_stop_speed: f32,
    pub swing_end_ms: f32,
    /// Required |vx| / |vy| when a stroke starts.
    pub swing_dominance: f32,
    /// Aim travel per column in Swing style, SH (the first column needs `swing_first_col` of it).
    pub swing_col_sh: f32,
    pub swing_first_col: f32,
    /// Each further column needs this much more travel than the one before (fraction of
    /// `swing_col_sh`): small swings stay precise, big ones still cross the board.
    pub swing_col_growth: f32,
    /// Most columns one stroke may move.
    pub swing_max_cols: f32,
    /// Opposite motion within this time after a stroke counts as the return, ms...
    pub swing_return_window_ms: f32,
    /// ...up to this multiple of the stroke's own travel.
    pub swing_return_ratio: f32,
    /// Holding still this long ends the return window early, ms.
    pub swing_return_still_ms: f32,
    /// With circles enabled: a stroke must keep its heading within this many degrees of
    /// horizontal for `swing_confirm_ms` before it moves anything.
    pub swing_max_heading_dev_deg: f32,
    pub swing_confirm_ms: f32,
    /// Flick style: minimum travel of a stroke for its one column, SH.
    pub flick_min_travel_sh: f32,
    /// Speed below which a disarmed recognizer re-arms, SH/s.
    pub rearm_speed: f32,

    // --- Downward hard drop ----------------------------------------------------------
    /// Downward aim speed that starts a drop candidate, SH/s.
    pub drop_enter_speed: f32,
    /// Peak downward speed the burst must reach, SH/s.
    pub drop_min_peak_speed: f32,
    /// Minimum downward travel, SH.
    pub drop_min_travel_sh: f32,
    /// Required |vy| / |vx| throughout the burst.
    pub drop_dominance: f32,
    /// Max heading deviation from straight down during the burst, degrees.
    pub drop_max_heading_dev_deg: f32,
    /// Longest burst counted as a drop, ms.
    pub drop_max_ms: f32,
    /// After a drop: time without downward motion needed to re-arm, ms (on top of
    /// `drop_min_latch_ms`).
    pub drop_rearm_still_ms: f32,
    /// Minimum time a drop stays latched, ms.
    pub drop_min_latch_ms: f32,
    /// No drop within this window after a rotation gesture, ms.
    pub drop_after_rotate_block_ms: f32,

    // --- Pointing ----------------------------------------------------------------------
    /// Column hysteresis: the target changes only after the pointer passes the boundary by
    /// this fraction of a column.
    pub point_hysteresis_cols: f32,
    /// Movement needed to re-engage pointing after a button correction, SH.
    pub point_reengage_sh: f32,
    /// Pointing smoothing time constant, ms.
    pub point_smoothing_ms: f32,

    // --- Arbitration ---------------------------------------------------------------------
    /// Gestures ignored after a button press (pressing jiggles the remote), ms.
    pub button_guard_ms: f32,
    /// Pointing frozen after a rotation/drop commit, ms.
    pub post_commit_freeze_ms: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            prefer_gyro: true,
            deg_per_sh: 24.0,
            velocity_smoothing_ms: 22.0,
            max_sample_gap_ms: 150.0,
            rearm_still_ms: 220.0,
            still_speed_sh: 0.30,
            still_twist_dps: 40.0,

            twist_enter_deg: 30.0,
            twist_rearm_deg: 13.0,
            twist_min_peak_dps: 110.0,
            twist_baseline_tau_s: 0.5,
            twist_hold_rebase_ms: 2000.0,

            circle_min_turn_deg: 300.0,
            circle_candidate_turn_deg: 150.0,
            circle_min_radius_sh: 0.035,
            circle_max_radius_sh: 0.45,
            circle_max_radius_cv: 0.42,
            circle_min_ms: 200.0,
            circle_max_ms: 1400.0,
            circle_max_path_ratio: 1.9,
            circle_min_axis_ratio: 0.45,

            swing_start_speed: 0.8,
            swing_stop_speed: 0.3,
            swing_end_ms: 60.0,
            swing_dominance: 1.3,
            swing_col_sh: 0.10,
            swing_first_col: 0.5,
            swing_col_growth: 0.15,
            swing_max_cols: 9.0,
            swing_return_window_ms: 1100.0,
            swing_return_ratio: 1.6,
            swing_return_still_ms: 220.0,
            swing_max_heading_dev_deg: 40.0,
            swing_confirm_ms: 90.0,
            flick_min_travel_sh: 0.04,
            rearm_speed: 0.45,

            drop_enter_speed: 1.8,
            drop_min_peak_speed: 2.6,
            drop_min_travel_sh: 0.10,
            drop_dominance: 2.2,
            drop_max_heading_dev_deg: 32.0,
            drop_max_ms: 320.0,
            drop_rearm_still_ms: 160.0,
            drop_min_latch_ms: 280.0,
            drop_after_rotate_block_ms: 250.0,

            point_hysteresis_cols: 0.2,
            point_reengage_sh: 0.06,
            point_smoothing_ms: 35.0,

            button_guard_ms: 180.0,
            post_commit_freeze_ms: 200.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RotationStyle {
    #[default]
    Twist,
    Circle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HorizontalStyle {
    /// Quick sideways strokes move the piece by as many columns as the stroke is long; the
    /// return pull is ignored. Default: it matches how people naturally move a remote.
    #[default]
    Swing,
    /// The piece follows the column the remote points at (absolute).
    Pointing,
    /// Each sideways flick moves exactly one column; returns ignored.
    Flick,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub rotation: RotationStyle,
    pub horizontal: HorizontalStyle,
    pub gesture_drop: bool,
    /// 0.5 (needs bigger motion) .. 2.0 (reacts to smaller motion); 1.0 = defaults.
    pub sens_horizontal: f32,
    pub sens_rotation: f32,
    pub sens_drop: f32,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            rotation: RotationStyle::Twist,
            horizontal: HorizontalStyle::Swing,
            gesture_drop: true,
            sens_horizontal: 1.0,
            sens_rotation: 1.0,
            sens_drop: 1.0,
        }
    }
}

impl Profile {
    pub fn clamp(&mut self) {
        for s in [
            &mut self.sens_horizontal,
            &mut self.sens_rotation,
            &mut self.sens_drop,
        ] {
            if !s.is_finite() {
                *s = 1.0;
            }
            *s = s.clamp(0.5, 2.0);
        }
    }
}

/// Thresholds with the profile's sensitivities applied: higher sensitivity lowers the
/// motion needed. Timing limits are not scaled.
pub fn effective(t: &Thresholds, p: &Profile) -> Thresholds {
    let mut e = t.clone();
    let h = p.sens_horizontal.clamp(0.5, 2.0);
    let r = p.sens_rotation.clamp(0.5, 2.0);
    let d = p.sens_drop.clamp(0.5, 2.0);
    e.swing_start_speed /= h;
    e.swing_col_sh /= h;
    e.flick_min_travel_sh /= h;
    e.twist_enter_deg /= r;
    e.twist_rearm_deg /= r;
    e.twist_min_peak_dps /= r;
    e.circle_min_radius_sh /= r;
    e.drop_enter_speed /= d;
    e.drop_min_peak_speed /= d;
    e.drop_min_travel_sh /= d;
    e
}
