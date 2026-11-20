//! RamRonin - an unhinged stickman ninja that lives on your desktop, perches
//! on your windows and bonks memory hogs. Also keeps chosen apps awake.
//!
//! Architecture (see README.md):
//!   host window (hidden)  - timer, tray icon, hotkeys, menus
//!   overlay window        - per-pixel-alpha layered window that follows the
//!                           ninja and is click-through everywhere he isn't
//!   brain::Ninja          - behaviour state machine
//!   tracker::Tracker      - app windows + visible top edges
//!   memory / keepalive    - background threads
//!   control               - the Control Center window

#![windows_subsystem = "windows"]

mod brain;
mod config;
mod control;
mod fx;
mod gfx;
mod keepalive;
mod memory;
mod props;
mod rig;
mod skins;
mod system;
mod taskbar;
mod tracker;
mod util;
mod win;

use brain::{JutsuK, Ninja, World};
use config::Config;
use gfx::Gfx;
use skins::{Look, Skin};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracker::Tracker;
use util::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_SHOW: u32 = WM_APP + 2;
pub const WM_APP_QUIT: u32 = WM_APP + 3;
pub const WM_APP_CMD: u32 = WM_APP + 4;
const OVERLAY_CLASS: &str = "RamRoninOverlay";
const COPYDATA_WHOP: usize = 0x5248_0001;

/// Win32 COPYDATASTRUCT (WM_COPYDATA payload).
#[repr(C)]
#[allow(non_snake_case)]
struct COPYDATASTRUCT {
    dwData: usize,
    cbData: u32,
    lpData: *mut c_void,
}
const TIMER_FRAME: usize = 1;

thread_local! {
    pub static CFG: RefCell<Config> = RefCell::new(Config::default());
    pub static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct Dib {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

pub struct App {
    pub host: HWND,
    pub overlay: HWND,
    pub ninja: Ninja,
    pub tracker: Tracker,
    pub mem: memory::Shared,
    pub keep: keepalive::Shared,
    pub tb: taskbar::Shared,
    pub tb_cache: Vec<taskbar::TbButton>,
    /// last time the *user* (not our Keep-Active F15 tap) touched input
    user_active_at: Instant,
    last_beat: Option<std::time::SystemTime>,
    pub stop: Arc<AtomicBool>,
    pub skins: Vec<Skin>,
    pub look: Look,
    gfx: Gfx,
    dib: Option<Dib>,
    last: Instant,
    pub hidden: bool,
    save_due: Option<Instant>,
    last_topmost: Instant,
    pub mem_cache: memory::MemSnap,
    mem_cache_at: Instant,
    fps: u32,
    autostart: bool,
    taskbar_msg: u32,
    shown: bool,
    debug: bool,
    last_debug: Instant,
    tb_logged: usize,
}

impl App {
    pub fn look_for(skins: &[Skin], id: &str) -> Look {
        skins.iter().find(|s| s.id == id).or_else(|| skins.first()).map(|s| s.look()).unwrap_or_else(|| Skin::default().look())
    }

    fn frame(&mut self, cfg: &Config) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        self.tracker.ledges_on = cfg.climb;
        self.tracker.tick();
        if self.mem_cache_at.elapsed() > Duration::from_millis(500) {
            if let Ok(m) = self.mem.lock() {
                self.mem_cache = m.clone();
            }
            if let Ok(t) = self.tb.lock() {
                self.tb_cache = t.clone();
            }
            if let Ok(k) = self.keep.lock() {
                self.last_beat = k.last_beat;
            }
            self.mem_cache_at = now;
        }
        // user idle time, ignoring our own Keep-Active heartbeat taps
        let beat_recent = self.last_beat.and_then(|b| b.elapsed().ok()).map(|e| e.as_secs_f32() < 3.0).unwrap_or(false);
        if win::idle_ms() < 1500 && !beat_recent {
            self.user_active_at = now;
        }
        let user_idle = (now - self.user_active_at).as_secs_f32();
        let w = World { tr: &self.tracker, mem: &self.mem_cache, cfg, look: &self.look, tb: &self.tb_cache, user_idle };
        self.ninja.update(dt, &w);
        if self.debug && self.last_debug.elapsed() > Duration::from_millis(250) {
            self.last_debug = now;
            let ledges = self.tracker.segs.iter().filter(|s| s.surf.is_ledge()).count();
            if self.tb_cache.len() != self.tb_logged {
                self.tb_logged = self.tb_cache.len();
                let names: Vec<String> = self.tb_cache.iter().map(|b| format!("{} @({},{})", b.name, b.rect.l, b.rect.t)).collect();
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                    .and_then(|mut f| std::io::Write::write_all(&mut f, format!("taskbar: {}\n", names.join(" | ")).as_bytes()));
            }
            let line = format!(
                "{:.2} act={:?} on={:?} pos=({:.0},{:.0}) segs={} ledges={} wins={} tb={} idle={:.0} clones={} pkg={}\n",
                self.ninja.t,
                self.ninja.act,
                self.ninja.on,
                self.ninja.x,
                self.ninja.y,
                self.tracker.segs.len(),
                ledges,
                self.tracker.wins.len(),
                self.tb_cache.len(),
                user_idle,
                self.ninja.clone_count(),
                system::packaged()
            );
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
                .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        }
        if self.hidden {
            return;
        }
        let lay = self.ninja.layout(&w);
        let b = self.ninja.bounds(&lay);
        let l = b.0.floor();
        let t = b.1.floor();
        let wd = ((((b.2 - b.0).ceil() as i32) + 31) / 32 * 32).clamp(32, 2600);
        let ht = ((((b.3 - b.1).ceil() as i32) + 31) / 32 * 32).clamp(32, 1800);
        self.gfx.reset(l, t, wd as u32, ht as u32, None);
        self.ninja.draw(&mut self.gfx, &lay, &w);
        let hb = self.ninja.hit;
        if hb.2 > hb.0 {
            self.gfx.hit_area(hb.0, hb.1, hb.2, hb.3);
        }
        self.present(l as i32, t as i32, wd, ht);
        if self.last_topmost.elapsed() > Duration::from_secs(2) {
            self.last_topmost = now;
            unsafe {
                SetWindowPos(self.overlay, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        if let Some(d) = self.save_due {
            if now >= d {
                cfg.save();
                self.save_due = None;
            }
        }
    }

    fn present(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            if self.dib.as_ref().map(|d| d.w != w || d.h != h).unwrap_or(true) {
                if let Some(d) = self.dib.take() {
                    SelectObject(d.dc, d.old);
                    DeleteObject(d.bmp);
                    DeleteDC(d.dc);
                }
                let screen = GetDC(std::ptr::null_mut());
                let dc = CreateCompatibleDC(screen);
                let mut bmi: BITMAPINFO = zeroed();
                bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = w;
                bmi.bmiHeader.biHeight = -h;
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB;
                let mut bits: *mut c_void = std::ptr::null_mut();
                let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
                ReleaseDC(std::ptr::null_mut(), screen);
                if bmp.is_null() || bits.is_null() {
                    DeleteDC(dc);
                    return;
                }
                let old = SelectObject(dc, bmp);
                self.dib = Some(Dib { dc, bmp, old, bits: bits as *mut u8, w, h });
            }
            let d = self.dib.as_ref().unwrap();
            let dst = std::slice::from_raw_parts_mut(d.bits, (w * h * 4) as usize);
            self.gfx.copy_bgra(dst);
            let pt_dst = POINT { x, y };
            let size = SIZE { cx: w, cy: h };
            let pt_src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(self.overlay, std::ptr::null_mut(), &pt_dst, &size, d.dc, &pt_src, 0, &blend, ULW_ALPHA);
            if !self.shown {
                ShowWindow(self.overlay, SW_SHOWNOACTIVATE);
                self.shown = true;
            }
        }
    }

    pub fn set_hidden(&mut self, hide: bool) {
        self.hidden = hide;
        unsafe {
            ShowWindow(self.overlay, if hide { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
        self.shown = !hide;
    }
}

/// Debug logging: env var RAMRONIN_DEBUG, or a file named `ramronin.debug`
/// next to the exe (then the log is written next to the exe too).
fn debug_flag_file() -> Option<std::path::PathBuf> {
    let exe = std::path::PathBuf::from(win::exe_path());
    let f = exe.parent()?.join("ramronin.debug");
    f.exists().then_some(f)
}
fn debug_log_path() -> std::path::PathBuf {
    match debug_flag_file() {
        Some(f) => f.with_file_name("debug.log"),
        None => data_dir().join("debug.log"),
    }
}

// ---------------------------------------------------------------------------
// access helpers (re-entrancy safe: never panic on a busy RefCell)
// ---------------------------------------------------------------------------

pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

/// Run `f` with the ninja and a World built from current state.
pub fn with_world<R>(f: impl FnOnce(&mut Ninja, &World) -> R) -> Option<R> {
    CFG.with(|c| {
        let cfg = c.try_borrow().ok()?;
        APP.with(|a| {
            let mut a = a.try_borrow_mut().ok()?;
            let app = a.as_mut()?;
            let user_idle = (Instant::now() - app.user_active_at).as_secs_f32();
            let w = World { tr: &app.tracker, mem: &app.mem_cache, cfg: &cfg, look: &app.look, tb: &app.tb_cache, user_idle };
            Some(f(&mut app.ninja, &w))
        })
    })
}

pub fn cfg_get<R>(f: impl FnOnce(&Config) -> R) -> R {
    CFG.with(|c| f(&c.borrow()))
}

/// Mutate the config, then apply side effects (skin, fps, autostart, save).
pub fn cfg_update(f: impl FnOnce(&mut Config)) {
    let ok = CFG.with(|c| {
        if let Ok(mut c) = c.try_borrow_mut() {
            f(&mut c);
            true
        } else {
            false
        }
    });
    if ok {
        config_changed();
    }
}

pub fn config_changed() {
    let cfg = cfg_get(|c| c.clone());
    with_app(|app| {
        if app.look.id != cfg.skin {
            app.look = App::look_for(&app.skins, &cfg.skin);
        }
        if let Ok(mut k) = app.keep.lock() {
            k.cfg = cfg.keep.clone();
        }
        if app.fps != cfg.fps {
            app.fps = cfg.fps;
            unsafe {
                SetTimer(app.host, TIMER_FRAME, 1000 / cfg.fps.clamp(15, 60), None);
            }
        }
        if app.autostart != cfg.autostart {
            app.autostart = cfg.autostart;
            system::set_autostart(cfg.autostart);
        }
        app.save_due = Some(Instant::now() + Duration::from_millis(800));
    });
}

pub fn reload_skins() {
    let skins = skins::load_all();
    let id = cfg_get(|c| c.skin.clone());
    with_app(|app| {
        app.look = App::look_for(&skins, &id);
        app.skins = skins;
    });
}

// ---------------------------------------------------------------------------
// menu & commands
// ---------------------------------------------------------------------------

pub const CMD_CONTROL: usize = 1000;
pub const CMD_CALLOUT: usize = 1001;
pub const CMD_BONK: usize = 1002;
pub const CMD_KEEP: usize = 1003;
pub const CMD_SHINOBI: usize = 1004;
pub const CMD_PAUSE: usize = 1005;
pub const CMD_HIDE: usize = 1006;
pub const CMD_HOME: usize = 1007;
pub const CMD_EXIT: usize = 1009;
pub const CMD_RIDE: usize = 1008;
pub const CMD_SKIN: usize = 1100;
pub const CMD_JUTSU: usize = 1200;

fn append(m: HMENU, id: usize, text: &str, checked: bool, enabled: bool) {
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    if !enabled {
        flags |= MF_GRAYED;
    }
    unsafe {
        AppendMenuW(m, flags, id, wide(text).as_ptr());
    }
}

pub fn show_menu(host: HWND) {
    let cfg = cfg_get(|c| c.clone());
    let (skins, hidden) = with_app(|a| (a.skins.iter().map(|s| (s.id.clone(), s.name.clone())).collect::<Vec<_>>(), a.hidden)).unwrap_or_default();
    unsafe {
        // compact: a few top-level entries, everything else in sub-menus
        let m = CreatePopupMenu();
        append(m, CMD_CONTROL, "Control Center...\tCtrl+Alt+R", false, true);
        let ch = CreatePopupMenu();
        for (i, (id, name)) in skins.iter().enumerate() {
            append(ch, CMD_SKIN + i, name, *id == cfg.skin, true);
        }
        AppendMenuW(ch, MF_SEPARATOR, 0, std::ptr::null());
        append(ch, CMD_SHINOBI, "Shinobi Edition (jutsu)", cfg.shinobi, true);
        AppendMenuW(m, MF_POPUP, ch as usize, wide("Character").as_ptr());
        let act = CreatePopupMenu();
        append(act, CMD_CALLOUT, "Memory callout now", false, true);
        append(act, CMD_BONK, "Whop the top memory hog", false, true);
        let js = CreatePopupMenu();
        for (i, n) in ["Spiral chakra orb", "Wind-spiral shuriken", "Kunai volley", "Shadow clones", "Fire breath"].iter().enumerate() {
            append(js, CMD_JUTSU + i, n, false, cfg.shinobi);
        }
        AppendMenuW(act, MF_POPUP | if cfg.shinobi { 0 } else { MF_GRAYED }, js as usize, wide("Jutsu").as_ptr());
        append(act, CMD_RIDE, "Joyride (horse / car)", false, cfg.props.horse || cfg.props.car);
        append(act, CMD_HOME, "Come here (to the mouse)", false, true);
        AppendMenuW(m, MF_POPUP, act as usize, wide("Do something").as_ptr());
        let opt = CreatePopupMenu();
        append(opt, CMD_KEEP, "Keep-Active mode", cfg.keep.enabled, true);
        append(opt, CMD_PAUSE, "Pause (meditate)", cfg.paused, true);
        append(opt, CMD_HIDE, if hidden { "Show character\tCtrl+Alt+H" } else { "Hide character\tCtrl+Alt+H" }, false, true);
        AppendMenuW(m, MF_POPUP, opt as usize, wide("Modes").as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        append(m, CMD_EXIT, "Exit RamRonin", false, true);
        let (cx, cy) = win::cursor_pos();
        SetForegroundWindow(host);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, cx, cy, 0, host, std::ptr::null()) as usize;
        PostMessageW(host, WM_NULL, 0, 0);
        DestroyMenu(m);
        handle_cmd(cmd);
    }
}

pub fn handle_cmd(cmd: usize) {
    match cmd {
        CMD_CONTROL => control::open(),
        CMD_CALLOUT => {
            with_world(|n, w| n.callout_now(w));
        }
        CMD_BONK => {
            with_world(|n, w| n.bonk_now(w));
        }
        CMD_KEEP => cfg_update(|c| c.keep.enabled = !c.keep.enabled),
        CMD_SHINOBI => cfg_update(|c| c.shinobi = !c.shinobi),
        CMD_PAUSE => cfg_update(|c| c.paused = !c.paused),
        CMD_HIDE => {
            with_app(|a| {
                let h = !a.hidden;
                a.set_hidden(h)
            });
        }
        CMD_HOME => {
            with_app(|a| a.set_hidden(false));
            with_world(|n, w| n.come_here(w));
        }
        CMD_RIDE => {
            with_world(|n, w| n.ride_now(w));
        }
        CMD_EXIT => quit(),
        c if (CMD_SKIN..CMD_SKIN + 100).contains(&c) => {
            let id = with_app(|a| a.skins.get(c - CMD_SKIN).map(|s| s.id.clone())).flatten();
            if let Some(id) = id {
                cfg_update(|cfg| cfg.skin = id);
            }
        }
        c if (CMD_JUTSU..CMD_JUTSU + 5).contains(&c) => {
            let k = [JutsuK::Orb, JutsuK::Shuriken, JutsuK::Kunai, JutsuK::Clones, JutsuK::Fire][c - CMD_JUTSU];
            with_world(|n, w| n.jutsu_now(k, w));
        }
        _ => {}
    }
}

pub fn quit() {
    cfg_get(|c| c.save());
    let host = with_app(|a| {
        a.stop.store(true, Ordering::Relaxed);
        a.host
    });
    control::close();
    if let Some(h) = host {
        unsafe {
            DestroyWindow(h);
        }
    }
}

// ---------------------------------------------------------------------------
// window procedures
// ---------------------------------------------------------------------------

fn tray(host: HWND, add: bool) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = host;
        nid.uID = 1;
        if add {
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_APP_TRAY;
            nid.hIcon = LoadIconW(GetModuleHandleW(std::ptr::null()), 1 as _);
            let tip = wide("RamRonin - right-click for menu");
            nid.szTip[..tip.len()].copy_from_slice(&tip);
            Shell_NotifyIconW(NIM_ADD, &nid);
        } else {
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

fn frame() {
    CFG.with(|c| {
        let Ok(cfg) = c.try_borrow() else { return };
        APP.with(|a| {
            let Ok(mut a) = a.try_borrow_mut() else { return };
            if let Some(app) = a.as_mut() {
                app.frame(&cfg);
            }
        })
    });
}

unsafe extern "system" fn host_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp == TIMER_FRAME => {
            frame();
            0
        }
        WM_HOTKEY => {
            match wp {
                1 => control::open(),
                2 => handle_cmd(CMD_HIDE),
                _ => {}
            }
            0
        }
        WM_APP_TRAY => {
            match (lp & 0xffff) as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(h),
                WM_LBUTTONDBLCLK | WM_LBUTTONUP => control::open(),
                _ => {}
            }
            0
        }
        WM_APP_SHOW => {
            with_app(|a| a.set_hidden(false));
            control::open();
            0
        }
        WM_APP_QUIT => {
            quit();
            0
        }
        WM_APP_CMD => {
            handle_cmd(wp);
            0
        }
        WM_COPYDATA => {
            // `--cmd whop <exe>` from a second process
            let cds = &*(lp as *const COPYDATASTRUCT);
            if cds.dwData == COPYDATA_WHOP && !cds.lpData.is_null() {
                let bytes = std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
                let name = String::from_utf8_lossy(bytes).to_string();
                with_world(|n, w| n.whop_app(&name, w));
            }
            1
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            cfg_get(|c| c.save());
            0
        }
        WM_DESTROY => {
            tray(h, false);
            UnregisterHotKey(h, 1);
            UnregisterHotKey(h, 2);
            PostQuitMessage(0);
            0
        }
        _ => {
            let tb = with_app(|a| a.taskbar_msg).unwrap_or(0);
            if tb != 0 && msg == tb {
                tray(h, true);
                return 0;
            }
            DefWindowProcW(h, msg, wp, lp)
        }
    }
}

unsafe extern "system" fn overlay_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let cur = || {
        let (x, y) = win::cursor_pos();
        (x as f32, y as f32)
    };
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN => {
            SetCapture(h);
            let p = cur();
            with_app(|a| a.ninja.mouse_down(p));
            0
        }
        WM_MOUSEMOVE => {
            if GetCapture() == h {
                let p = cur();
                with_world(|n, w| n.mouse_move(p, w));
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            let p = cur();
            let click = with_app(|a| a.ninja.mouse_up(p)).unwrap_or(false);
            if click {
                with_world(|n, w| n.click(w));
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            with_world(|n, w| n.callout_now(w));
            0
        }
        WM_RBUTTONUP => {
            let host = with_app(|a| a.host);
            if let Some(host) = host {
                show_menu(host);
            }
            0
        }
        WM_CAPTURECHANGED => {
            let p = cur();
            with_app(|a| a.ninja.mouse_up(p));
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

fn register_class(name: &str, proc_: WNDPROC, cursor: PCWSTR, dbl: bool) {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let cls = wide(name);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: if dbl { CS_DBLCLKS } else { 0 },
            lpfnWndProc: proc_,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: LoadIconW(hinst, 1 as _),
            hCursor: LoadCursorW(std::ptr::null_mut(), cursor),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: LoadIconW(hinst, 1 as _),
        };
        RegisterClassExW(&wc);
    }
}

use windows_sys::core::PCWSTR;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[{:?}] RamRonin {} crashed: {}\n", std::time::SystemTime::now(), system::VERSION, info);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data_dir().join("crash.log"))
            .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
    }));

    let args: Vec<String> = std::env::args().collect();
    let has = |a: &str| args.iter().any(|x| x.eq_ignore_ascii_case(a));
    if has("--uninstall") {
        system::uninstall_flow();
        return;
    }
    // `RamRonin.exe --cmd bonk` etc: send a command to the running instance
    if let Some(i) = args.iter().position(|a| a.eq_ignore_ascii_case("--cmd")) {
        let name = args.get(i + 1).map(|s| s.to_lowercase()).unwrap_or_default();
        if name == "whop" {
            // `--cmd whop chrome.exe`: whop that specific app
            let target = args.get(i + 2).cloned().unwrap_or_default();
            unsafe {
                let h = FindWindowW(wide(system::HOST_CLASS).as_ptr(), std::ptr::null());
                if !h.is_null() && !target.is_empty() {
                    let cds = COPYDATASTRUCT { dwData: COPYDATA_WHOP, cbData: target.len() as u32, lpData: target.as_ptr() as *mut c_void };
                    SendMessageW(h, WM_COPYDATA, 0, &cds as *const _ as LPARAM);
                }
            }
            return;
        }
        let id = match name.as_str() {
            "control" | "settings" => CMD_CONTROL,
            "callout" | "memory" => CMD_CALLOUT,
            "bonk" => CMD_BONK,
            "keep" => CMD_KEEP,
            "shinobi" => CMD_SHINOBI,
            "pause" => CMD_PAUSE,
            "hide" | "show" => CMD_HIDE,
            "home" => CMD_HOME,
            "ride" => CMD_RIDE,
            "exit" | "quit" => CMD_EXIT,
            "orb" | "rasengan" => CMD_JUTSU,
            "shuriken" => CMD_JUTSU + 1,
            "kunai" => CMD_JUTSU + 2,
            "clones" => CMD_JUTSU + 3,
            "fire" => CMD_JUTSU + 4,
            _ => 0,
        };
        unsafe {
            let h = FindWindowW(wide(system::HOST_CLASS).as_ptr(), std::ptr::null());
            if id != 0 && !h.is_null() {
                PostMessageW(h, WM_APP_CMD, id, 0);
            }
        }
        return;
    }
    // started from inside a packaged app (e.g. a Store app's terminal)? Then
    // Windows would sandbox our files and registry: relaunch via the shell.
    if args.len() <= 1 && system::break_out_of_package() {
        return;
    }
    system::cleanup_old();
    let after_swap = has("--updated") || has("--installed");
    if !after_swap && system::other_instance_window() {
        system::signal_existing();
        return;
    }
    if !system::acquire_single_instance(after_swap) {
        system::signal_existing();
        return;
    }

    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES | ICC_BAR_CLASSES | ICC_STANDARD_CLASSES | ICC_WIN95_CLASSES,
        };
        InitCommonControlsEx(&icc);
    }

    let cfg = Config::load();
    CFG.with(|c| *c.borrow_mut() = cfg.clone());
    if !cfg.packs_seeded {
        skins::seed_examples();
    }
    let skins = skins::load_all();
    let look = App::look_for(&skins, &cfg.skin);

    register_class(system::HOST_CLASS, Some(host_proc), IDC_ARROW, false);
    register_class(OVERLAY_CLASS, Some(overlay_proc), IDC_HAND, true);
    control::register();

    let (host, overlay) = unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let host = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            wide(system::HOST_CLASS).as_ptr(),
            wide("RamRonin").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        let overlay = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wide(OVERLAY_CLASS).as_ptr(),
            wide("RamRonin").as_ptr(),
            WS_POPUP,
            0,
            0,
            32,
            32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        (host, overlay)
    };
    if host.is_null() || overlay.is_null() {
        win::message_box(std::ptr::null_mut(), "RamRonin could not create its windows.", "RamRonin", MB_ICONERROR);
        return;
    }

    let stop = Arc::new(AtomicBool::new(false));
    let mem: memory::Shared = Arc::new(Mutex::new(memory::MemSnap::default()));
    let keep: keepalive::Shared = Arc::new(Mutex::new(keepalive::KeepState { cfg: cfg.keep.clone(), ..Default::default() }));
    memory::spawn(mem.clone(), stop.clone());
    keepalive::spawn(keep.clone(), mem.clone(), stop.clone());
    let tb: taskbar::Shared = Arc::new(Mutex::new(vec![]));
    taskbar::spawn(tb.clone(), stop.clone());

    let mut tracker = Tracker::new();
    tracker.own = vec![host as isize, overlay as isize];
    let mut ninja = Ninja::new(&tracker);
    if !cfg.welcomed {
        ninja.greet();
    } else if has("--updated") {
        ninja.say(vec![format!("Updated to v{}!", system::VERSION), "New moves unlocked.".into()], 5.0, false);
    } else if has("--installed") {
        ninja.say(vec!["Installed! Find me in the Start Menu.".into()], 5.0, false);
    }
    let autostart = system::autostart_enabled();
    let taskbar_msg = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    let app = App {
        host,
        overlay,
        ninja,
        tracker,
        mem,
        keep,
        tb,
        tb_cache: vec![],
        user_active_at: Instant::now(),
        last_beat: None,
        stop: stop.clone(),
        skins,
        look,
        gfx: Gfx::new(64, 64),
        dib: None,
        last: Instant::now(),
        hidden: false,
        save_due: None,
        last_topmost: Instant::now(),
        mem_cache: memory::sample(),
        mem_cache_at: Instant::now(),
        fps: cfg.fps,
        autostart,
        taskbar_msg,
        shown: false,
        debug: std::env::var_os("RAMRONIN_DEBUG").is_some() || debug_flag_file().is_some(),
        last_debug: Instant::now(),
        tb_logged: 0,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    if !cfg.welcomed || autostart != cfg.autostart || !cfg.packs_seeded {
        cfg_update(|c| {
            c.welcomed = true;
            c.autostart = autostart;
            c.packs_seeded = true;
        });
    }

    unsafe {
        tray(host, true);
        RegisterHotKey(host, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'R' as u32);
        RegisterHotKey(host, 2, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'H' as u32);
        SetTimer(host, TIMER_FRAME, 1000 / cfg.fps.clamp(15, 60), None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            let cc = control::hwnd();
            if !cc.is_null() && IsDialogMessageW(cc, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    stop.store(true, Ordering::Relaxed);
    cfg_get(|c| c.save());
    std::thread::sleep(Duration::from_millis(150));
    system::release_single_instance();
    // relaunch requested by install/update?
    if let Some((path, args)) = control::take_relaunch() {
        system::launch(&path, &args);
    }
}
