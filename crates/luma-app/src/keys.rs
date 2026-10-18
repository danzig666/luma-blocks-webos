//! SDL event decoding into platform-neutral inputs.
//!
//! webOS remote buttons arrive from the SDL-webOS fork as keyboard events with fork-specific
//! scancodes (`include/SDL_scancode.h` of the 2.30.x fork): BACK 482, RED 486, GREEN 487,
//! YELLOW 488, BLUE 489, CH_UP 480, CH_DOWN 481, EXIT 505. The keyboard event layout is
//! chosen from the SDL library actually loaded (LG's own 2.0.x build inserts a 32-bit field
//! and shifts the keysym by four bytes). Decoding approach adapted from the same owner's
//! webos-rustdesk-client, where it is verified on an LG C3.
//!
//! Desktop simulator: arrows, Enter (OK), Esc/Backspace (Back), F1-F4 (colour buttons),
//! PageUp/PageDown (CH+/-).

use luma_input::buttons::Button;
use sdl2::{event::Event, keyboard::Keycode, mouse::MouseButton};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum In {
    Button {
        b: Button,
        down: bool,
        repeat: bool,
    },
    /// Other keyboard keys (simulator shortcuts, diagnostics).
    Key {
        key: Keycode,
        down: bool,
        repeat: bool,
    },
    PointerMove {
        x: i32,
        y: i32,
    },
    Click {
        left: bool,
        down: bool,
        x: i32,
        y: i32,
    },
    Wheel {
        dy: i32,
    },
    Focus(bool),
    Quit,
}

const WEBOS_CH_UP: i32 = 480;
const WEBOS_CH_DOWN: i32 = 481;
const WEBOS_BACK: i32 = 482;
const WEBOS_RED: i32 = 486;
const WEBOS_GREEN: i32 = 487;
const WEBOS_YELLOW: i32 = 488;
const WEBOS_BLUE: i32 = 489;
const WEBOS_EXIT: i32 = 505;

pub enum WebosKey {
    Button(Button),
    Exit,
}

pub fn webos_scancode(sc: i32) -> Option<WebosKey> {
    Some(WebosKey::Button(match sc {
        WEBOS_CH_UP => Button::ChUp,
        WEBOS_CH_DOWN => Button::ChDown,
        WEBOS_BACK => Button::Back,
        WEBOS_RED => Button::Red,
        WEBOS_GREEN => Button::Green,
        WEBOS_YELLOW => Button::Yellow,
        WEBOS_BLUE => Button::Blue,
        WEBOS_EXIT => return Some(WebosKey::Exit),
        _ => return None,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAbi {
    Upstream,
    Lg,
}

static KEY_ABI: std::sync::OnceLock<KeyAbi> = std::sync::OnceLock::new();

/// Detect (once) the loaded SDL version and keyboard event layout.
pub fn init_key_abi() -> String {
    let mut v = sdl2::sys::SDL_version {
        major: 0,
        minor: 0,
        patch: 0,
    };
    unsafe { sdl2::sys::SDL_GetVersion(&mut v) };
    let abi = match std::env::var("LUMA_SDL_ABI").ok().as_deref() {
        Some("lg") => KeyAbi::Lg,
        Some("upstream") => KeyAbi::Upstream,
        // LG's firmware SDL2 builds are 2.0.x; the bundled backport is 2.30.x.
        _ if crate::platform::is_webos() && v.major == 2 && v.minor == 0 => KeyAbi::Lg,
        _ => KeyAbi::Upstream,
    };
    let _ = KEY_ABI.set(abi);
    format!("SDL {}.{}.{} ({:?} key layout)", v.major, v.minor, v.patch, abi)
}

fn key_abi() -> KeyAbi {
    *KEY_ABI.get().unwrap_or(&KeyAbi::Upstream)
}

/// One raw input observation for the diagnostics screen.
#[derive(Debug, Clone)]
pub struct RawLog {
    pub t_us: u64,
    pub text: String,
}

static RAW_LOG: Mutex<VecDeque<RawLog>> = Mutex::new(VecDeque::new());
const RAW_LOG_LEN: usize = 18;

pub fn log_raw(text: String) {
    if let Ok(mut l) = RAW_LOG.lock() {
        if l.len() >= RAW_LOG_LEN {
            l.pop_front();
        }
        l.push_back(RawLog {
            t_us: crate::platform::now_us(),
            text,
        });
    }
}

pub fn raw_log() -> Vec<RawLog> {
    RAW_LOG
        .lock()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

struct RawKey {
    down: bool,
    repeat: bool,
    scancode: i32,
    sym: i32,
}

fn decode_key(raw: &sdl2::sys::SDL_Event, down: bool) -> RawKey {
    let p = raw as *const sdl2::sys::SDL_Event as *const u8;
    // Upstream: repeat@13 keysym{scancode@16 sym@20}; LG layout: everything +4.
    let off = match key_abi() {
        KeyAbi::Upstream => 0,
        KeyAbi::Lg => 4,
    };
    // SAFETY: SDL_Event is a 56-byte union; every offset read is below 32.
    unsafe {
        RawKey {
            down,
            repeat: *p.add(13 + off) != 0,
            scancode: std::ptr::read_unaligned(p.add(16 + off) as *const i32),
            sym: std::ptr::read_unaligned(p.add(20 + off) as *const i32),
        }
    }
}

fn scancode_name(sc: i32) -> String {
    if !(0..512).contains(&sc) {
        return String::new();
    }
    extern "C" {
        // Declared with a plain int: webOS scancodes are not variants of the bindgen enum.
        fn SDL_GetScancodeName(scancode: std::ffi::c_int) -> *const std::ffi::c_char;
    }
    // Never Keycode::name(): for webOS remote keys SDL builds the name from a keysym outside
    // Unicode and the crate's UTF-8 unwrap panics.
    let p = unsafe { SDL_GetScancodeName(sc) };
    if p.is_null() {
        String::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Poll all pending SDL events and translate them.
pub fn poll(out: &mut Vec<In>) {
    use sdl2::sys::SDL_EventType as T;
    loop {
        let mut raw = std::mem::MaybeUninit::<sdl2::sys::SDL_Event>::uninit();
        // SAFETY: SDL_PollEvent fully initializes the event when it returns 1.
        if unsafe { sdl2::sys::SDL_PollEvent(raw.as_mut_ptr()) } == 0 {
            break;
        }
        let raw = unsafe { raw.assume_init() };
        let ty = unsafe { raw.type_ };
        if ty == T::SDL_KEYDOWN as u32 || ty == T::SDL_KEYUP as u32 {
            let k = decode_key(&raw, ty == T::SDL_KEYDOWN as u32);
            log_raw(format!(
                "{} {}{} sc{} sym0x{:x}",
                if k.down { "down" } else { "up  " },
                scancode_name(k.scancode),
                if k.repeat { " (repeat)" } else { "" },
                k.scancode,
                k.sym
            ));
            match webos_scancode(k.scancode) {
                Some(WebosKey::Button(b)) => {
                    out.push(In::Button {
                        b,
                        down: k.down,
                        repeat: k.repeat,
                    });
                    continue;
                }
                Some(WebosKey::Exit) => {
                    if k.down {
                        out.push(In::Quit);
                    }
                    continue;
                }
                None => {}
            }
            if let Some(key) = Keycode::from_i32(k.sym) {
                translate_key(key, k.down, k.repeat, out);
            }
            continue;
        }
        if ty == T::SDL_TEXTINPUT as u32 || ty == T::SDL_TEXTEDITING as u32 {
            continue; // no text entry in this app (and the LG layout shifts the payload)
        }
        translate(Event::from_ll(raw), out);
    }
}

fn translate_key(key: Keycode, down: bool, repeat: bool, out: &mut Vec<In>) {
    let webos = crate::platform::is_webos();
    let b = match key {
        Keycode::Up => Some(Button::Up),
        Keycode::Down => Some(Button::Down),
        Keycode::Left => Some(Button::Left),
        Keycode::Right => Some(Button::Right),
        Keycode::Return | Keycode::KpEnter => Some(Button::Ok),
        Keycode::AcBack => Some(Button::Back),
        Keycode::Escape | Keycode::Backspace => Some(Button::Back),
        Keycode::F1 if !webos => Some(Button::Red),
        Keycode::F2 if !webos => Some(Button::Green),
        Keycode::F3 if !webos => Some(Button::Yellow),
        Keycode::F4 if !webos => Some(Button::Blue),
        Keycode::PageUp => Some(Button::ChUp),
        Keycode::PageDown => Some(Button::ChDown),
        _ => None,
    };
    match b {
        Some(b) => out.push(In::Button { b, down, repeat }),
        None => out.push(In::Key { key, down, repeat }),
    }
}

fn translate(ev: Event, out: &mut Vec<In>) {
    match ev {
        Event::Quit { .. } => out.push(In::Quit),
        Event::MouseMotion { x, y, .. } => out.push(In::PointerMove { x, y }),
        Event::MouseButtonDown { mouse_btn, x, y, .. } | Event::MouseButtonUp { mouse_btn, x, y, .. } => {
            let down = matches!(ev, Event::MouseButtonDown { .. });
            let left = match mouse_btn {
                MouseButton::Left => true,
                MouseButton::Right => false,
                _ => return,
            };
            log_raw(format!(
                "{} mouse {} at {x},{y}",
                if down { "down" } else { "up  " },
                if left { "left" } else { "right" }
            ));
            out.push(In::Click { left, down, x, y });
        }
        Event::MouseWheel { y, .. } => {
            log_raw(format!("wheel {y:+}"));
            out.push(In::Wheel { dy: y })
        }
        Event::Window { win_event, .. } => match win_event {
            sdl2::event::WindowEvent::FocusGained | sdl2::event::WindowEvent::Shown => {
                out.push(In::Focus(true))
            }
            sdl2::event::WindowEvent::FocusLost | sdl2::event::WindowEvent::Hidden => {
                out.push(In::Focus(false))
            }
            _ => {}
        },
        Event::AppWillEnterBackground { .. } | Event::AppDidEnterBackground { .. } => {
            out.push(In::Focus(false))
        }
        Event::AppDidEnterForeground { .. } => out.push(In::Focus(true)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webos_colour_back_and_exit_scancodes() {
        assert!(matches!(webos_scancode(486), Some(WebosKey::Button(Button::Red))));
        assert!(matches!(
            webos_scancode(489),
            Some(WebosKey::Button(Button::Blue))
        ));
        assert!(matches!(
            webos_scancode(482),
            Some(WebosKey::Button(Button::Back))
        ));
        assert!(matches!(webos_scancode(505), Some(WebosKey::Exit)));
        assert!(webos_scancode(40).is_none());
    }

    #[test]
    fn desktop_keys_map_to_remote_buttons() {
        let mut v = Vec::new();
        translate_key(Keycode::Return, true, false, &mut v);
        translate_key(Keycode::F1, true, false, &mut v);
        translate_key(Keycode::Q, true, false, &mut v);
        assert_eq!(
            v,
            vec![
                In::Button {
                    b: Button::Ok,
                    down: true,
                    repeat: false
                },
                In::Button {
                    b: Button::Red,
                    down: true,
                    repeat: false
                },
                In::Key {
                    key: Keycode::Q,
                    down: true,
                    repeat: false
                },
            ]
        );
    }
}
