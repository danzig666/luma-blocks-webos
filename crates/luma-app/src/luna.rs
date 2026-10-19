//! Minimal native Luna (LS2) client, loaded at runtime.
//!
//! `libluna-service2.so.3` and `libglib-2.0.so.0` are `dlopen`ed rather than linked, so the
//! binary still starts (with motion reported unavailable) on a system without them and the
//! host build needs no webOS libraries. Signatures follow `luna-service2/lunaservice.h` from
//! the webOS SDK sysroot (LS2 3.21). All unsafe code is in this file; the bus handle and its
//! private glib context are only touched from the one thread that owns the [`Bus`].
//!
//! Connection: the TV's own `libhelpers.so.2` is tried first. SDL-webOS already makes its
//! system calls through it, so it carries the identity the system gave this app. Then our
//! own LS2 registrations (a native app's generated role file allows its app id and "" as
//! names), then the `luna-send` commands where they exist. Which path the hub accepts, and
//! whether the motion service then answers, is measured at runtime: the shapes are tried in
//! order and every outcome is logged and shown in diagnostics. Field notes from plx-native (docs in `platform/src/webos.rs`, commit
//! 17eb48b) informed the order: an anonymous `LSRegister(NULL)` was accepted on a webOS 4
//! set where application-service registration was refused.

// The transmute target type is the declared `Lib` field type of each symbol.
#![allow(clippy::missing_transmute_annotations)]

use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_ulong, c_void, CStr, CString};
use std::sync::{Arc, Mutex, OnceLock};

#[repr(C)]
struct LSError {
    error_code: c_int,
    message: *mut c_char,
    file: *const c_char,
    line: c_int,
    func: *const c_char,
    padding: *mut c_void,
    magic: c_ulong,
}

type Handle = *mut c_void;
type FilterFn = extern "C" fn(Handle, *mut c_void, *mut c_void) -> bool;

struct Lib {
    error_init: unsafe extern "C" fn(*mut LSError) -> bool,
    error_free: unsafe extern "C" fn(*mut LSError),
    register: unsafe extern "C" fn(*const c_char, *mut Handle, *mut LSError) -> bool,
    register_app:
        Option<unsafe extern "C" fn(*const c_char, *const c_char, *mut Handle, *mut LSError) -> bool>,
    attach: unsafe extern "C" fn(Handle, *mut c_void, *mut LSError) -> bool,
    call: unsafe extern "C" fn(
        Handle,
        *const c_char,
        *const c_char,
        FilterFn,
        *mut c_void,
        *mut c_ulong,
        *mut LSError,
    ) -> bool,
    call_from_app: Option<
        unsafe extern "C" fn(
            Handle,
            *const c_char,
            *const c_char,
            *const c_char,
            FilterFn,
            *mut c_void,
            *mut c_ulong,
            *mut LSError,
        ) -> bool,
    >,
    cancel: unsafe extern "C" fn(Handle, c_ulong, *mut LSError) -> bool,
    payload: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    unregister: unsafe extern "C" fn(Handle, *mut LSError) -> bool,
    ctx_new: unsafe extern "C" fn() -> *mut c_void,
    ctx_iteration: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    ctx_unref: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: plain function pointers resolved once; the library stays loaded for the process.
unsafe impl Send for Lib {}
unsafe impl Sync for Lib {}

static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();

fn dlopen_any(names: &[&str]) -> Result<*mut c_void, String> {
    for n in names {
        let c = CString::new(*n).unwrap();
        // SAFETY: dlopen with a valid C string; the handle is never closed.
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
        if !h.is_null() {
            return Ok(h);
        }
    }
    Err(format!("{} not found", names[0]))
}

fn sym(h: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).unwrap();
    // SAFETY: valid handle and C string.
    let p = unsafe { libc::dlsym(h, c.as_ptr()) };
    (!p.is_null()).then_some(p)
}

fn load() -> Result<Lib, String> {
    let ls = dlopen_any(&["libluna-service2.so.3", "libluna-service2.so"])?;
    let gl = dlopen_any(&["libglib-2.0.so.0", "libglib-2.0.so"])?;
    macro_rules! req {
        ($h:expr, $n:literal) => {
            // SAFETY: the symbol has the C signature declared in `Lib` (lunaservice.h / glib).
            unsafe {
                std::mem::transmute::<*mut c_void, _>(sym($h, $n).ok_or_else(|| format!("missing {}", $n))?)
            }
        };
    }
    macro_rules! opt {
        ($h:expr, $n:literal) => {
            // SAFETY: as above.
            sym($h, $n).map(|p| unsafe { std::mem::transmute::<*mut c_void, _>(p) })
        };
    }
    Ok(Lib {
        error_init: req!(ls, "LSErrorInit"),
        error_free: req!(ls, "LSErrorFree"),
        register: req!(ls, "LSRegister"),
        register_app: opt!(ls, "LSRegisterApplicationService"),
        attach: req!(ls, "LSGmainContextAttach"),
        call: req!(ls, "LSCall"),
        call_from_app: opt!(ls, "LSCallFromApplication"),
        cancel: req!(ls, "LSCallCancel"),
        payload: req!(ls, "LSMessageGetPayload"),
        unregister: req!(ls, "LSUnregister"),
        ctx_new: req!(gl, "g_main_context_new"),
        ctx_iteration: req!(gl, "g_main_context_iteration"),
        ctx_unref: req!(gl, "g_main_context_unref"),
    })
}

fn lib() -> Result<&'static Lib, String> {
    LIB.get_or_init(load).as_ref().map_err(|e| e.clone())
}

/// Whether the LS2 libraries could be loaded (false on a desktop).
pub fn available() -> Result<(), String> {
    match (helpers(), lib()) {
        (Err(a), Err(b)) => Err(format!("{a}; {b}")),
        _ => Ok(()),
    }
}

struct Err {
    e: LSError,
    lib: &'static Lib,
}

impl Err {
    fn new(lib: &'static Lib) -> Err {
        // SAFETY: LSErrorInit on zeroed storage is the documented initialisation.
        let mut e: LSError = unsafe { std::mem::zeroed() };
        unsafe { (lib.error_init)(&mut e) };
        Err { e, lib }
    }
    fn text(&self) -> String {
        if self.e.message.is_null() {
            return format!("code {}", self.e.error_code);
        }
        // SAFETY: LS2 sets a NUL-terminated message.
        let m = unsafe { CStr::from_ptr(self.e.message) }.to_string_lossy();
        format!("code {}: {}", self.e.error_code, m.trim())
    }
}

impl Drop for Err {
    fn drop(&mut self) {
        // SAFETY: freeing an initialised (possibly unset) LSError is valid.
        unsafe { (self.lib.error_free)(&mut self.e) };
    }
}

/// A reply delivered by the bus to one call.
#[derive(Debug, Clone)]
pub struct Reply {
    pub call: u64,
    pub t_us: u64,
    pub payload: String,
}

pub type ReplyQueue = Arc<Mutex<VecDeque<Reply>>>;
const QUEUE_MAX: usize = 2048;

struct CallCtx {
    id: u64,
    queue: ReplyQueue,
}

extern "C" fn on_reply(_h: Handle, msg: *mut c_void, ctx: *mut c_void) -> bool {
    if ctx.is_null() || msg.is_null() {
        return true;
    }
    // SAFETY: ctx is a leaked `CallCtx` (never freed, see `Bus::call`); msg is valid for the
    // duration of the callback.
    let ctx = unsafe { &*(ctx as *const CallCtx) };
    let Ok(lib) = lib() else { return true };
    let p = unsafe { (lib.payload)(msg) };
    if p.is_null() {
        return true;
    }
    let payload = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    push_reply(ctx, payload);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The TV's own `libhelpers.so.2` (`HLunaServiceCall`, public bus): the same path SDL-webOS
    /// uses for its own calls, so it carries the identity the system gave this app.
    Helpers,
    /// `libhelpers.so.2` on the private bus.
    HelpersPrivate,
    /// `LSRegisterApplicationService(app_id, app_id)`.
    AppService,
    /// `LSRegister(app_id)`.
    Named,
    /// `LSRegister(NULL)`.
    Anonymous,
    /// `LSRegister(NULL)` with calls sent as `LSCallFromApplication(app_id)`.
    AnonymousFromApp,
    /// The `luna-send-pub` command (present when the app is not jailed, e.g. rooted TVs).
    LunaSendPub,
    /// The `luna-send` command.
    LunaSend,
}

impl Shape {
    pub const ORDER: [Shape; 8] = [
        Shape::Helpers,
        Shape::HelpersPrivate,
        Shape::AppService,
        Shape::Named,
        Shape::Anonymous,
        Shape::AnonymousFromApp,
        Shape::LunaSendPub,
        Shape::LunaSend,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Shape::Helpers => "system helper (public)",
            Shape::HelpersPrivate => "system helper (private)",
            Shape::AppService => "application service (app id)",
            Shape::Named => "named client (app id)",
            Shape::Anonymous => "anonymous client",
            Shape::AnonymousFromApp => "anonymous client, calls tagged with app id",
            Shape::LunaSendPub => "luna-send-pub command",
            Shape::LunaSend => "luna-send command",
        }
    }
}

// ---------------------------------------------------------------------------------------
// libhelpers (webos-helpers/libhelpers.h in the SDK sysroot)
// ---------------------------------------------------------------------------------------

type HFilterFn = extern "C" fn(Handle, *mut c_void, *mut HContext) -> bool;

#[repr(C)]
struct HContext {
    callback: HFilterFn,
    userdata: *mut c_void,
    unknown: *mut c_void,
    multiple: c_int,
    public: c_int,
    ret_token: c_ulong,
}

struct Helpers {
    call: unsafe extern "C" fn(*const c_char, *const c_char, *mut HContext) -> c_int,
    unregister: unsafe extern "C" fn(*mut HContext) -> c_int,
    message: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    error: Option<unsafe extern "C" fn(c_int) -> *const c_char>,
}

// SAFETY: plain function pointers resolved once; the library stays loaded for the process.
unsafe impl Send for Helpers {}
unsafe impl Sync for Helpers {}

static HELPERS: OnceLock<Result<Helpers, String>> = OnceLock::new();

fn helpers() -> Result<&'static Helpers, String> {
    HELPERS
        .get_or_init(|| {
            let h = dlopen_any(&["libhelpers.so.2", "libhelpers.so"])?;
            let need = |n: &str| sym(h, n).ok_or_else(|| format!("missing {n}"));
            // SAFETY: the symbols have the C signatures declared in `Helpers` (libhelpers.h).
            unsafe {
                Ok(Helpers {
                    call: std::mem::transmute::<*mut c_void, _>(need("HLunaServiceCall")?),
                    unregister: std::mem::transmute::<*mut c_void, _>(need("HUnregisterServiceCallback")?),
                    message: std::mem::transmute::<*mut c_void, _>(need("HLunaServiceMessage")?),
                    error: sym(h, "HGetError").map(|p| std::mem::transmute::<*mut c_void, _>(p)),
                })
            }
        })
        .as_ref()
        .map_err(|e| e.clone())
}

extern "C" fn on_helper_reply(_h: Handle, msg: *mut c_void, ctx: *mut HContext) -> bool {
    if ctx.is_null() || msg.is_null() {
        return true;
    }
    let Ok(h) = helpers() else { return true };
    // SAFETY: ctx is a leaked `HContext` whose userdata is a leaked `CallCtx`; msg is valid
    // for the duration of the callback.
    let call = unsafe { (*ctx).userdata as *const CallCtx };
    let p = unsafe { (h.message)(msg) };
    if call.is_null() || p.is_null() {
        return true;
    }
    let payload = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    push_reply(unsafe { &*call }, payload);
    true
}

fn push_reply(ctx: &CallCtx, payload: String) {
    if let Ok(mut q) = ctx.queue.lock() {
        if q.len() >= QUEUE_MAX {
            q.pop_front();
        }
        q.push_back(Reply {
            call: ctx.id,
            t_us: crate::platform::now_us(),
            payload,
        });
    }
}

// ---------------------------------------------------------------------------------------
// Bus
// ---------------------------------------------------------------------------------------

enum Transport {
    /// Our own LS2 registration with a private glib context.
    Ls {
        lib: &'static Lib,
        handle: Handle,
        ctx: *mut c_void,
        app_id: CString,
    },
    /// libhelpers' shared handle; it dispatches replies on its own thread.
    Helpers {
        h: &'static Helpers,
        public: bool,
        live: Vec<(u64, *mut HContext)>,
    },
    /// One `luna-send` process per call; stdout lines are replies.
    Command {
        path: &'static str,
        kids: Vec<(u64, std::process::Child)>,
    },
}

/// A live connection to the bus. Owned by one thread.
pub struct Bus {
    transport: Transport,
    pub shape: Shape,
    queue: ReplyQueue,
}

/// Call ids are unique for the whole process, so a late reply to a connection that was
/// replaced can never be mistaken for a reply to a new call.
static NEXT_CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn find_command(names: &[&'static str]) -> Option<&'static str> {
    names
        .iter()
        .copied()
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
}

impl Bus {
    pub fn register(shape: Shape, app_id: &str, queue: ReplyQueue) -> Result<Bus, String> {
        let transport = match shape {
            Shape::Helpers | Shape::HelpersPrivate => Transport::Helpers {
                h: helpers()?,
                public: shape == Shape::Helpers,
                live: Vec::new(),
            },
            Shape::LunaSendPub | Shape::LunaSend => {
                let names: &[&'static str] = if shape == Shape::LunaSendPub {
                    &["/usr/bin/luna-send-pub", "/bin/luna-send-pub"]
                } else {
                    &["/usr/bin/luna-send", "/bin/luna-send"]
                };
                Transport::Command {
                    path: find_command(names).ok_or("command not found")?,
                    kids: Vec::new(),
                }
            }
            _ => Self::register_ls(shape, app_id)?,
        };
        Ok(Bus {
            transport,
            shape,
            queue,
        })
    }

    fn register_ls(shape: Shape, app_id: &str) -> Result<Transport, String> {
        let lib = lib()?;
        let app = CString::new(app_id).map_err(|_| "bad app id".to_owned())?;
        let mut err = Err::new(lib);
        // SAFETY: FFI calls with valid pointers; failure paths free what was created.
        let ctx = unsafe { (lib.ctx_new)() };
        if ctx.is_null() {
            return Err("g_main_context_new failed".into());
        }
        let mut handle: Handle = std::ptr::null_mut();
        let ok = unsafe {
            match shape {
                Shape::AppService => match lib.register_app {
                    Some(f) => f(app.as_ptr(), app.as_ptr(), &mut handle, &mut err.e),
                    None => {
                        (lib.ctx_unref)(ctx);
                        return Err("LSRegisterApplicationService not exported".into());
                    }
                },
                Shape::Named => (lib.register)(app.as_ptr(), &mut handle, &mut err.e),
                _ => (lib.register)(std::ptr::null(), &mut handle, &mut err.e),
            }
        };
        if !ok || handle.is_null() {
            unsafe { (lib.ctx_unref)(ctx) };
            return Err(format!("register refused ({})", err.text()));
        }
        let mut err2 = Err::new(lib);
        if !unsafe { (lib.attach)(handle, ctx, &mut err2.e) } {
            let mut e3 = Err::new(lib);
            unsafe {
                (lib.unregister)(handle, &mut e3.e);
                (lib.ctx_unref)(ctx);
            }
            return Err(format!("attach failed ({})", err2.text()));
        }
        Ok(Transport::Ls {
            lib,
            handle,
            ctx,
            app_id: app,
        })
    }

    /// Start a call; replies (one, or many for a subscription) arrive in the queue tagged
    /// with the returned id. Returns (call id, cancel token).
    pub fn call(&mut self, uri: &str, payload: &str, subscribe: bool) -> Result<(u64, u64), String> {
        let id = NEXT_CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let u = CString::new(uri).map_err(|_| "uri".to_owned())?;
        let p = CString::new(payload).map_err(|_| "payload".to_owned())?;
        // The context is leaked on purpose: the bus may still dispatch into it after a cancel
        // races with a reply. A few dozen bytes per call, bounded by the call count.
        let ctx = Box::into_raw(Box::new(CallCtx {
            id,
            queue: self.queue.clone(),
        }));
        match &mut self.transport {
            Transport::Ls {
                lib, handle, app_id, ..
            } => {
                let mut token: c_ulong = 0;
                let mut err = Err::new(lib);
                // SAFETY: valid handle, C strings live across the call, callback/ctx valid
                // forever.
                let ok = unsafe {
                    match (self.shape, lib.call_from_app) {
                        (Shape::AnonymousFromApp, Some(f)) => f(
                            *handle,
                            u.as_ptr(),
                            p.as_ptr(),
                            app_id.as_ptr(),
                            on_reply,
                            ctx as *mut c_void,
                            &mut token,
                            &mut err.e,
                        ),
                        _ => (lib.call)(
                            *handle,
                            u.as_ptr(),
                            p.as_ptr(),
                            on_reply,
                            ctx as *mut c_void,
                            &mut token,
                            &mut err.e,
                        ),
                    }
                };
                if !ok {
                    return Err(format!("call failed ({})", err.text()));
                }
                Ok((id, token as u64))
            }
            Transport::Helpers { h, public, live } => {
                let hc = Box::into_raw(Box::new(HContext {
                    callback: on_helper_reply,
                    userdata: ctx as *mut c_void,
                    unknown: std::ptr::null_mut(),
                    multiple: subscribe as c_int,
                    public: *public as c_int,
                    ret_token: 0,
                }));
                // SAFETY: C strings live across the call; the context is leaked (valid forever).
                let rc = unsafe { (h.call)(u.as_ptr(), p.as_ptr(), hc) };
                if rc != 0 {
                    let text = h
                        .error
                        .map(|f| unsafe { f(rc) })
                        .filter(|t| !t.is_null())
                        .map(|t| unsafe { CStr::from_ptr(t) }.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    return Err(format!("call failed ({rc} {text})"));
                }
                if subscribe {
                    live.push((id, hc));
                }
                Ok((id, id))
            }
            Transport::Command { path, kids } => {
                use std::io::BufRead;
                let mut cmd = std::process::Command::new(*path);
                if subscribe {
                    cmd.arg("-i");
                } else {
                    cmd.args(["-n", "1"]);
                }
                let mut child = cmd
                    .args([uri, payload])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("spawn failed ({e})"))?;
                let out = child.stdout.take().ok_or("no stdout")?;
                // SAFETY: ctx is leaked and never freed.
                let cc: &'static CallCtx = unsafe { &*ctx };
                let _ = std::thread::Builder::new()
                    .name("luna-send".into())
                    .spawn(move || {
                        for line in std::io::BufReader::new(out).lines() {
                            let Ok(line) = line else { break };
                            let t = line.trim();
                            if t.starts_with('{') {
                                push_reply(cc, t.to_owned());
                            }
                        }
                    });
                if subscribe {
                    kids.push((id, child));
                } else {
                    // One-shot: reap it in the background.
                    let _ = std::thread::Builder::new()
                        .name("luna-send-wait".into())
                        .spawn(move || {
                            let _ = child.wait();
                        });
                }
                Ok((id, id))
            }
        }
    }

    pub fn cancel(&mut self, token: u64) {
        if token == 0 {
            return;
        }
        match &mut self.transport {
            Transport::Ls { lib, handle, .. } => {
                let mut err = Err::new(lib);
                // SAFETY: valid handle; an unknown token only produces an error.
                if !unsafe { (lib.cancel)(*handle, token as c_ulong, &mut err.e) } {
                    log::debug!("LSCallCancel({token}) refused: {}", err.text());
                }
            }
            Transport::Helpers { h, live, .. } => {
                if let Some(i) = live.iter().position(|(id, _)| *id == token) {
                    let (_, hc) = live.remove(i);
                    // SAFETY: hc is the leaked context of a live subscription.
                    let rc = unsafe { (h.unregister)(hc) };
                    if rc != 0 {
                        log::debug!("HUnregisterServiceCallback refused ({rc})");
                    }
                }
            }
            Transport::Command { kids, .. } => {
                if let Some(i) = kids.iter().position(|(id, _)| *id == token) {
                    let (_, mut child) = kids.remove(i);
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    /// Dispatch pending bus messages without blocking (only our own registration needs it).
    pub fn pump(&mut self) {
        if let Transport::Ls { lib, ctx, .. } = &self.transport {
            for _ in 0..256 {
                // SAFETY: the context belongs to this bus and this thread.
                if unsafe { (lib.ctx_iteration)(*ctx, 0) } == 0 {
                    break;
                }
            }
        }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let tokens: Vec<u64> = match &self.transport {
            Transport::Helpers { live, .. } => live.iter().map(|(id, _)| *id).collect(),
            Transport::Command { kids, .. } => kids.iter().map(|(id, _)| *id).collect(),
            Transport::Ls { .. } => Vec::new(),
        };
        for t in tokens {
            self.cancel(t);
        }
        if let Transport::Ls { lib, handle, ctx, .. } = &self.transport {
            let mut err = Err::new(lib);
            // SAFETY: unregister the handle; free the context only if the handle is gone.
            if unsafe { (lib.unregister)(*handle, &mut err.e) } {
                unsafe { (lib.ctx_unref)(*ctx) };
            } else {
                log::warn!("LSUnregister refused ({}); leaking its context", err.text());
            }
        }
    }
}
