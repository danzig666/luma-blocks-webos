//! Motion acquisition: the webOS Magic Remote sensor adapter (a worker thread speaking
//! `com.webos.service.mrcu` over Luna), the desktop simulator and trace playback. All of
//! them deliver normalized [`MotionSample`]s through a bounded queue; the frame loop never
//! blocks on acquisition.
//!
//! Backend policy: prefer `sensor2/getSensorEventData` (webOS TV 24+), request a ~100 Hz
//! interval within the reported bounds, and fall back to legacy `sensor/getSensorData` only
//! when sensor2 is unsupported. Sleep, permission denial, unsupported firmware, duplicate
//! subscription and malformed data are distinguished. The subscription is cancelled while
//! paused/backgrounded and re-established on demand.

use crate::luna::{self, Bus, ReplyQueue, Shape};
use crate::platform::now_us;
use luma_input::mrcu::{self, LegacyQuatOrder, MrcuError, Reply};
use luma_input::sample::{Fields, MotionSample, Origin, RateMeter};
use luma_input::synth;
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    NotStarted,
    /// LS2 libraries missing (desktop build, or an unexpected firmware).
    NoBus(String),
    Registering,
    Probing,
    Subscribing,
    /// Subscribed, waiting for the first data callback.
    Subscribed,
    Streaming,
    /// The remote's sensor is asleep (move the remote to wake it).
    Sleeping,
    /// Stopped on request (paused / backgrounded).
    Stopped,
    Unsupported(String),
    Denied(String),
    Failed(String),
    Simulated,
    Trace,
}

impl State {
    pub fn label(&self) -> String {
        match self {
            State::NotStarted => "not started".into(),
            State::NoBus(e) => format!("no Luna bus ({e})"),
            State::Registering => "registering on the bus".into(),
            State::Probing => "probing motion service".into(),
            State::Subscribing => "subscribing".into(),
            State::Subscribed => "subscribed, waiting for data".into(),
            State::Streaming => "streaming".into(),
            State::Sleeping => "remote asleep".into(),
            State::Stopped => "stopped (paused)".into(),
            State::Unsupported(e) => format!("unsupported: {e}"),
            State::Denied(e) => format!("permission denied: {e}"),
            State::Failed(e) => format!("failed: {e}"),
            State::Simulated => "simulated".into(),
            State::Trace => "trace playback".into(),
        }
    }
    pub fn is_streaming(&self) -> bool {
        matches!(self, State::Streaming | State::Simulated | State::Trace)
    }
    /// Permanent unavailability (no point retrying automatically).
    pub fn is_unavailable(&self) -> bool {
        matches!(self, State::NoBus(_) | State::Unsupported(_) | State::Denied(_))
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    pub state: State,
    pub backend: Option<Origin>,
    pub registration: Option<String>,
    pub api_version: Option<String>,
    pub interval: Option<(u32, u32, u32)>,
    pub requested_ms: Option<u32>,
    pub last_error: Option<MrcuError>,
    pub log: VecDeque<String>,
    pub last_payload: String,
    /// Incremented whenever the stream (re)starts: remote wake, reconnection, resubscribe.
    pub generation: u64,
    pub malformed: u64,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            state: State::NotStarted,
            backend: None,
            registration: None,
            api_version: None,
            interval: None,
            requested_ms: None,
            last_error: None,
            log: VecDeque::new(),
            last_payload: String::new(),
            generation: 0,
            malformed: 0,
        }
    }
}

impl Status {
    fn note(&mut self, s: String) {
        log::info!("motion: {s}");
        if self.log.len() >= 14 {
            self.log.pop_front();
        }
        self.log.push_back(format!("{:>8.2}s {s}", now_us() as f64 / 1e6));
    }
}

struct Shared {
    samples: Mutex<VecDeque<MotionSample>>,
    status: Mutex<Status>,
    dropped: Mutex<u64>,
}

const SAMPLE_QUEUE: usize = 512;

impl Shared {
    fn push(&self, s: MotionSample) {
        if let Ok(mut q) = self.samples.lock() {
            if q.len() >= SAMPLE_QUEUE {
                q.pop_front();
                if let Ok(mut d) = self.dropped.lock() {
                    *d += 1;
                }
            }
            q.push_back(s);
        }
    }
    fn status<R>(&self, f: impl FnOnce(&mut Status) -> R) -> R {
        let mut g = self.status.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut g)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Start,
    Stop,
    Recenter,
    SetQuatOrder(LegacyQuatOrder),
    Shutdown,
}

// ---------------------------------------------------------------------------------------
// webOS worker
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Call {
    Probe,
    State,
    GetInterval,
    SetInterval,
    Subscribe,
    LegacySubscribe,
    Cancel,
    Recenter,
}

struct Worker {
    shared: Arc<Shared>,
    rx: Receiver<Cmd>,
    queue: ReplyQueue,
    app_id: String,
    bus: Option<Bus>,
    shape_idx: usize,
    calls: Vec<(u64, Call)>,
    sub_token: u64,
    legacy: bool,
    order: LegacyQuatOrder,
    target_hz: f32,
    want_running: bool,
    last_data_us: u64,
    next_action_us: u64,
    probe_sent_us: u64,
    retried_duplicate: bool,
}

impl Worker {
    fn set_state(&self, s: State) {
        self.shared.status(|st| {
            if st.state != s {
                st.note(format!("state: {}", s.label()));
                st.state = s;
            }
        });
    }

    fn state(&self) -> State {
        self.shared.status(|s| s.state.clone())
    }

    fn call(&mut self, uri_method: &str, payload: &str, kind: Call) -> Option<u64> {
        let bus = self.bus.as_mut()?;
        let uri = format!("{}/{}", mrcu::SERVICE, uri_method);
        match bus.call(&uri, payload) {
            Ok((id, token)) => {
                self.calls.push((id, kind));
                if self.calls.len() > 64 {
                    self.calls.remove(0);
                }
                Some(token)
            }
            Err(e) => {
                self.shared.status(|s| s.note(format!("{uri_method}: {e}")));
                None
            }
        }
    }

    fn register_next(&mut self) -> bool {
        self.bus = None;
        while self.shape_idx < Shape::ORDER.len() {
            let shape = Shape::ORDER[self.shape_idx];
            self.shape_idx += 1;
            match Bus::register(shape, &self.app_id, self.queue.clone()) {
                Ok(b) => {
                    self.shared.status(|s| {
                        s.registration = Some(shape.label().to_owned());
                        s.note(format!("registered as {}", shape.label()));
                    });
                    self.bus = Some(b);
                    return true;
                }
                Err(e) => self.shared.status(|s| s.note(format!("{}: {e}", shape.label()))),
            }
        }
        false
    }

    fn start(&mut self) {
        self.want_running = true;
        if self.bus.is_none() {
            self.set_state(State::Registering);
            if !self.register_next() {
                self.set_state(State::Failed("no registration accepted".into()));
                return;
            }
        }
        self.probe();
    }

    fn probe(&mut self) {
        self.set_state(State::Probing);
        self.probe_sent_us = now_us();
        self.call("getAPIVersion", "{}", Call::Probe);
    }

    fn subscribe(&mut self) {
        self.retried_duplicate = false;
        self.set_state(State::Subscribing);
        if self.legacy {
            let code = mrcu::legacy_interval_code(self.target_hz);
            let p = format!("{{\"callbackInterval\":{code},\"subscribe\":true}}");
            self.sub_token = self
                .call("sensor/getSensorData", &p, Call::LegacySubscribe)
                .unwrap_or(0);
            self.shared.status(|s| s.backend = Some(Origin::Legacy));
        } else {
            let p = format!(
                "{{\"subscribe\":true,\"sensorType\":\"{}\"}}",
                mrcu::SENSOR2_TYPES
            );
            self.sub_token = self
                .call("sensor2/getSensorEventData", &p, Call::Subscribe)
                .unwrap_or(0);
            self.shared.status(|s| s.backend = Some(Origin::Sensor2));
        }
        self.last_data_us = now_us();
    }

    fn unsubscribe(&mut self) {
        if let Some(b) = self.bus.as_mut() {
            b.cancel(self.sub_token);
        }
        self.sub_token = 0;
        if !self.legacy && self.bus.is_some() {
            self.call("sensor2/cancelSensorDataSubscribe", "{}", Call::Cancel);
        }
    }

    fn stop(&mut self) {
        self.want_running = false;
        self.unsubscribe();
        if !self.state().is_unavailable() {
            self.set_state(State::Stopped);
        }
    }

    fn on_reply(&mut self, call: Call, t_us: u64, payload: &str) {
        self.shared.status(|s| {
            s.last_payload = payload.chars().take(300).collect();
        });
        match call {
            Call::Probe => match parse_simple(payload) {
                Ok(v) => {
                    let ver = v
                        .get("version")
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_owned();
                    self.shared.status(|s| s.api_version = Some(ver));
                    self.legacy = false;
                    self.call("sensor2/getSensorInterval", "{}", Call::GetInterval);
                }
                Err(MrcuError::PermissionDenied(t)) => {
                    self.shared
                        .status(|s| s.last_error = Some(MrcuError::PermissionDenied(t.clone())));
                    if self.register_next() {
                        self.probe();
                    } else {
                        self.set_state(State::Denied(t));
                    }
                }
                Err(e) => {
                    // No sensor2 family: try the legacy API (it answers for itself).
                    self.shared
                        .status(|s| s.note(format!("getAPIVersion: {e:?}; trying legacy API")));
                    self.legacy = true;
                    self.subscribe();
                }
            },
            Call::GetInterval => match mrcu::parse_interval(payload) {
                Ok((cur, min, max)) => {
                    let want = mrcu::choose_interval(min, max, self.target_hz);
                    self.shared.status(|s| {
                        s.interval = Some((cur, min, max));
                        s.requested_ms = Some(want);
                    });
                    if want != cur {
                        self.call(
                            "sensor2/setSensorInterval",
                            &format!("{{\"interval\":{want}}}"),
                            Call::SetInterval,
                        );
                    }
                    self.subscribe();
                }
                Err(e) => {
                    self.shared
                        .status(|s| s.note(format!("getSensorInterval: {e:?}")));
                    self.subscribe();
                }
            },
            Call::SetInterval => {
                if let Err(e) = parse_simple(payload) {
                    self.shared
                        .status(|s| s.note(format!("setSensorInterval: {e:?}")));
                }
            }
            Call::State => match mrcu::parse_state(payload) {
                Ok(true) => {
                    if self.want_running && !matches!(self.state(), State::Streaming | State::Subscribed) {
                        self.shared.status(|s| s.note("remote awake".into()));
                        self.unsubscribe();
                        self.subscribe();
                    }
                }
                Ok(false) => self.set_state(State::Sleeping),
                Err(e) => self.shared.status(|s| s.note(format!("getSensorState: {e:?}"))),
            },
            Call::Subscribe | Call::LegacySubscribe => {
                let r = if call == Call::Subscribe {
                    mrcu::parse_sensor2(payload, t_us)
                } else {
                    mrcu::parse_legacy(payload, t_us, self.order)
                };
                self.on_stream(r, call == Call::LegacySubscribe);
            }
            Call::Cancel | Call::Recenter => {
                if let Err(e) = parse_simple(payload) {
                    self.shared.status(|s| s.note(format!("{call:?}: {e:?}")));
                }
            }
        }
    }

    fn on_stream(&mut self, r: Reply, legacy: bool) {
        if !self.want_running {
            return;
        }
        match r {
            Reply::Data(s) => {
                let restarted = !matches!(self.state(), State::Streaming);
                self.last_data_us = s.t_us;
                if restarted {
                    self.shared.status(|st| st.generation += 1);
                    self.set_state(State::Streaming);
                }
                self.shared.push(s);
            }
            Reply::Subscribed => self.set_state(State::Subscribed),
            Reply::Ok => {}
            Reply::Error(e) => {
                self.shared.status(|s| s.last_error = Some(e.clone()));
                match e {
                    MrcuError::Sleeping => {
                        self.unsubscribe();
                        self.set_state(State::Sleeping);
                        self.next_action_us = now_us() + 1_000_000;
                    }
                    MrcuError::AlreadySubscribed if !self.retried_duplicate => {
                        self.retried_duplicate = true;
                        self.call("sensor2/cancelSensorDataSubscribe", "{}", Call::Cancel);
                        self.next_action_us = now_us() + 300_000;
                        self.set_state(State::Failed("already subscribed; retrying".into()));
                    }
                    MrcuError::Unsupported(t) if !legacy => {
                        self.shared
                            .status(|s| s.note(format!("sensor2 unsupported ({t}); trying legacy")));
                        self.legacy = true;
                        self.subscribe();
                    }
                    MrcuError::Unsupported(t) => self.set_state(State::Unsupported(t)),
                    MrcuError::PermissionDenied(t) => {
                        if self.register_next() {
                            self.probe();
                        } else {
                            self.set_state(State::Denied(t));
                        }
                    }
                    MrcuError::Malformed(m) => {
                        self.shared.status(|s| {
                            s.malformed += 1;
                            if s.malformed % 50 == 1 {
                                s.note(format!("malformed sample: {m}"));
                            }
                        });
                    }
                    other => {
                        self.set_state(State::Failed(other.user_label().to_owned()));
                        self.next_action_us = now_us() + 2_000_000;
                    }
                }
            }
        }
    }

    fn timers(&mut self) {
        if !self.want_running || self.bus.is_none() {
            return;
        }
        let now = now_us();
        match self.state() {
            State::Probing if now - self.probe_sent_us > 2_500_000 => {
                self.shared
                    .status(|s| s.note("no reply to getAPIVersion; trying legacy API".into()));
                self.legacy = true;
                self.subscribe();
            }
            State::Streaming | State::Subscribed | State::Subscribing
                if now - self.last_data_us > 1_500_000 =>
            {
                // Stream stalled: the remote may have gone to sleep or the subscription died.
                if self.legacy {
                    self.unsubscribe();
                    self.subscribe();
                } else {
                    self.set_state(State::Sleeping);
                    self.call("sensor2/getSensorState", "{}", Call::State);
                    self.next_action_us = now + 1_000_000;
                }
            }
            State::Sleeping | State::Failed(_) if now >= self.next_action_us => {
                self.next_action_us = now + 1_000_000;
                if self.legacy {
                    self.subscribe();
                } else if matches!(self.state(), State::Failed(_)) {
                    self.unsubscribe();
                    self.subscribe();
                } else {
                    self.call("sensor2/getSensorState", "{}", Call::State);
                }
            }
            _ => {}
        }
    }

    fn run(mut self) {
        loop {
            while let Ok(c) = self.rx.try_recv() {
                match c {
                    Cmd::Start => self.start(),
                    Cmd::Stop => self.stop(),
                    Cmd::Recenter => {
                        let m = if self.legacy {
                            "sensor/resetQuaternion"
                        } else {
                            "sensor2/resetQuaternion"
                        };
                        self.call(m, "{}", Call::Recenter);
                    }
                    Cmd::SetQuatOrder(o) => self.order = o,
                    Cmd::Shutdown => {
                        self.stop();
                        if let Some(b) = self.bus.as_mut() {
                            b.pump();
                        }
                        return;
                    }
                }
            }
            if let Some(b) = self.bus.as_mut() {
                b.pump();
            }
            let replies: Vec<luna::Reply> = self
                .queue
                .lock()
                .map(|mut q| q.drain(..).collect())
                .unwrap_or_default();
            for r in replies {
                if let Some(kind) = self.calls.iter().find(|(id, _)| *id == r.call).map(|(_, k)| *k) {
                    self.on_reply(kind, r.t_us, &r.payload);
                }
            }
            self.timers();
            let idle = !self.want_running || self.bus.is_none();
            std::thread::sleep(Duration::from_millis(if idle { 30 } else { 2 }));
        }
    }
}

fn parse_simple(payload: &str) -> Result<serde_json::Value, MrcuError> {
    let v: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| MrcuError::Malformed(e.to_string()))?;
    if v.get("returnValue").and_then(|r| r.as_bool()) == Some(false) {
        return Err(mrcu::classify_error(&v, false));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------
// Hub used by the frame loop
// ---------------------------------------------------------------------------------------

enum Kind {
    Remote { tx: Sender<Cmd> },
    Sim(Box<SimMotion>),
    Trace(Box<TracePlayer>),
    None,
}

pub struct MotionHub {
    kind: Kind,
    shared: Arc<Shared>,
    pub rate: RateMeter,
    pub fields: Fields,
    pub last: Option<MotionSample>,
    pub origin: Option<Origin>,
    seen_generation: u64,
    running: bool,
}

impl MotionHub {
    fn new_shared() -> Arc<Shared> {
        Arc::new(Shared {
            samples: Mutex::new(VecDeque::new()),
            status: Mutex::new(Status::default()),
            dropped: Mutex::new(0),
        })
    }

    /// The real remote sensor (webOS). On a desktop the bus is missing and the status says so.
    pub fn remote(app_id: String, target_hz: f32) -> MotionHub {
        let shared = Self::new_shared();
        if let Err(e) = luna::available() {
            shared.status(|s| {
                s.state = State::NoBus(e.clone());
                s.note(format!("Luna unavailable: {e}"));
            });
            return MotionHub {
                kind: Kind::None,
                shared,
                rate: RateMeter::default(),
                fields: Fields::default(),
                last: None,
                origin: None,
                seen_generation: 0,
                running: false,
            };
        }
        let (tx, rx) = channel();
        let sh = shared.clone();
        // The worker (and the bus it owns) is created on, and never leaves, its own thread.
        let _ = std::thread::Builder::new().name("mrcu".into()).spawn(move || {
            Worker {
                shared: sh,
                rx,
                queue: Arc::new(Mutex::new(VecDeque::new())),
                app_id,
                bus: None,
                shape_idx: 0,
                calls: Vec::new(),
                sub_token: 0,
                legacy: false,
                order: LegacyQuatOrder::default(),
                target_hz,
                want_running: false,
                last_data_us: 0,
                next_action_us: 0,
                probe_sent_us: 0,
                retried_duplicate: false,
            }
            .run()
        });
        MotionHub {
            kind: Kind::Remote { tx },
            shared,
            rate: RateMeter::default(),
            fields: Fields::default(),
            last: None,
            origin: Some(Origin::Sensor2),
            seen_generation: 0,
            running: false,
        }
    }

    pub fn simulated() -> MotionHub {
        let shared = Self::new_shared();
        shared.status(|s| {
            s.state = State::Simulated;
            s.backend = Some(Origin::Simulated);
        });
        MotionHub {
            kind: Kind::Sim(Box::default()),
            shared,
            rate: RateMeter::default(),
            fields: Fields::default(),
            last: None,
            origin: Some(Origin::Simulated),
            seen_generation: 0,
            running: true,
        }
    }

    pub fn trace(lines: Vec<luma_input::trace::TraceLine>) -> MotionHub {
        let shared = Self::new_shared();
        shared.status(|s| {
            s.state = State::Trace;
            s.backend = Some(Origin::Trace);
        });
        MotionHub {
            kind: Kind::Trace(Box::new(TracePlayer::new(lines))),
            shared,
            rate: RateMeter::default(),
            fields: Fields::default(),
            last: None,
            origin: Some(Origin::Trace),
            seen_generation: 0,
            running: true,
        }
    }

    pub fn status(&self) -> Status {
        self.shared.status(|s| s.clone())
    }

    pub fn dropped(&self) -> u64 {
        self.shared.dropped.lock().map(|d| *d).unwrap_or(0)
    }

    pub fn send(&self, c: Cmd) {
        if let Kind::Remote { tx } = &self.kind {
            let _ = tx.send(c);
        }
    }

    pub fn start(&mut self) {
        if !self.running {
            self.running = true;
            self.send(Cmd::Start);
        }
    }

    pub fn stop(&mut self) {
        if self.running && matches!(self.kind, Kind::Remote { .. }) {
            self.running = false;
            self.send(Cmd::Stop);
        }
    }

    /// Retry after a failure (e.g. the "Try again" button).
    pub fn restart(&mut self) {
        self.running = true;
        self.send(Cmd::Stop);
        self.send(Cmd::Start);
    }

    pub fn is_real(&self) -> bool {
        matches!(self.kind, Kind::Remote { .. })
    }

    pub fn sim(&mut self) -> Option<&mut SimMotion> {
        match &mut self.kind {
            Kind::Sim(s) => Some(s),
            _ => None,
        }
    }

    /// Collect pending samples. Returns them oldest first and whether the stream restarted
    /// (wake / reconnection) since the last call.
    pub fn drain(&mut self, now: u64) -> (Vec<MotionSample>, bool) {
        match &mut self.kind {
            Kind::Sim(s) => {
                for smp in s.generate(now) {
                    self.shared.push(smp);
                }
            }
            Kind::Trace(t) => {
                for smp in t.generate(now) {
                    self.shared.push(smp);
                }
            }
            _ => {}
        }
        let v: Vec<MotionSample> = self
            .shared
            .samples
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        for s in &v {
            self.rate.push(s.t_us);
            self.fields.observe(s);
            self.last = Some(*s);
        }
        let generation = self.shared.status(|s| {
            if let Some(b) = s.backend {
                self.origin = Some(b);
            }
            s.generation
        });
        let restarted = generation != self.seen_generation;
        self.seen_generation = generation;
        (v, restarted)
    }

    pub fn state(&self) -> State {
        self.shared.status(|s| s.state.clone())
    }

    pub fn is_streaming(&self, now: u64) -> bool {
        self.state().is_streaming() && self.rate.age_ms(now).is_some_and(|a| a < 400.0)
    }
}

impl Drop for MotionHub {
    fn drop(&mut self) {
        self.send(Cmd::Shutdown);
    }
}

// ---------------------------------------------------------------------------------------
// Desktop simulator
// ---------------------------------------------------------------------------------------

/// Desktop stand-in for the remote's sensor: hold the right mouse button and move to aim
/// (yaw/pitch), Q/E (or Shift+wheel) to twist, and number keys 1-6 to play scripted
/// gestures. Output is labelled Simulated everywhere.
pub struct SimMotion {
    synth: synth::Synth,
    pub aiming: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub twist: f32,
    twist_vel: f32,
    scripted: VecDeque<MotionSample>,
    script_base_us: u64,
    last_us: u64,
}

impl Default for SimMotion {
    fn default() -> Self {
        let mut s = synth::Synth::new(100.0, 1);
        s.tremor_deg = 0.08;
        SimMotion {
            synth: s,
            aiming: false,
            yaw: 0.0,
            pitch: 0.0,
            twist: 0.0,
            twist_vel: 0.0,
            scripted: VecDeque::new(),
            script_base_us: 0,
            last_us: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Script {
    TwistCw,
    TwistCcw,
    CircleCw,
    CircleCcw,
    FlickLeft,
    FlickRight,
    SwingLeft,
    SwingRight,
    Drop,
}

impl SimMotion {
    pub fn mouse_delta(&mut self, dx: f32, dy: f32) {
        if self.aiming {
            self.yaw = (self.yaw + dx * 0.04).clamp(-40.0, 40.0);
            self.pitch = (self.pitch - dy * 0.04).clamp(-40.0, 40.0);
        }
    }
    pub fn twist_key(&mut self, dir: f32) {
        self.twist_vel = dir * 260.0;
    }
    pub fn twist_release(&mut self) {
        self.twist_vel = 0.0;
    }
    pub fn play(&mut self, s: Script) {
        let mut g = synth::Synth::new(100.0, 7);
        g.base = (self.yaw, self.pitch, self.twist);
        g.tremor_deg = 0.1;
        let v = match s {
            Script::TwistCw => synth::gestures::twist(&mut g, 1.0, 45.0, 200.0),
            Script::TwistCcw => synth::gestures::twist(&mut g, -1.0, 45.0, 200.0),
            Script::CircleCw => synth::gestures::circle(&mut g, 1.0, 3.5, 600.0, 1.05),
            Script::CircleCcw => synth::gestures::circle(&mut g, -1.0, 3.5, 600.0, 1.05),
            Script::FlickLeft => synth::gestures::flick(&mut g, -1.0, 7.0, 120.0),
            Script::FlickRight => synth::gestures::flick(&mut g, 1.0, 7.0, 120.0),
            Script::SwingLeft => synth::gestures::swing(&mut g, -1.0, 9.0, 170.0, 1.0),
            Script::SwingRight => synth::gestures::swing(&mut g, 1.0, 9.0, 170.0, 1.0),
            Script::Drop => synth::gestures::drop(&mut g, 18.0, 160.0),
        };
        // Continue from wherever the script leaves the remote, so there is no jump.
        (self.yaw, self.pitch, self.twist) = g.base;
        self.scripted = v.into();
        self.script_base_us = 0;
    }

    fn generate(&mut self, now: u64) -> Vec<MotionSample> {
        let mut out = Vec::new();
        if self.last_us == 0 {
            self.last_us = now;
        }
        if let Some(first) = self.scripted.front().copied() {
            if self.script_base_us == 0 {
                self.script_base_us = now.saturating_sub(first.t_us);
            }
            while let Some(s) = self.scripted.front().copied() {
                let t = s.t_us + self.script_base_us;
                if t > now {
                    break;
                }
                self.scripted.pop_front();
                let mut s = s;
                s.t_us = t;
                out.push(s);
                if self.scripted.is_empty() {
                    self.synth.t_us = t;
                }
            }
            self.last_us = now;
            return out;
        }
        // Continuous pose at 100 Hz.
        let step = 10_000;
        while self.last_us + step <= now {
            self.last_us += step;
            self.twist = (self.twist + self.twist_vel * step as f32 / 1e6).clamp(-120.0, 120.0);
            if self.twist_vel == 0.0 {
                self.twist *= 0.93; // springs back to neutral when released
            }
            self.synth.t_us = self.last_us - step;
            self.synth.base = (self.yaw, self.pitch, self.twist);
            out.extend(self.synth.hold(10.0));
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// Trace playback
// ---------------------------------------------------------------------------------------

pub struct TracePlayer {
    samples: VecDeque<MotionSample>,
    offset: Option<i64>,
}

impl TracePlayer {
    fn new(lines: Vec<luma_input::trace::TraceLine>) -> Self {
        TracePlayer {
            samples: lines.iter().filter_map(|l| l.to_sample()).collect(),
            offset: None,
        }
    }
    fn generate(&mut self, now: u64) -> Vec<MotionSample> {
        let mut out = Vec::new();
        let Some(first) = self.samples.front() else {
            return out;
        };
        let off = *self.offset.get_or_insert(now as i64 - first.t_us as i64);
        while let Some(s) = self.samples.front() {
            let t = (s.t_us as i64 + off) as u64;
            if t > now {
                break;
            }
            let mut s = self.samples.pop_front().unwrap();
            s.t_us = t;
            out.push(s);
        }
        out
    }
}
