//! Minimal native Luna (LS2) client, loaded at runtime.
//!
//! `libluna-service2.so.3` and `libglib-2.0.so.0` are `dlopen`ed rather than linked, so the
//! binary still starts (with motion reported unavailable) on a system without them and the
//! host build needs no webOS libraries. Signatures follow `luna-service2/lunaservice.h` from
//! the webOS SDK sysroot (LS2 3.21). All unsafe code is in this file; the bus handle and its
//! private glib context are only touched from the one thread that owns the [`Bus`].
//!
//! Registration: a native app's generated role file allows its app id and "" as names. Which
//! registration shape the hub accepts, and whether the motion service then answers, is
//! measured at runtime (the shapes are tried in order and every outcome is logged and shown
//! in diagnostics). Field notes from plx-native (docs in `platform/src/webos.rs`, commit
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
    lib().map(|_| ())
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
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `LSRegisterApplicationService(app_id, app_id)`.
    AppService,
    /// `LSRegister(app_id)`.
    Named,
    /// `LSRegister(NULL)`.
    Anonymous,
    /// `LSRegister(NULL)` with calls sent as `LSCallFromApplication(app_id)`.
    AnonymousFromApp,
}

impl Shape {
    pub const ORDER: [Shape; 4] = [
        Shape::AppService,
        Shape::Named,
        Shape::Anonymous,
        Shape::AnonymousFromApp,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Shape::AppService => "application service (app id)",
            Shape::Named => "named client (app id)",
            Shape::Anonymous => "anonymous client",
            Shape::AnonymousFromApp => "anonymous client, calls tagged with app id",
        }
    }
}

/// A live bus registration with its private glib context. Owned by one thread.
pub struct Bus {
    lib: &'static Lib,
    handle: Handle,
    ctx: *mut c_void,
    app_id: CString,
    pub shape: Shape,
    queue: ReplyQueue,
    next_id: u64,
}

impl Bus {
    pub fn register(shape: Shape, app_id: &str, queue: ReplyQueue) -> Result<Bus, String> {
        let lib = lib()?;
        let app = CString::new(app_id).map_err(|_| "bad app id".to_owned())?;
        let err = Err::new(lib);
        let mut err = err;
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
                Shape::Anonymous | Shape::AnonymousFromApp => {
                    (lib.register)(std::ptr::null(), &mut handle, &mut err.e)
                }
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
        Ok(Bus {
            lib,
            handle,
            ctx,
            app_id: app,
            shape,
            queue,
            next_id: 1,
        })
    }

    /// Start a call; replies (one, or many for a subscription) arrive in the queue tagged
    /// with the returned id. Returns (call id, LS2 token).
    pub fn call(&mut self, uri: &str, payload: &str) -> Result<(u64, u64), String> {
        let id = self.next_id;
        self.next_id += 1;
        let u = CString::new(uri).map_err(|_| "uri".to_owned())?;
        let p = CString::new(payload).map_err(|_| "payload".to_owned())?;
        // The context is leaked on purpose: LS2 may still dispatch into it after a cancel
        // races with a reply. A few dozen bytes per call, bounded by the call count.
        let ctx = Box::into_raw(Box::new(CallCtx {
            id,
            queue: self.queue.clone(),
        })) as *mut c_void;
        let mut token: c_ulong = 0;
        let mut err = Err::new(self.lib);
        // SAFETY: valid handle, C strings live across the call, callback/ctx valid forever.
        let ok = unsafe {
            match (self.shape, self.lib.call_from_app) {
                (Shape::AnonymousFromApp, Some(f)) => f(
                    self.handle,
                    u.as_ptr(),
                    p.as_ptr(),
                    self.app_id.as_ptr(),
                    on_reply,
                    ctx,
                    &mut token,
                    &mut err.e,
                ),
                _ => (self.lib.call)(
                    self.handle,
                    u.as_ptr(),
                    p.as_ptr(),
                    on_reply,
                    ctx,
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

    pub fn cancel(&mut self, token: u64) {
        if token == 0 {
            return;
        }
        let mut err = Err::new(self.lib);
        // SAFETY: valid handle; an unknown token only produces an error.
        if !unsafe { (self.lib.cancel)(self.handle, token as c_ulong, &mut err.e) } {
            log::debug!("LSCallCancel({token}) refused: {}", err.text());
        }
    }

    /// Dispatch pending bus messages without blocking.
    pub fn pump(&mut self) {
        for _ in 0..256 {
            // SAFETY: the context belongs to this bus and this thread.
            if unsafe { (self.lib.ctx_iteration)(self.ctx, 0) } == 0 {
                break;
            }
        }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let mut err = Err::new(self.lib);
        // SAFETY: unregister the handle; free the context only if the handle is gone.
        if unsafe { (self.lib.unregister)(self.handle, &mut err.e) } {
            unsafe { (self.lib.ctx_unref)(self.ctx) };
        } else {
            log::warn!("LSUnregister refused ({}); leaking its context", err.text());
        }
    }
}
