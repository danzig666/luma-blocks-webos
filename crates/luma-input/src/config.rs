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

    // --- Horizontal flick ------------------------------------------------------------
    /// Horizontal aim speed that starts a flick, SH/s.
    pub flick_enter_speed: f32,
    /// Speed below which a flick burst has ended / re-armed, SH/s.
    pub flick_rearm_speed: f32,
    /// Required |vx| / |vy| during the burst.
    pub flick_dominance: f32,
    /// Minimum horizontal travel during the burst, SH.
    pub flick_min_travel_sh: f32,
    /// Longest burst still counted as a flick (slower is a sweep), ms.
    pub flick_max_ms: f32,
    /// Opposite-direction motion within this window after a flick is recoil, ms.
    pub flick_recoil_ms: f32,
    /// Maximum heading change during a flick when circles are enabled, degrees.
    pub flick_max_heading_change_deg: f32,

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
    /// After a drop: stillness needed to re-arm, ms (on top of `drop_min_latch_ms`).
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

            flick_enter_speed: 1.5,
            flick_rearm_speed: 0.45,
            flick_dominance: 1.8,
            flick_min_travel_sh: 0.05,
            flick_max_ms: 280.0,
            flick_recoil_ms: 350.0,
            flick_max_heading_change_deg: 70.0,

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
    #[default]
    Pointing,
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
            horizontal: HorizontalStyle::Pointing,
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
    e.flick_enter_speed /= h;
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
