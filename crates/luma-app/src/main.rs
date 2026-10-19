//! Luma Blocks: a minimalist falling-block game for LG webOS TVs with Magic Remote motion
//! controls (native Rust, SDL2 renderer on OpenGL ES 2).
//!
//! Command line:
//!   luma-blocks                      run (TV: real sensor; desktop: simulated sensor)
//!   luma-blocks --self-test          non-graphical checks (CI under qemu-arm, or over SSH)
//!   luma-blocks --analyze-trace F [--profile twist-swing|twist-point|twist-flick|circle-swing|...]
//!                                    replay a recorded trace through the gesture engine
//!   luma-blocks --replay F           run the app with motion played back from a trace
//!   luma-blocks --shots DIR          render documentation screenshots (desktop)
//!   luma-blocks --flow-test          drive the app state machine headless (desktop CI)
//!   luma-blocks --motion-probe [S]   connect to the remote's motion sensor for S seconds
//!                                    (default 10) and print what happens (TV, over SSH)

mod app;
mod audio;
mod diag;
mod gfx;
mod illus;
mod keys;
mod luna;
mod motion;
mod platform;
mod screens;
mod setup;
mod store;
mod strings;
mod ui;
mod view;

use std::time::{Duration, Instant};

fn arg_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    if std::env::args().any(|a| a == "--flow-test") {
        std::process::exit(flow_test());
    }
    if std::env::args().any(|a| a == "--self-test") {
        std::process::exit(self_test());
    }
    if let Some(f) = arg_value("--analyze-trace") {
        std::process::exit(analyze_trace(&f));
    }
    if std::env::args().any(|a| a == "--motion-probe") {
        let secs = arg_value("--motion-probe")
            .and_then(|s| s.parse().ok())
            .unwrap_or(10.0);
        std::process::exit(motion_probe(secs));
    }
    if let Err(e) = run() {
        log::error!("fatal: {e}");
        eprintln!("fatal: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let data = platform::data_dir();
    let log_path = platform::init_logging(&data);
    let app_id = platform::app_id();
    log::info!(
        "Luma Blocks {} starting as {} (webOS {:?})",
        env!("CARGO_PKG_VERSION"),
        app_id,
        platform::webos_release()
    );
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    platform::set_sdl_hints();
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    log::info!("SDL video driver: {}", video.current_video_driver());
    let sdl_desc = keys::init_key_abi();
    log::info!("{sdl_desc}");
    let shots = arg_value("--shots");
    let (w, h) = match video.current_display_mode(0) {
        Ok(m) if m.w > 0 && m.h > 0 && shots.is_none() => (m.w as u32, m.h as u32),
        _ => (1920, 1080),
    };
    let mut wb = video.window(
        "Luma Blocks",
        if platform::is_webos() { w } else { w.min(1920) },
        if platform::is_webos() { h } else { h.min(1080) },
    );
    if platform::is_webos() || std::env::var("LUMA_FULLSCREEN").is_ok() {
        wb.fullscreen();
    }
    if platform::is_webos() {
        wb.opengl();
    } else if shots.is_none() {
        wb.resizable();
    }
    let window = wb.build().map_err(|e| e.to_string())?;
    let canvas = match window.clone().into_canvas().accelerated().present_vsync().build() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("accelerated renderer unavailable ({e}); using software rendering");
            window
                .into_canvas()
                .software()
                .build()
                .map_err(|e| e.to_string())?
        }
    };
    log::info!(
        "renderer: {} output {:?}",
        canvas.info().name,
        canvas.output_size()
    );
    let mut g = gfx::Gfx::new(canvas)?;

    let store = if shots.is_some() {
        let d = std::env::temp_dir().join(format!("luma-shots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        store::Store::open(&d)
    } else {
        store::Store::open(&data)
    };
    let motion = if let Some(f) = arg_value("--replay") {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?;
        let (lines, bad) = luma_input::trace::parse(&text);
        log::info!(
            "replaying {} trace lines ({bad} unreadable) from {f}",
            lines.len()
        );
        motion::MotionHub::trace(lines)
    } else if platform::is_webos() {
        motion::MotionHub::remote(app_id.clone(), 100.0)
    } else {
        motion::MotionHub::simulated()
    };
    // Kept alive for the whole run; see `platform::make_cursor`.
    let _cursor = if platform::is_webos() || std::env::var("LUMA_OWN_CURSOR").is_ok() {
        match platform::make_cursor(h) {
            Ok(c) => {
                c.set();
                log::info!("pointer: app-provided arrow");
                Some(c)
            }
            Err(e) => {
                log::warn!("pointer: keeping the system arrow ({e})");
                None
            }
        }
    } else {
        None
    };
    let mut app = app::App::new(sdl.clone(), store, motion, log_path, sdl_desc);
    if let Some(dir) = shots {
        return screenshots(&mut app, &mut g, &dir);
    }

    let mut inputs = Vec::new();
    while !app.quit {
        let t0 = Instant::now();
        inputs.clear();
        keys::poll(&mut inputs);
        app.handle(std::mem::take(&mut inputs));
        app.update();
        app.draw(&mut g);
        g.canvas.present();
        let el = t0.elapsed();
        app.stats.push(el.as_secs_f32() * 1000.0);
        let now = platform::now_us();
        if app.wants_low_power(now) {
            // Backgrounded or fully dimmed: render rarely.
            std::thread::sleep(Duration::from_millis(100));
        } else if el < Duration::from_millis(6) {
            // present_vsync paces us; this guards renderers without vsync.
            std::thread::sleep(Duration::from_millis(6) - el);
        }
    }
    app.save_now();
    app.store.save_settings();
    app.store.flush();
    log::info!("exit");
    Ok(())
}

/// Render a fixed tour of screens to BMP files (converted to PNG by scripts/shots.sh).
fn screenshots(app: &mut app::App, g: &mut gfx::Gfx, dir: &str) -> Result<(), String> {
    use luma_core::{Action, Mode};
    use luma_input::buttons::Button;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let shot = |app: &mut app::App, g: &mut gfx::Gfx, name: &str, frames: u32| -> Result<(), String> {
        for _ in 0..frames {
            app.update();
            app.draw(g);
            g.canvas.present();
            std::thread::sleep(Duration::from_millis(16));
        }
        app.draw(g);
        let (w, h) = g.canvas.output_size()?;
        let mut px = g
            .canvas
            .read_pixels(None, sdl2::pixels::PixelFormatEnum::ABGR8888)?;
        let surf =
            sdl2::surface::Surface::from_data(&mut px, w, h, w * 4, sdl2::pixels::PixelFormatEnum::ABGR8888)?;
        surf.save_bmp(format!("{dir}/{name}.bmp"))?;
        Ok(())
    };
    let press = |app: &mut app::App, b: Button| {
        app.handle(vec![
            keys::In::Button {
                b,
                down: true,
                repeat: false,
            },
            keys::In::Button {
                b,
                down: false,
                repeat: false,
            },
        ]);
    };
    app.store.settings.first_run_done = true;
    app.store.settings.developer = true;
    app.go(app::Screen::Menu);
    shot(app, g, "menu", 30)?;
    // A mid-game board.
    app.new_game(Mode::Marathon);
    if let Some(s) = app.session.as_mut() {
        s.ready_until = None;
        let script = [
            Action::MoveLeft,
            Action::MoveLeft,
            Action::MoveLeft,
            Action::HardDrop,
            Action::MoveRight,
            Action::MoveRight,
            Action::MoveRight,
            Action::MoveRight,
            Action::HardDrop,
            Action::RotateCw,
            Action::HardDrop,
            Action::MoveLeft,
            Action::RotateCcw,
            Action::MoveLeft,
            Action::HardDrop,
            Action::Hold,
            Action::MoveRight,
            Action::MoveRight,
            Action::MoveRight,
            Action::HardDrop,
            Action::RotateCw,
            Action::MoveRight,
            Action::HardDrop,
            Action::MoveLeft,
            Action::MoveLeft,
            Action::HardDrop,
        ];
        for a in script {
            s.game.apply(a);
        }
        s.game.drain_events();
        s.game.score = 18_420;
        s.game.lines = 23;
        s.game.level = 3;
        s.game.apply(Action::MoveLeft);
    }
    shot(app, g, "game", 20)?;
    app.pause(app::PauseReason::User);
    shot(app, g, "pause", 10)?;
    app.quit_to_menu();
    app.return_to = app::Screen::Menu;
    app.go(app::Screen::Settings);
    shot(app, g, "settings", 10)?;
    app.go(app::Screen::Diagnostics);
    if let Some(sim) = app.motion.sim() {
        sim.play(motion::Script::TwistCw);
    }
    shot(app, g, "diagnostics", 45)?;
    app.start_onboarding();
    app.store.settings.control = store::ControlMode::Motion;
    press(app, Button::Ok);
    shot(app, g, "onboarding-style", 40)?;
    app.start_calibration(app::Screen::Menu);
    shot(app, g, "calibration-hold", 40)?;
    shot(app, g, "calibration-step", 40)?;
    let play = |app: &mut app::App, sc: motion::Script| {
        if let Some(sim) = app.motion.sim() {
            sim.play(sc);
        }
    };
    play(app, motion::Script::TwistCw);
    shot(app, g, "calibration-twist", 14)?;
    shot(app, g, "calibration-twist-done", 70)?;
    play(app, motion::Script::SwingRight);
    shot(app, g, "calibration-swing", 9)?;
    shot(app, g, "calibration-swing-done", 70)?;
    play(app, motion::Script::SwingRight);
    shot(app, g, "calibration-try-1", 70)?;
    play(app, motion::Script::TwistCw);
    shot(app, g, "calibration-try", 22)?;
    // A line clear caught mid-animation.
    app.new_game(Mode::Marathon);
    if let Some(s) = app.session.as_mut() {
        s.ready_until = None;
        for y in 36..40 {
            for x in 1..10 {
                s.game.board[(y * 10 + x) as usize] = 1 + ((x + y) % 7) as u8;
            }
        }
        s.game.board[(35 * 10 + 4) as usize] = 3;
        s.game.active = luma_core::Piece {
            kind: luma_core::Kind::I,
            rot: 1,
            x: -2,
            y: 22,
        };
    }
    shot(app, g, "line-clear-before", 2)?;
    if let Some(s) = app.session.as_mut() {
        s.game.apply(Action::HardDrop);
    }
    shot(app, g, "line-clear", 5)?;
    app.new_game(Mode::Practice);
    if let Some(s) = app.session.as_mut() {
        s.ready_until = None;
    }
    shot(app, g, "practice", 20)?;
    log::info!("screenshots written to {dir}");
    Ok(())
}

/// Replay a recorded trace offline and print the recognized gestures.
fn analyze_trace(path: &str) -> i32 {
    use luma_input::config::{HorizontalStyle, Profile, RotationStyle};
    use luma_input::trace::{parse, replay, TraceLine};
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("cannot read {path}");
        return 2;
    };
    let (lines, bad) = parse(&text);
    let profile = arg_value("--profile").map(|p| {
        let (r, h) = p.split_once('-').unwrap_or(("twist", "point"));
        Profile {
            rotation: if r == "circle" {
                RotationStyle::Circle
            } else {
                RotationStyle::Twist
            },
            horizontal: match h {
                "flick" => HorizontalStyle::Flick,
                "point" => HorizontalStyle::Pointing,
                _ => HorizontalStyle::Swing,
            },
            ..Profile::default()
        }
    });
    if let Some(TraceLine::Header { origin, note, .. }) = lines.first() {
        println!("trace origin {origin:?}: {note}");
    }
    let motion = lines
        .iter()
        .filter(|l| matches!(l, TraceLine::Motion { .. }))
        .count();
    println!(
        "{} lines ({motion} motion samples, {bad} unreadable)",
        lines.len()
    );
    let t0 = lines.iter().find_map(|l| l.time()).unwrap_or(0);
    println!("recorded live:");
    for l in &lines {
        if let TraceLine::Recognized { t, g } = l {
            println!("  {:>8.3}s  {}", (t - t0) as f64 / 1e6, g.label());
        }
    }
    println!("replayed now:");
    for c in replay(&lines, profile, None) {
        println!(
            "  {:>8.3}s  {}  ({}, {:.0} ms)",
            (c.t_us - t0) as f64 / 1e6,
            c.gesture.label(),
            c.recognizer,
            c.recognition_ms()
        );
    }
    0
}

/// Non-graphical self test (CI under qemu-arm, and over SSH on a TV). Exit code 0 = passed.
fn self_test() -> i32 {
    use luma_core::{Action, Game, Mode, Rules};
    use luma_input::synth::{gestures, Synth};
    let mut ok = true;
    let mut check = |name: &str, r: Result<String, String>| match &r {
        Ok(d) => println!("PASS {name}: {d}"),
        Err(e) => {
            println!("FAIL {name}: {e}");
            ok = false;
        }
    };
    check("engine", {
        let run = || {
            let mut g = Game::new(Mode::Marathon, Rules::default(), 99);
            for i in 0..400u64 {
                g.apply(
                    [
                        Action::MoveLeft,
                        Action::RotateCw,
                        Action::HardDrop,
                        Action::MoveRight,
                    ][(i % 4) as usize],
                );
                g.tick(16_667);
            }
            g
        };
        let (a, b) = (run(), run());
        if a == b {
            Ok(format!(
                "deterministic replay, {} pieces, score {}",
                a.pieces_placed, a.score
            ))
        } else {
            Err("replay diverged".into())
        }
    });
    check("gestures", {
        let mut e =
            luma_input::GestureEngine::new(Default::default(), Default::default(), Default::default());
        let mut s = Synth::new(100.0, 1);
        let mut v = s.hold(500.0);
        v.extend(gestures::twist(&mut s, 1.0, 45.0, 200.0));
        v.extend(gestures::drop(&mut s, 18.0, 160.0));
        let mut got = Vec::new();
        for smp in &v {
            e.tick(smp.t_us);
            e.motion(smp);
            got.extend(e.take().into_iter().map(|c| c.gesture));
        }
        if got == vec![luma_input::Gesture::RotateCw, luma_input::Gesture::HardDrop] {
            Ok("synthetic twist + drop recognized".into())
        } else {
            Err(format!("{got:?}"))
        }
    });
    check("mrcu parse", {
        let p = r#"{"returnValue":true,"gameRotationVector":{"w":0.78,"x":0.1,"y":-0.03,"z":0.62},"gyroscope":{"x":0.1,"y":0.2,"z":0.3}}"#;
        match luma_input::mrcu::parse_sensor2(p, 1) {
            luma_input::mrcu::Reply::Data(s) if s.quat.is_some() && s.gyro.is_some() => {
                Ok("sensor2 sample".into())
            }
            r => Err(format!("{r:?}")),
        }
    });
    let dir = std::env::temp_dir().join(format!("luma-selftest-{}", std::process::id()));
    check("store", {
        let mut st = store::Store::open(&dir);
        let g = Game::new(Mode::Zen, Rules::default(), 3);
        st.save_game(&g);
        st.save_settings();
        st.flush();
        let r = store::Store::open(&dir);
        let res = match r.save {
            Some(s) if s.game.board == g.board => Ok("atomic save and restore".into()),
            _ => Err("save not restored".into()),
        };
        let _ = std::fs::remove_dir_all(&dir);
        res
    });
    match luna::available() {
        Ok(()) => println!("INFO luna: libluna-service2 and glib loaded"),
        Err(e) => println!("INFO luna: unavailable ({e})"),
    }
    println!("INFO webOS release: {:?}", platform::webos_release());
    println!("INFO app id: {}", platform::app_id());
    if ok {
        0
    } else {
        1
    }
}

/// Drive the real application state machine without a window (SDL dummy drivers): menus,
/// gameplay, pause/resume, save-on-quit, relaunch with the resume prompt, focus loss.
fn flow_test() -> i32 {
    use app::Screen;
    use keys::In;
    use luma_input::buttons::Button;
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");
    let dir = std::env::temp_dir().join(format!("luma-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let sdl = match sdl2::init() {
        Ok(s) => s,
        Err(e) => {
            println!("SKIP flow test: SDL unavailable ({e})");
            return 0;
        }
    };
    let _video = sdl.video();
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        println!("{} {name}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            fails += 1;
        }
    };
    let press = |a: &mut app::App, b: Button| {
        a.handle(vec![In::Button {
            b,
            down: true,
            repeat: false,
        }]);
        a.update();
        a.handle(vec![In::Button {
            b,
            down: false,
            repeat: false,
        }]);
        a.update();
    };
    let wait = |a: &mut app::App, ms: u64| {
        let end = platform::now_us() + ms * 1000;
        while platform::now_us() < end {
            a.update();
            std::thread::sleep(Duration::from_millis(8));
        }
    };
    let mk = |sdl: &sdl2::Sdl| {
        app::App::new(
            sdl.clone(),
            store::Store::open(&dir),
            motion::MotionHub::simulated(),
            dir.join("log.txt"),
            "test".into(),
        )
    };
    let mut a = mk(&sdl);
    check("first run opens onboarding", a.screen == Screen::Onboard);
    press(&mut a, Button::Back); // skip setup
    check("skipping setup reaches the menu", a.screen == Screen::Menu);
    press(&mut a, Button::Ok); // Marathon
    check("Marathon starts", a.screen == Screen::Game);
    wait(&mut a, 700); // ready guard + stillness
    let ready_done = a.session.as_ref().is_some_and(|s| s.ready_until.is_none());
    check("ready guard ends after release and stillness", ready_done);
    press(&mut a, Button::Left);
    press(&mut a, Button::Up);
    press(&mut a, Button::Ok);
    let placed = a.session.as_ref().map(|s| s.game.pieces_placed).unwrap_or(0);
    check("buttons move, rotate and drop", placed == 1);
    press(&mut a, Button::Back);
    check("Back pauses (never abandons the game)", a.screen == Screen::Pause);
    press(&mut a, Button::Back);
    check("Back from pause resumes", a.screen == Screen::Game);
    wait(&mut a, 700);
    press(&mut a, Button::Back);
    // Pause menu: Resume, Rotate anticlockwise, Hold, Recenter, Settings, Restart, Quit.
    for _ in 0..6 {
        press(&mut a, Button::Down);
    }
    press(&mut a, Button::Ok);
    check("quit to menu", a.screen == Screen::Menu && a.session.is_none());
    check("game saved on quit", a.store.save.is_some());
    a.store.flush();
    drop(a);
    let mut b = mk(&sdl);
    check("relaunch asks to continue", b.screen == Screen::Resume);
    press(&mut b, Button::Ok);
    check("continue restores paused", b.screen == Screen::Pause);
    let restored = b.session.as_ref().map(|s| s.game.pieces_placed).unwrap_or(0);
    check("restored game state", restored == 1);
    press(&mut b, Button::Ok); // Resume
    wait(&mut b, 700);
    b.handle(vec![In::Focus(false)]);
    b.update();
    check("focus loss pauses", b.screen == Screen::Pause);
    b.handle(vec![In::Focus(true)]);
    b.update();
    check("focus gain stays paused", b.screen == Screen::Pause);
    b.exit();
    check("exit requested", b.quit);
    // Guided motion setup end to end with the simulated remote.
    b.quit = false;
    b.quit_to_menu();
    b.start_calibration(Screen::Menu);
    let step = |a: &app::App| a.cal.as_ref().map(|c| c.step);
    wait(&mut b, 1600);
    check(
        "setup: holding still completes step 1",
        step(&b) == Some(setup::CalStep::Twist),
    );
    if let Some(sim) = b.motion.sim() {
        sim.play(motion::Script::TwistCw);
    }
    wait(&mut b, 1300);
    check(
        "setup: a clockwise twist completes step 2",
        step(&b) == Some(setup::CalStep::Swing),
    );
    check(
        "setup: measured axis matches the remote",
        b.engine.calib.long_axis == luma_input::math::Vec3::Y && b.engine.calib.twist_sign == 1.0,
    );
    wait(&mut b, 400);
    if let Some(sim) = b.motion.sim() {
        sim.play(motion::Script::SwingRight);
    }
    wait(&mut b, 1300);
    check(
        "setup: a swing completes step 3",
        step(&b) == Some(setup::CalStep::Try),
    );
    press(&mut b, Button::Ok);
    check(
        "setup: OK finishes and returns",
        b.screen == Screen::Menu && b.cal.is_none(),
    );
    check("setup: calibration saved", b.store.calibration.axes_measured);
    b.continue_saved();
    b.resume();
    // Motion loss: Twist profile, sensor stream dies during play.
    b.quit = false;
    b.resume();
    wait(&mut b, 700);
    b.motion = motion::MotionHub::trace(Vec::new());
    if let Some(s) = b.session.as_mut() {
        s.sensor_seen = true;
    }
    wait(&mut b, 1800);
    check("motion loss pauses with options", b.screen == Screen::MotionLost);
    let _ = std::fs::remove_dir_all(&dir);
    if fails == 0 {
        println!("flow test passed");
        0
    } else {
        1
    }
}

/// Connect to the motion sensor without the game, printing the connection log and the
/// measured rate once a second. Exit code 0 when samples arrived at 50/s or more.
fn motion_probe(secs: f64) -> i32 {
    let mut hub = motion::MotionHub::remote(platform::app_id(), 100.0);
    hub.start();
    let t0 = platform::now_us();
    let (mut shown, mut total, mut last_print) = (0u64, 0usize, t0);
    let mut best_hz = 0.0f32;
    let mut first: Option<luma_input::sample::MotionSample> = None;
    loop {
        let now = platform::now_us();
        let (v, _) = hub.drain(now);
        total += v.len();
        if first.is_none() {
            first = v.first().copied();
            if let Some(s) = first {
                println!("first sample: {s:?}");
            }
        }
        let st = hub.status();
        let new = (st.notes - shown).min(st.log.len() as u64) as usize;
        for l in st.log.iter().skip(st.log.len() - new) {
            println!("{l}");
        }
        shown = st.notes;
        if now - last_print >= 1_000_000 {
            last_print = now;
            let hz = hub.rate.hz(now);
            best_hz = best_hz.max(hz);
            println!(
                "  [{:>4.1}s] state: {} · samples {total} · {hz:.0}/s",
                (now - t0) as f64 / 1e6,
                st.state.label()
            );
        }
        if (now - t0) as f64 / 1e6 >= secs {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("last payload: {}", hub.status().last_payload);
    hub.stop();
    std::thread::sleep(Duration::from_millis(200));
    if best_hz >= 50.0 {
        println!("PASS motion stream reached {best_hz:.0}/s");
        0
    } else {
        println!("FAIL motion stream best {best_hz:.0}/s");
        1
    }
}
