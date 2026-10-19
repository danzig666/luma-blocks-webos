//! Scenario tests for the gesture layer, driven by synthetic remote motion at realistic
//! sample rates with hand tremor. Synthetic traces cover the logic; they are not evidence
//! of real-remote accuracy (that needs captured TV traces, replayable through the same code).

use luma_input::calib::{Calibration, PointRange};
use luma_input::config::{HorizontalStyle, Profile, RotationStyle, Thresholds};
use luma_input::engine::{target_left_column, GestureEngine};
use luma_input::math::{Quat, Vec2};
use luma_input::sample::MotionSample;
use luma_input::synth::{ease, gestures, Synth};
use luma_input::Gesture;

fn profile(rot: RotationStyle, hor: HorizontalStyle) -> Profile {
    Profile {
        rotation: rot,
        horizontal: hor,
        ..Profile::default()
    }
}

fn engine(p: Profile) -> GestureEngine {
    GestureEngine::new(Thresholds::default(), p, Calibration::default())
}

fn feed(e: &mut GestureEngine, samples: &[MotionSample]) -> Vec<Gesture> {
    let mut out = Vec::new();
    for s in samples {
        e.tick(s.t_us);
        e.motion(s);
        out.extend(e.take().into_iter().map(|c| c.gesture));
    }
    out
}

/// Engine + synth that has settled (armed) at neutral.
fn ready(p: Profile, hz: f32, seed: u64) -> (GestureEngine, Synth) {
    let mut e = engine(p);
    let mut s = Synth::new(hz, seed);
    let warm = s.hold(500.0);
    assert!(feed(&mut e, &warm).is_empty());
    assert!(e.is_armed());
    (e, s)
}

const ALL: [(RotationStyle, HorizontalStyle); 6] = [
    (RotationStyle::Twist, HorizontalStyle::Swing),
    (RotationStyle::Twist, HorizontalStyle::Pointing),
    (RotationStyle::Twist, HorizontalStyle::Flick),
    (RotationStyle::Circle, HorizontalStyle::Swing),
    (RotationStyle::Circle, HorizontalStyle::Pointing),
    (RotationStyle::Circle, HorizontalStyle::Flick),
];

const RATES: [f32; 3] = [50.0, 100.0, 60.0];

// --- Wrist twist -----------------------------------------------------------------------

#[test]
fn twist_both_directions_one_rotation_each_and_return_does_not_undo() {
    for hz in RATES {
        for (dir, want) in [(1.0, Gesture::RotateCw), (-1.0, Gesture::RotateCcw)] {
            for (amp, ms) in [(40.0, 180.0), (55.0, 260.0), (35.0, 150.0)] {
                let (mut e, mut s) = ready(Profile::default(), hz, 11);
                let got = feed(&mut e, &gestures::twist(&mut s, dir, amp, ms));
                assert_eq!(got, vec![want], "hz {hz} dir {dir} amp {amp} ms {ms}");
            }
        }
    }
}

#[test]
fn holding_a_twist_does_not_repeat_and_repeated_twists_count() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 2);
    let mut v = s.motion(200.0, |p| (0.0, 0.0, 45.0 * ease(p)));
    v.extend(s.hold(1500.0));
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCw]);
    let back = s.motion(300.0, |p| (0.0, 0.0, -45.0 * ease(p)));
    assert!(feed(&mut e, &back).is_empty());
    let mut got = Vec::new();
    for _ in 0..3 {
        got.extend(feed(&mut e, &s.hold(250.0)));
        got.extend(feed(&mut e, &gestures::twist(&mut s, 1.0, 45.0, 200.0)));
    }
    assert_eq!(got, vec![Gesture::RotateCw; 3]);
}

#[test]
fn slow_roll_and_small_twists_are_ignored() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 3);
    // 50 degrees over 3 seconds: posture drift.
    let v = s.motion(3000.0, |p| (0.0, 0.0, 50.0 * p));
    assert!(feed(&mut e, &v).is_empty());
    // A 15 degree flick of the wrist is below the threshold.
    let v = gestures::twist(&mut s, 1.0, 15.0, 120.0);
    assert!(feed(&mut e, &v).is_empty());
}

#[test]
fn held_posture_rebases_and_returning_is_not_an_opposite_rotation() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 4);
    let mut v = s.motion(200.0, |p| (0.0, 0.0, -45.0 * ease(p)));
    v.extend(s.hold(2600.0)); // keeps the wrist rotated: becomes the new rest pose
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCcw]);
    let v = s.motion(260.0, |p| (0.0, 0.0, 45.0 * ease(p))); // quick return to old neutral
    assert!(feed(&mut e, &v).is_empty(), "return after rebase must not rotate");
    let v = gestures::twist(&mut s, 1.0, 45.0, 200.0);
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCw]);
}

#[test]
fn twist_handles_angle_wraparound() {
    // Neutral pose already twisted by 170 degrees: a +45 twist crosses +-180.
    let cal = Calibration {
        neutral: Some(Quat::IDENTITY),
        ..Default::default()
    };
    let mut e = GestureEngine::new(Thresholds::default(), Profile::default(), cal);
    let mut s = Synth::new(100.0, 5);
    s.base = (0.0, 0.0, 170.0);
    feed(&mut e, &s.hold(500.0));
    let got = feed(&mut e, &gestures::twist(&mut s, 1.0, 45.0, 200.0));
    assert_eq!(got, vec![Gesture::RotateCw]);
    let got = feed(&mut e, &gestures::twist(&mut s, -1.0, 45.0, 200.0));
    assert_eq!(got, vec![Gesture::RotateCcw]);
}

#[test]
fn twist_does_not_drag_the_pointing_column() {
    let (mut e, mut s) = ready(profile(RotationStyle::Twist, HorizontalStyle::Pointing), 100.0, 6);
    let range = PointRange {
        left: -0.3,
        center: 0.0,
        right: 0.3,
    };
    let before = e.aim_column(10.0, range).unwrap();
    // A sloppy twist that also wobbles the aim by a couple of degrees.
    let v = s.motion(220.0, |p| (2.0 * ease(p), 0.5, 45.0 * ease(p)));
    let mut max_dev: f32 = 0.0;
    for smp in &v {
        e.tick(smp.t_us);
        e.motion(smp);
        let c = e.aim_column(10.0, range).unwrap();
        max_dev = max_dev.max((c - before).abs());
    }
    assert_eq!(e.take().len(), 1);
    assert!(max_dev < 0.35, "column moved {max_dev}");
}

// --- Circles ---------------------------------------------------------------------------

#[test]
fn circle_both_directions_one_rotation_each() {
    let p = profile(RotationStyle::Circle, HorizontalStyle::Pointing);
    for hz in RATES {
        for (dir, want) in [(1.0, Gesture::RotateCw), (-1.0, Gesture::RotateCcw)] {
            for (r, period) in [(3.0, 600.0), (5.0, 800.0), (2.2, 450.0)] {
                let (mut e, mut s) = ready(p, hz, 7);
                let got = feed(&mut e, &gestures::circle(&mut s, dir, r, period, 1.05));
                assert_eq!(got, vec![want], "hz {hz} dir {dir} r {r} period {period}");
            }
        }
    }
}

#[test]
fn imperfect_circle_counts_once_and_one_and_a_half_turns_is_one() {
    let p = profile(RotationStyle::Circle, HorizontalStyle::Pointing);
    let (mut e, mut s) = ready(p, 100.0, 8);
    // Elliptical, slightly drifting circle.
    let v = s.motion(700.0, |q| {
        let a = q * 1.1 * std::f32::consts::TAU;
        (4.5 * (a.cos() - 1.0) + q * 1.0, -2.8 * a.sin(), 0.0)
    });
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCw]);
    let (mut e, mut s) = ready(p, 100.0, 9);
    let v = gestures::circle(&mut s, 1.0, 3.5, 600.0, 1.5);
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCw]);
}

#[test]
fn incomplete_circle_curved_swipe_and_scribble_do_not_rotate() {
    let p = profile(RotationStyle::Circle, HorizontalStyle::Flick);
    let (mut e, mut s) = ready(p, 100.0, 10);
    // 200 degrees of arc.
    let v = gestures::circle(&mut s, 1.0, 3.5, 700.0, 0.55);
    assert!(!feed(&mut e, &v).iter().any(|g| g.is_rotation()));
    // Curved horizontal swipe.
    let v = s.motion(350.0, |q| {
        (12.0 * ease(q), -3.0 * (q * std::f32::consts::PI).sin(), 0.0)
    });
    assert!(!feed(&mut e, &v).iter().any(|g| g.is_rotation()));
    // Jittery scribble.
    let v = s.motion(800.0, |q| {
        let t = q * 40.0;
        (
            2.0 * (t * 1.7).sin() + 1.5 * (t * 4.3).cos(),
            2.0 * (t * 2.9).cos(),
            0.0,
        )
    });
    assert!(!feed(&mut e, &v).iter().any(|g| g.is_rotation()));
}

#[test]
fn circle_does_not_drift_the_piece_and_returns_to_the_column() {
    let p = profile(RotationStyle::Circle, HorizontalStyle::Pointing);
    let (mut e, mut s) = ready(p, 100.0, 11);
    let range = PointRange {
        left: -0.25,
        center: 0.0,
        right: 0.25,
    };
    let before = e.aim_column(10.0, range).unwrap();
    let start_col = target_left_column(before, 3, 10, None, 0.2);
    let mut cur = Some(start_col);
    let mut targets = vec![start_col];
    let v = gestures::circle(&mut s, 1.0, 3.0, 650.0, 1.05);
    let mut got = Vec::new();
    for smp in &v {
        e.tick(smp.t_us);
        e.motion(smp);
        got.extend(e.take().into_iter().map(|c| c.gesture));
        if let Some(u) = e.aim_column(10.0, range) {
            let t = target_left_column(u, 3, 10, cur, 0.2);
            cur = Some(t);
            targets.push(t);
        }
    }
    assert_eq!(got, vec![Gesture::RotateCw]);
    let spread = targets.iter().max().unwrap() - targets.iter().min().unwrap();
    assert!(
        spread <= 1,
        "piece target wandered {spread} columns during the circle"
    );
    assert_eq!(*targets.last().unwrap(), start_col);
}

// --- Flicks ----------------------------------------------------------------------------

#[test]
fn flick_both_directions_with_recoil_is_one_move() {
    for (rot, _) in ALL.iter().filter(|(_, h)| *h == HorizontalStyle::Flick) {
        let p = profile(*rot, HorizontalStyle::Flick);
        for hz in RATES {
            for (dir, want) in [(1.0, Gesture::MoveRight), (-1.0, Gesture::MoveLeft)] {
                for (amp, ms) in [(5.0, 110.0), (8.0, 140.0), (14.0, 160.0)] {
                    let (mut e, mut s) = ready(p, hz, 12);
                    let got = feed(&mut e, &gestures::flick(&mut s, dir, amp, ms));
                    assert_eq!(got, vec![want], "{rot:?} hz {hz} dir {dir} amp {amp}");
                }
            }
        }
    }
}

#[test]
fn repeated_flicks_move_repeatedly() {
    let p = profile(RotationStyle::Twist, HorizontalStyle::Flick);
    let (mut e, mut s) = ready(p, 100.0, 13);
    let mut got = Vec::new();
    for _ in 0..4 {
        got.extend(feed(&mut e, &gestures::flick(&mut s, 1.0, 6.0, 120.0)));
    }
    assert_eq!(got, vec![Gesture::MoveRight; 4]);
}

#[test]
fn slow_sweeps_diagonals_and_idle_noise_do_nothing() {
    for (rot, hor) in ALL {
        let p = profile(rot, hor);
        let (mut e, mut s) = ready(p, 100.0, 14);
        let mut v = gestures::idle(&mut s, 3000.0);
        v.extend(s.motion(1500.0, |q| (15.0 * ease(q), 0.0, 0.0))); // slow pan right
        v.extend(s.motion(1500.0, |q| (0.0, -10.0 * ease(q), 0.0))); // slow aim down
        v.extend(s.motion(180.0, |q| (6.0 * ease(q), -6.0 * ease(q), 0.0))); // fast diagonal
        v.extend(s.hold(400.0));
        let got = feed(&mut e, &v);
        assert!(got.is_empty(), "{rot:?}/{hor:?}: {got:?}");
    }
}

// --- Hard drop -------------------------------------------------------------------------

#[test]
fn downward_flick_drops_exactly_once_including_recoil_and_next_spawn() {
    for (rot, hor) in ALL {
        let p = profile(rot, hor);
        for hz in RATES {
            for (amp, ms) in [(12.0, 140.0), (18.0, 170.0), (25.0, 200.0)] {
                let (mut e, mut s) = ready(p, hz, 15);
                let got = feed(&mut e, &gestures::drop(&mut s, amp, ms));
                assert_eq!(got, vec![Gesture::HardDrop], "{rot:?}/{hor:?} hz {hz} amp {amp}");
            }
        }
    }
}

#[test]
fn bouncy_recoil_after_drop_does_not_drop_again() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 16);
    let mut v = s.motion(150.0, |p| (0.0, -18.0 * ease(p), 0.0));
    // Big rebound up and a second downward swing within the latch (no stillness yet).
    v.extend(s.motion(150.0, |p| (0.0, 14.0 * ease(p), 0.0)));
    v.extend(s.motion(150.0, |p| (0.0, -12.0 * ease(p), 0.0)));
    v.extend(s.hold(400.0));
    assert_eq!(feed(&mut e, &v), vec![Gesture::HardDrop]);
}

#[test]
fn no_drop_from_a_circle_descending_arc() {
    for hor in [HorizontalStyle::Pointing, HorizontalStyle::Flick] {
        let p = profile(RotationStyle::Circle, hor);
        for (r, period) in [(3.0, 500.0), (6.0, 600.0), (8.0, 700.0)] {
            let (mut e, mut s) = ready(p, 100.0, 17);
            let got = feed(&mut e, &gestures::circle(&mut s, 1.0, r, period, 1.05));
            assert!(
                !got.contains(&Gesture::HardDrop),
                "r {r} period {period}: {got:?}"
            );
            assert!(!got.contains(&Gesture::MoveLeft) && !got.contains(&Gesture::MoveRight));
        }
    }
}

#[test]
fn slow_downward_pointing_and_twist_do_not_drop() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 18);
    let v = s.motion(900.0, |p| (0.0, -20.0 * ease(p), 0.0));
    assert!(feed(&mut e, &v).is_empty());
    // A twist with a slight downward dip: one rotation, no drop.
    let v = s.motion(200.0, |p| (0.0, -3.0 * ease(p), 45.0 * ease(p)));
    let got = feed(&mut e, &v);
    assert_eq!(got, vec![Gesture::RotateCw]);
}

#[test]
fn gesture_drop_can_be_disabled() {
    let p = Profile {
        gesture_drop: false,
        ..Profile::default()
    };
    let (mut e, mut s) = ready(p, 100.0, 19);
    assert!(feed(&mut e, &gestures::drop(&mut s, 18.0, 160.0)).is_empty());
}

// --- Profiles, mixed controls ----------------------------------------------------------

/// The sideways gesture of each style and what it should produce.
fn sideways(hor: HorizontalStyle, s: &mut Synth, dir: f32) -> (Vec<MotionSample>, Vec<Gesture>) {
    let g = if dir > 0.0 {
        Gesture::MoveRight
    } else {
        Gesture::MoveLeft
    };
    match hor {
        // 8 degrees at 24 deg/SH = 0.333 SH: columns at 0.05, 0.15 and 0.265 SH, so 3.
        HorizontalStyle::Swing => (gestures::swing(s, dir, 8.0, 160.0, 1.0), vec![g; 3]),
        HorizontalStyle::Flick => (gestures::flick(s, dir, 7.0, 120.0), vec![g]),
        HorizontalStyle::Pointing => (Vec::new(), Vec::new()),
    }
}

#[test]
fn all_six_profiles_run_a_mixed_sequence() {
    for (rot, hor) in ALL {
        let p = profile(rot, hor);
        let (mut e, mut s) = ready(p, 100.0, 20);
        let mut got = Vec::new();
        let mut want = Vec::new();
        let rotate = |s: &mut Synth, dir: f32| match rot {
            RotationStyle::Twist => gestures::twist(s, dir, 45.0, 200.0),
            RotationStyle::Circle => gestures::circle(s, dir, 3.5, 600.0, 1.05),
        };
        got.extend(feed(&mut e, &rotate(&mut s, 1.0)));
        want.push(Gesture::RotateCw);
        let (v, w) = sideways(hor, &mut s, -1.0);
        got.extend(feed(&mut e, &v));
        want.extend(w);
        got.extend(feed(&mut e, &rotate(&mut s, -1.0)));
        want.push(Gesture::RotateCcw);
        got.extend(feed(&mut e, &gestures::drop(&mut s, 18.0, 160.0)));
        want.push(Gesture::HardDrop);
        let (v, w) = sideways(hor, &mut s, 1.0);
        got.extend(feed(&mut e, &v));
        want.extend(w);
        assert_eq!(got, want, "{rot:?}/{hor:?}");
    }
}

// --- Swing: sideways strokes with the return pull ignored -----------------------------

#[test]
fn swing_moves_proportionally_and_the_pull_back_is_ignored() {
    for rot in [RotationStyle::Twist, RotationStyle::Circle] {
        let p = profile(rot, HorizontalStyle::Swing);
        for hz in RATES {
            for (dir, g) in [(1.0, Gesture::MoveRight), (-1.0, Gesture::MoveLeft)] {
                // degrees -> columns: the first at 1.2 degrees, then 3.6, 6.4, 9.5, 13, 16.8,
                // 21 degrees (each column a little wider than the one before)
                for (amp, cols, ms, back) in [
                    (4.0, 2, 120.0, 1.0),
                    (8.0, 3, 160.0, 1.4),
                    (14.0, 5, 220.0, 0.8),
                    (20.0, 6, 260.0, 1.8),
                ] {
                    let (mut e, mut s) = ready(p, hz, 31);
                    let got = feed(&mut e, &gestures::swing(&mut s, dir, amp, ms, back));
                    assert_eq!(
                        got,
                        vec![g; cols],
                        "{rot:?} hz {hz} dir {dir} amp {amp} back {back}"
                    );
                }
            }
        }
    }
}

#[test]
fn repeated_swings_ratchet_in_one_direction() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 32);
    let mut got = Vec::new();
    for _ in 0..3 {
        got.extend(feed(&mut e, &gestures::swing(&mut s, 1.0, 8.0, 160.0, 1.2)));
    }
    assert_eq!(got, vec![Gesture::MoveRight; 9]);
}

#[test]
fn chained_strokes_without_returning_keep_moving() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 33);
    let mut v = s.motion(150.0, |p| (6.0 * ease(p), 0.0, 0.0));
    v.extend(s.hold(150.0));
    v.extend(s.motion(150.0, |p| (6.0 * ease(p), 0.0, 0.0)));
    v.extend(s.hold(400.0));
    let got = feed(&mut e, &v);
    assert!(
        got.len() >= 4 && got.iter().all(|g| *g == Gesture::MoveRight),
        "{got:?}"
    );
}

#[test]
fn a_deliberate_opposite_swing_after_resting_moves_back() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 34);
    let mut got = feed(&mut e, &gestures::swing(&mut s, 1.0, 8.0, 160.0, 1.0));
    got.extend(feed(&mut e, &s.hold(300.0)));
    got.extend(feed(&mut e, &gestures::swing(&mut s, -1.0, 4.0, 120.0, 1.0)));
    let mut want = vec![Gesture::MoveRight; 3];
    want.extend([Gesture::MoveLeft; 2]);
    assert_eq!(got, want);
}

#[test]
fn slow_drift_back_to_centre_never_moves() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 35);
    let got = feed(&mut e, &s.motion(1500.0, |p| (-15.0 * ease(p), 0.0, 0.0)));
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn twisting_does_not_swing() {
    for hor in [HorizontalStyle::Swing, HorizontalStyle::Flick] {
        let (mut e, mut s) = ready(profile(RotationStyle::Twist, hor), 100.0, 36);
        let v = s.motion(200.0, |p| (3.0 * ease(p), 0.5 * ease(p), 45.0 * ease(p)));
        let mut got = feed(&mut e, &v);
        got.extend(feed(&mut e, &s.hold(200.0)));
        got.extend(feed(
            &mut e,
            &s.motion(300.0, |p| (-3.0 * ease(p), -0.5 * ease(p), -45.0 * ease(p))),
        ));
        got.extend(feed(&mut e, &s.hold(300.0)));
        assert_eq!(got, vec![Gesture::RotateCw], "{hor:?}");
    }
}

#[test]
fn button_press_jiggle_is_not_a_gesture() {
    let (mut e, mut s) = ready(profile(RotationStyle::Twist, HorizontalStyle::Flick), 100.0, 21);
    let v = s.motion(90.0, |p| (4.0 * ease(p), -4.0 * ease(p), 12.0 * ease(p)));
    e.button(v[0].t_us);
    assert!(feed(&mut e, &v).is_empty());
    // A fast flick right at the press is still suppressed by the guard...
    let (mut e, mut s) = ready(profile(RotationStyle::Twist, HorizontalStyle::Flick), 100.0, 22);
    let v = gestures::flick(&mut s, 1.0, 8.0, 100.0);
    e.button(v[0].t_us);
    assert!(feed(&mut e, &v).is_empty());
    // ...and gestures work again afterwards.
    assert_eq!(
        feed(&mut e, &gestures::flick(&mut s, 1.0, 8.0, 120.0)),
        vec![Gesture::MoveRight]
    );
}

#[test]
fn reset_requires_stillness_before_new_gestures() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 23);
    // Mid-twist the game pauses (or focus is lost); on resume the wrist is still moving.
    let first = s.motion(100.0, |p| (0.0, 0.0, 20.0 * p));
    feed(&mut e, &first);
    e.reset();
    let rest = s.motion(150.0, |p| (0.0, 0.0, 30.0 * ease(p)));
    assert!(
        feed(&mut e, &rest).is_empty(),
        "stale motion must not commit after reset"
    );
    feed(&mut e, &s.hold(400.0));
    assert!(e.is_armed());
}

#[test]
fn sample_gap_mid_gesture_does_not_commit_and_recovers() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 24);
    let v = s.motion(220.0, |p| (0.0, 0.0, 45.0 * ease(p)));
    // Deliver only the first third, then a 400 ms gap (remote asleep / radio loss).
    let part: Vec<MotionSample> = v.iter().take(7).copied().collect();
    feed(&mut e, &part);
    let mut later: Vec<MotionSample> = v.iter().skip(7).copied().collect();
    for smp in &mut later {
        smp.t_us += 400_000;
    }
    e.tick(later[0].t_us - 1);
    let got = feed(&mut e, &later);
    assert!(got.is_empty(), "{got:?}");
    s.t_us += 400_000;
    feed(&mut e, &s.motion(300.0, |p| (0.0, 0.0, -45.0 * ease(p))));
    feed(&mut e, &s.hold(500.0));
    assert_eq!(
        feed(&mut e, &gestures::twist(&mut s, 1.0, 45.0, 200.0)),
        vec![Gesture::RotateCw]
    );
}

#[test]
fn invalid_samples_are_ignored_not_zeroed() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 25);
    let mut v = gestures::twist(&mut s, 1.0, 45.0, 200.0);
    for (i, smp) in v.iter_mut().enumerate() {
        if i % 5 == 2 {
            smp.quat = Some(Quat::new(f32::NAN, 0.0, 0.0, 0.0));
            *smp = smp.sanitized();
            smp.gyro = None;
        }
    }
    assert_eq!(feed(&mut e, &v), vec![Gesture::RotateCw]);
}

#[test]
fn motion_off_mode_reports_nothing() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 26);
    e.enabled = false;
    assert!(feed(&mut e, &gestures::twist(&mut s, 1.0, 45.0, 200.0)).is_empty());
    assert!(!e.debug.suppressed.is_empty());
}

// --- Pointer-only fallback -------------------------------------------------------------

fn pointer_feed(e: &mut GestureEngine, pts: &[(u64, Vec2)]) -> Vec<Gesture> {
    let mut out = Vec::new();
    for (t, p) in pts {
        e.tick(*t);
        e.pointer(*t, *p);
        out.extend(e.take().into_iter().map(|c| c.gesture));
    }
    out
}

fn path(t0: u64, ms: f32, hz: f32, f: impl Fn(f32) -> Vec2) -> Vec<(u64, Vec2)> {
    let n = (ms / 1000.0 * hz) as usize;
    (1..=n)
        .map(|k| {
            let p = k as f32 / n as f32;
            (t0 + (k as f32 * 1e6 / hz) as u64, f(p))
        })
        .collect()
}

#[test]
fn pointer_fallback_supports_circle_flick_and_drop_but_never_twist() {
    let p = profile(RotationStyle::Circle, HorizontalStyle::Flick);
    let mut e = engine(p);
    let c = Vec2::new(0.9, 0.5);
    let mut t = 1_000_000;
    let mut all = Vec::new();
    let still = path(t, 400.0, 60.0, |_| c);
    t += 400_000;
    all.extend(pointer_feed(&mut e, &still));
    assert!(e.is_armed());
    let circle = path(t, 650.0, 60.0, |q| {
        let a = q * 1.05 * std::f32::consts::TAU;
        Vec2::new(c.x + 0.12 * (a.cos() - 1.0), c.y + 0.12 * a.sin())
    });
    t += 650_000;
    all.extend(pointer_feed(&mut e, &circle));
    let settle = path(t, 400.0, 60.0, |_| c);
    t += 400_000;
    all.extend(pointer_feed(&mut e, &settle));
    let flick = path(t, 120.0, 60.0, |q| Vec2::new(c.x + 0.25 * ease(q), c.y));
    t += 120_000;
    all.extend(pointer_feed(&mut e, &flick));
    let rest = path(t, 500.0, 60.0, |_| Vec2::new(c.x + 0.25, c.y));
    t += 500_000;
    all.extend(pointer_feed(&mut e, &rest));
    let drop = path(t, 150.0, 60.0, |q| Vec2::new(c.x + 0.25, c.y + 0.4 * ease(q)));
    t += 150_000;
    all.extend(pointer_feed(&mut e, &drop));
    all.extend(pointer_feed(
        &mut e,
        &path(t, 500.0, 60.0, |_| Vec2::new(c.x + 0.25, c.y + 0.4)),
    ));
    assert_eq!(
        all,
        vec![Gesture::RotateCw, Gesture::MoveRight, Gesture::HardDrop]
    );

    // Twist profile with only a pointer: no rotation can ever come from pointer motion.
    let mut e = engine(Profile::default());
    let circle = path(1_000_000, 650.0, 60.0, |q| {
        let a = q * 1.05 * std::f32::consts::TAU;
        Vec2::new(c.x + 0.12 * (a.cos() - 1.0), c.y + 0.12 * a.sin())
    });
    assert!(!pointer_feed(&mut e, &circle).iter().any(|g| g.is_rotation()));
}

#[test]
fn pointing_near_a_boundary_does_not_flicker() {
    let (mut e, mut s) = ready(
        profile(RotationStyle::Twist, HorizontalStyle::Pointing),
        100.0,
        27,
    );
    let range = PointRange {
        left: -0.3,
        center: 0.0,
        right: 0.3,
    };
    // Aim exactly at a centre boundary for a 3-wide piece (u = 5.0) with heavy tremor.
    // u = 5.0 corresponds to aim x = 0.0.
    s.tremor_deg = 0.5;
    let mut cur = None;
    let mut changes = 0;
    let v = s.hold(3000.0);
    for smp in &v {
        e.tick(smp.t_us);
        e.motion(smp);
        if let Some(u) = e.aim_column(10.0, range) {
            let t = target_left_column(u, 3, 10, cur, Thresholds::default().point_hysteresis_cols);
            if cur.is_some() && cur != Some(t) {
                changes += 1;
            }
            cur = Some(t);
        }
    }
    assert!(changes <= 1, "target flickered {changes} times");
    // A deliberate move across the board is followed.
    let v = s.motion(500.0, |p| (6.0 * ease(p), 0.0, 0.0));
    for smp in &v {
        e.tick(smp.t_us);
        e.motion(smp);
    }
    feed(&mut e, &s.hold(200.0));
    let u = e.aim_column(10.0, range).unwrap();
    assert!(u > 7.5, "u {u}");
}

#[test]
fn button_correction_suspends_pointing_until_a_new_movement() {
    let (mut e, mut s) = ready(
        profile(RotationStyle::Twist, HorizontalStyle::Pointing),
        100.0,
        28,
    );
    let range = PointRange {
        left: -0.3,
        center: 0.0,
        right: 0.3,
    };
    assert!(e.aim_column(10.0, range).is_some());
    e.suspend_pointing();
    feed(&mut e, &gestures::idle(&mut s, 1000.0));
    assert!(
        e.aim_column(10.0, range).is_none(),
        "static pointer must not undo the button move"
    );
    feed(&mut e, &s.motion(400.0, |p| (4.0 * ease(p), 0.0, 0.0)));
    assert!(e.aim_column(10.0, range).is_some());
}

// --- Variation: many natural attempts ----------------------------------------------------

/// Deterministic variation in 0..1.
fn var(seed: u64, k: u64) -> f32 {
    let mut z = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(k.wrapping_mul(1442695040888963407));
    z ^= z >> 33;
    z = z.wrapping_mul(0xff51afd7ed558ccd);
    z ^= z >> 33;
    (z % 10_000) as f32 / 10_000.0
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// 20 varied attempts per gesture and direction (amplitude, speed, tremor, rate, slight
/// off-axis wobble). The synthetic acceptance bar mirrors the hardware target (19/20) but is
/// only a regression guard: real accuracy must be measured with captured TV traces.
#[test]
fn twenty_varied_attempts_per_gesture() {
    let mut report = Vec::new();
    for (rot, hor) in ALL {
        let p = profile(rot, hor);
        let kinds: Vec<(&str, Gesture)> = {
            let mut k = vec![
                ("rot+", Gesture::RotateCw),
                ("rot-", Gesture::RotateCcw),
                ("drop", Gesture::HardDrop),
            ];
            if hor == HorizontalStyle::Flick {
                k.push(("flick+", Gesture::MoveRight));
                k.push(("flick-", Gesture::MoveLeft));
            }
            k
        };
        for (name, want) in kinds {
            let mut ok = 0;
            let mut extra = 0;
            for i in 0..20u64 {
                let r = |k: u64| var(i * 31 + 7, k);
                let hz = [50.0, 60.0, 100.0][(i % 3) as usize];
                let (mut e, mut s) = ready(p, hz, 100 + i);
                s.tremor_deg = lerp(0.05, 0.45, r(1));
                let wob = lerp(-1.5, 1.5, r(2));
                let v = match want {
                    Gesture::RotateCw | Gesture::RotateCcw => {
                        let dir = if want == Gesture::RotateCw { 1.0 } else { -1.0 };
                        match rot {
                            RotationStyle::Twist => {
                                let amp = lerp(36.0, 70.0, r(3));
                                let ms = lerp(140.0, 300.0, r(4));
                                let mut v = s.motion(ms, |q| {
                                    (wob * ease(q), 0.5 * wob * ease(q), dir * amp * ease(q))
                                });
                                v.extend(s.hold(lerp(80.0, 400.0, r(5))));
                                v.extend(s.motion(ms * lerp(1.0, 2.0, r(6)), |q| {
                                    (-wob * ease(q), -0.5 * wob * ease(q), -dir * amp * ease(q))
                                }));
                                v.extend(s.hold(300.0));
                                v
                            }
                            RotationStyle::Circle => {
                                let rx = lerp(2.2, 6.0, r(3));
                                let ry = rx * lerp(0.7, 1.3, r(4));
                                let period = lerp(450.0, 1000.0, r(5));
                                let turns = lerp(1.0, 1.3, r(6));
                                let mut v = s.motion(period * turns, |q| {
                                    let a = dir * q * turns * std::f32::consts::TAU;
                                    (rx * (a.cos() - 1.0) + wob * q, -ry * a.sin(), 0.0)
                                });
                                v.extend(s.hold(300.0));
                                v
                            }
                        }
                    }
                    Gesture::HardDrop => {
                        let amp = lerp(11.0, 28.0, r(3));
                        let ms = lerp(120.0, 220.0, r(4));
                        let mut v = s.motion(ms, |q| (0.25 * wob * ease(q), -amp * ease(q), 0.0));
                        v.extend(s.motion(lerp(150.0, 300.0, r(5)), |q| {
                            (0.0, amp * lerp(0.3, 0.9, r(6)) * ease(q), 0.0)
                        }));
                        v.extend(s.hold(400.0));
                        v
                    }
                    Gesture::MoveLeft | Gesture::MoveRight => {
                        let dir = if want == Gesture::MoveRight { 1.0 } else { -1.0 };
                        let amp = lerp(4.5, 14.0, r(3));
                        let ms = lerp(90.0, 180.0, r(4));
                        let mut v = s.motion(ms, |q| (dir * amp * ease(q), 0.15 * wob * ease(q), 0.0));
                        v.extend(s.motion(lerp(120.0, 260.0, r(5)), |q| {
                            (-dir * amp * lerp(0.1, 0.5, r(6)) * ease(q), 0.0, 0.0)
                        }));
                        v.extend(s.hold(300.0));
                        v
                    }
                };
                let got = feed(&mut e, &v);
                if got == vec![want] {
                    ok += 1;
                } else if std::env::var("GESTURE_VERBOSE").is_ok() {
                    println!("{rot:?}/{hor:?} {name} #{i} hz {hz} wob {wob:.2}: {got:?}");
                }
                extra += got.iter().filter(|g| **g != want).count()
                    + got
                        .len()
                        .saturating_sub(1)
                        .min(got.iter().filter(|g| **g == want).count().saturating_sub(1));
            }
            report.push(format!("{rot:?}/{hor:?} {name}: {ok}/20, wrong {extra}"));
            assert!(ok >= 19 && extra == 0, "{}", report.join("\n"));
        }
    }
    println!("{}", report.join("\n"));
}

/// A long mixed session of rotations, sideways moves, pointing sweeps and idle time must
/// never produce a hard drop that was not asked for.
#[test]
fn long_mixed_session_has_no_accidental_drops() {
    for (rot, hor) in ALL {
        let p = profile(rot, hor);
        let (mut e, mut s) = ready(p, 100.0, 900);
        let mut drops = 0;
        for i in 0..120u64 {
            let r = |k: u64| var(i * 13 + 1, k);
            let v = match i % 5 {
                0 => match rot {
                    RotationStyle::Twist => gestures::twist(
                        &mut s,
                        if r(1) > 0.5 { 1.0 } else { -1.0 },
                        lerp(35.0, 60.0, r(2)),
                        lerp(150.0, 280.0, r(3)),
                    ),
                    RotationStyle::Circle => gestures::circle(
                        &mut s,
                        if r(1) > 0.5 { 1.0 } else { -1.0 },
                        lerp(2.5, 7.0, r(2)),
                        lerp(500.0, 900.0, r(3)),
                        1.1,
                    ),
                },
                1 => gestures::flick(
                    &mut s,
                    if r(1) > 0.5 { 1.0 } else { -1.0 },
                    lerp(5.0, 12.0, r(2)),
                    lerp(100.0, 170.0, r(3)),
                ),
                2 => s.motion(lerp(600.0, 1500.0, r(1)), |q| {
                    (
                        lerp(-10.0, 10.0, r(2)) * ease(q),
                        lerp(-8.0, 3.0, r(3)) * ease(q),
                        0.0,
                    )
                }),
                3 => gestures::idle(&mut s, lerp(500.0, 3000.0, r(1))),
                _ => {
                    // Return toward the centre at a natural pace.
                    let (y, pch, _) = s.base;
                    s.motion(700.0, move |q| (-y * ease(q), -pch * ease(q), 0.0))
                }
            };
            drops += feed(&mut e, &v)
                .iter()
                .filter(|g| **g == Gesture::HardDrop)
                .count();
        }
        assert_eq!(drops, 0, "{rot:?}/{hor:?}");
    }
}

/// Swing: 20 varied natural strokes per direction (size, speed, return speed and size,
/// tremor, vertical wobble) in both rotation styles. Every stroke must move the right way by
/// about its length (within one column) and the pull back must never move the piece.
#[test]
fn twenty_varied_swings_per_direction() {
    let mut report = Vec::new();
    for rot in [RotationStyle::Twist, RotationStyle::Circle] {
        let p = profile(rot, HorizontalStyle::Swing);
        for (dir, want) in [(1.0, Gesture::MoveRight), (-1.0, Gesture::MoveLeft)] {
            let mut ok = 0;
            let mut wrong = 0;
            for i in 0..20u64 {
                let r = |k: u64| var(i * 17 + 3, k);
                let hz = [50.0, 60.0, 100.0][(i % 3) as usize];
                let (mut e, mut s) = ready(p, hz, 200 + i);
                s.tremor_deg = lerp(0.05, 0.45, r(1));
                let amp = lerp(3.5, 16.0, r(2));
                let ms = lerp(110.0, 260.0, r(3));
                let wob = lerp(-1.0, 1.0, r(4));
                let back_frac = lerp(0.6, 1.2, r(5));
                let back_ms = ms * lerp(0.6, 1.6, r(6));
                let mut v = s.motion(ms, |q| (dir * amp * ease(q), wob * ease(q), 0.0));
                v.extend(s.hold(lerp(60.0, 300.0, r(7))));
                v.extend(s.motion(back_ms, |q| {
                    (-dir * amp * back_frac * ease(q), -wob * ease(q), 0.0)
                }));
                v.extend(s.hold(300.0));
                let got = feed(&mut e, &v);
                let n = got.iter().filter(|g| **g == want).count() as i32;
                let bad = got.len() as i32 - n;
                let t = Thresholds::default();
                let expect = luma_input::recognize::swing_columns(amp / t.deg_per_sh, &t);
                if bad == 0 && (n - expect).abs() <= 1 && n >= 1 {
                    ok += 1;
                }
                wrong += bad;
            }
            report.push(format!("{rot:?} swing {dir:+}: {ok}/20, wrong-way moves {wrong}"));
            assert!(ok >= 19 && wrong == 0, "{}", report.join("\n"));
        }
    }
    println!("{}", report.join("\n"));
}

// --- Robustness to firmware differences ----------------------------------------------

#[test]
fn gyro_reported_in_degrees_is_detected_and_still_works() {
    let (mut e, mut s) = ready(Profile::default(), 100.0, 40);
    let deg = |v: Vec<MotionSample>| -> Vec<MotionSample> {
        v.into_iter()
            .map(|mut m| {
                m.gyro = m.gyro.map(|g| g.scale(57.29578));
                m
            })
            .collect()
    };
    let mut got = feed(&mut e, &deg(gestures::twist(&mut s, 1.0, 45.0, 200.0)));
    got.extend(feed(&mut e, &deg(gestures::twist(&mut s, -1.0, 45.0, 200.0))));
    let (scale, checked) = e.gyro_units();
    assert!(
        checked && (scale - 1.0).abs() < 1e-3,
        "detected {scale} {checked}"
    );
    // The first twist may run before the units were confirmed; from then on it is exact.
    got.extend(feed(&mut e, &deg(gestures::swing(&mut s, 1.0, 8.0, 160.0, 1.0))));
    assert!(got.ends_with(&[Gesture::MoveRight; 3]), "{got:?}");
    assert!(got.contains(&Gesture::RotateCcw));
}

#[test]
fn gyro_only_and_orientation_only_streams_both_work() {
    // Gyro only.
    let (mut e, mut s) = ready(Profile::default(), 100.0, 41);
    let strip_q = |v: Vec<MotionSample>| -> Vec<MotionSample> {
        v.into_iter()
            .map(|mut m| {
                m.quat = None;
                m
            })
            .collect()
    };
    let mut got = feed(&mut e, &strip_q(gestures::twist(&mut s, 1.0, 45.0, 200.0)));
    got.extend(feed(
        &mut e,
        &strip_q(gestures::swing(&mut s, -1.0, 8.0, 160.0, 1.0)),
    ));
    got.extend(feed(&mut e, &strip_q(gestures::drop(&mut s, 18.0, 160.0))));
    let mut want = vec![Gesture::RotateCw];
    want.extend([Gesture::MoveLeft; 3]);
    want.push(Gesture::HardDrop);
    assert_eq!(got, want, "gyro only");
    // Orientation only.
    let (mut e, mut s) = ready(Profile::default(), 100.0, 42);
    let strip_g = |v: Vec<MotionSample>| -> Vec<MotionSample> {
        v.into_iter()
            .map(|mut m| {
                m.gyro = None;
                m
            })
            .collect()
    };
    let mut got = feed(&mut e, &strip_g(gestures::twist(&mut s, -1.0, 45.0, 200.0)));
    got.extend(feed(
        &mut e,
        &strip_g(gestures::swing(&mut s, 1.0, 8.0, 160.0, 1.0)),
    ));
    let mut want = vec![Gesture::RotateCcw];
    want.extend([Gesture::MoveRight; 3]);
    assert_eq!(got, want, "orientation only");
}

// --- Orientation convention ------------------------------------------------------------

/// Some firmware may report the rotation vector in the opposite convention (the inverse
/// rotation). Gyro-based gestures are unaffected; absolute pointing must still go the way the
/// remote moves once a little motion has shown how the two relate.
#[test]
fn pointing_follows_the_remote_even_with_an_inverted_orientation() {
    for invert in [false, true] {
        let p = profile(RotationStyle::Twist, HorizontalStyle::Pointing);
        let mut e = engine(p);
        let mut s = Synth::new(100.0, 41);
        let fix = |v: Vec<MotionSample>| -> Vec<MotionSample> {
            v.into_iter()
                .map(|mut m| {
                    if invert {
                        m.quat = m.quat.map(|q| q.conj());
                    }
                    m
                })
                .collect()
        };
        feed(&mut e, &fix(s.hold(500.0)));
        // Ordinary aiming around the board: about 40 degrees of yaw and some pitch.
        feed(
            &mut e,
            &fix(s.motion(1200.0, |q| (6.0 * (q * 12.0).sin(), 3.0 * (q * 9.0).sin(), 0.0))),
        );
        feed(&mut e, &fix(s.hold(400.0)));
        let range = PointRange {
            left: -0.3,
            center: 0.0,
            right: 0.3,
        };
        let centre = e.aim_column(10.0, range).unwrap();
        feed(&mut e, &fix(s.motion(600.0, |q| (5.0 * ease(q), 0.0, 0.0))));
        feed(&mut e, &fix(s.hold(300.0)));
        let right = e.aim_column(10.0, range).unwrap();
        assert!(right > centre + 1.0, "invert {invert}: {centre} -> {right}");
        let f = e.debug.last_frame;
        let up = f.abs.unwrap().y;
        feed(&mut e, &fix(s.motion(600.0, |q| (0.0, 4.0 * ease(q), 0.0))));
        feed(&mut e, &fix(s.hold(300.0)));
        let f = e.debug.last_frame;
        assert!(
            f.abs.unwrap().y < up - 0.05,
            "invert {invert}: tip up must aim higher"
        );
    }
}
