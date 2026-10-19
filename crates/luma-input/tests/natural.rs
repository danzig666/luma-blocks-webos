//! Natural, imperfect sideways swings (arcs, wrist roll, looping returns, no pause at the far
//! end) in every rotation/sideways combination. Synthetic: guards against regressions, not
//! a measure of real-remote accuracy.

use luma_input::calib::Calibration;
use luma_input::config::{HorizontalStyle, Profile, RotationStyle, Thresholds};
use luma_input::engine::GestureEngine;
use luma_input::sample::MotionSample;
use luma_input::synth::{gestures, Synth};
use luma_input::Gesture;

fn feed(e: &mut GestureEngine, samples: &[MotionSample]) -> Vec<Gesture> {
    let mut out = Vec::new();
    for s in samples {
        e.tick(s.t_us);
        e.motion(s);
        out.extend(e.take().into_iter().map(|c| c.gesture));
    }
    out
}

fn var(seed: u64, k: u64) -> f32 {
    let mut z = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(k.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f32 / (1u64 << 53) as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub struct Outcome {
    pub tries: u32,
    pub rotations: u32,
    pub none: u32,
    pub wrong_way: u32,
    pub off_by_more_than_one: u32,
}

/// Columns a swing of `amp` degrees should move with default thresholds.
pub fn expected_columns(amp: f32) -> i32 {
    let t = Thresholds::default();
    luma_input::recognize::swing_columns(amp / t.deg_per_sh, &t)
}

pub fn run(rot: RotationStyle, n: u64) -> Outcome {
    let p = Profile {
        rotation: rot,
        horizontal: HorizontalStyle::Swing,
        ..Profile::default()
    };
    let mut o = Outcome {
        tries: 0,
        rotations: 0,
        none: 0,
        wrong_way: 0,
        off_by_more_than_one: 0,
    };
    for i in 0..n {
        let r = |k: u64| var(i + 1000, k);
        let hz = [50.0, 60.0, 100.0][(i % 3) as usize];
        let mut e = GestureEngine::new(Thresholds::default(), p, Calibration::default());
        let mut s = Synth::new(hz, 900 + i);
        feed(&mut e, &s.hold(500.0));
        s.tremor_deg = lerp(0.05, 0.4, r(1));
        let dir = if r(2) < 0.5 { -1.0 } else { 1.0 };
        let amp = lerp(3.0, 14.0, r(3));
        let out_ms = lerp(110.0, 280.0, r(4));
        let arc = lerp(-0.2, 0.2, r(5));
        let roll = lerp(0.0, 0.8, r(6));
        let pause = if r(7) < 0.5 { 0.0 } else { lerp(20.0, 200.0, r(8)) };
        let back_frac = lerp(0.6, 1.15, r(9));
        let loop_gap = lerp(-0.25, 0.25, r(10));
        let back_ms = out_ms * lerp(0.7, 1.8, r(11));
        let v = gestures::natural_swing(
            &mut s, dir, amp, out_ms, arc, roll, pause, back_frac, loop_gap, back_ms,
        );
        let got = feed(&mut e, &v);
        let want = if dir > 0.0 {
            Gesture::MoveRight
        } else {
            Gesture::MoveLeft
        };
        let n_ok = got.iter().filter(|g| **g == want).count() as i32;
        let rots = got
            .iter()
            .filter(|g| matches!(g, Gesture::RotateCw | Gesture::RotateCcw))
            .count() as u32;
        let wrong = got
            .iter()
            .filter(|g| matches!(g, Gesture::MoveLeft | Gesture::MoveRight) && **g != want)
            .count() as u32;
        o.tries += 1;
        o.rotations += rots;
        o.wrong_way += wrong;
        if n_ok == 0 {
            o.none += 1;
        } else if (n_ok - expected_columns(amp)).abs() > 1 {
            o.off_by_more_than_one += 1;
        }
        if std::env::var("NATURAL_VERBOSE").is_ok() && (rots > 0 || n_ok == 0 || wrong > 0) {
            println!(
                "#{i} {rot:?} hz {hz} dir {dir:+} amp {amp:.1} out {out_ms:.0} arc {arc:+.2} roll {roll:.2} pause {pause:.0} back {back_frac:.2} loop {loop_gap:.2} -> {got:?}"
            );
        }
    }
    o
}

#[test]
fn natural_swings_move_and_never_rotate() {
    let mut report = Vec::new();
    let mut ok = true;
    for rot in [RotationStyle::Circle, RotationStyle::Twist] {
        let o = run(rot, 200);
        report.push(format!(
            "{rot:?}+Swing: {} swings, rotations {}, no move {}, wrong-way {}, columns off by >1 {}",
            o.tries, o.rotations, o.none, o.wrong_way, o.off_by_more_than_one
        ));
        // A few of the smallest, slowest swings (about 3 degrees) are as slow as drifting and
        // may be ignored.
        ok &= o.rotations == 0 && o.wrong_way == 0 && o.none <= 4 && o.off_by_more_than_one <= 6;
    }
    println!("{}", report.join("\n"));
    assert!(ok, "{}", report.join("\n"));
}
