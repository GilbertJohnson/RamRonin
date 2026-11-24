//! Control Center: a native, tabbed settings window.
//!
//! Tabs: Character | Props & Jutsu | Memory | Keep-Active | Behavior | About & Updates
//! Most controls are declared as "bindings" (checkbox/slider <-> config field),
//! so adding a new setting is a one-liner.

use crate::config::Config;
use crate::gfx::Gfx;
use crate::props::{self, Item};
use crate::rig::{self, poses, FigFx};
use crate::util::*;
use crate::{cfg_get, cfg_update, system, win, with_app};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS: &str = "RamRoninControl";
const PREVIEW_CLASS: &str = "RamRoninPreview";
const TIMER_UI: usize = 7;
const W: i32 = 840;
const H: i32 = 640;
const HEADER: i32 = 66;

const ID_SKIN: u16 = 100;
const ID_IMPORT: u16 = 101;
const ID_PACKS: u16 = 102;
const ID_RELOAD: u16 = 103;
const ID_TEST_BONK: u16 = 104;
const ID_TEST_JUTSU: u16 = 105;
const ID_ADD: u16 = 106;
const ID_REMOVE: u16 = 107;
const ID_ADD_NAME: u16 = 108;
const ID_IGNORE_SAVE: u16 = 109;
const ID_RESET: u16 = 110;
const ID_HOME: u16 = 111;
const ID_INSTALL: u16 = 112;
const ID_UNINSTALL: u16 = 113;
const ID_UPDATE: u16 = 114;
const ID_CFGDIR: u16 = 115;
const ID_TEST_CALLOUT: u16 = 116;
const ID_LB_RUNNING: u16 = 117;
const ID_LB_KEPT: u16 = 118;
const ID_ADD_TEAMS: u16 = 120;
const ID_TAB: u16 = 121;
const ID_KEEP_TEST: u16 = 122;
const ID_IMPORT_GIF: u16 = 123;
const ID_BIND_BASE: u16 = 2000;

type GetB = fn(&Config) -> bool;
type SetB = fn(&mut Config, bool);
type GetI = fn(&Config) -> i32;
type SetI = fn(&mut Config, i32);
type Fmt = fn(i32) -> String;

enum BindKind {
    Check(GetB, SetB),
    Slider { label: HWND, title: &'static str, get: GetI, set: SetI, fmt: Fmt },
}
struct Bind {
    hwnd: HWND,
    kind: BindKind,
}

struct Cc {
    hwnd: HWND,
    fonts: Vec<HFONT>,
    font_big: HFONT,
    binds: Vec<Bind>,
    pages: Vec<Vec<HWND>>,
    tab: HWND,
    skin_combo: HWND,
    skin_ids: Vec<String>,
    preview: HWND,
    lb_mem: HWND,
    st_mem: HWND,
    lb_running: HWND,
    lb_kept: HWND,
    ed_keep: HWND,
    ed_ignore: HWND,
    st_keep: HWND,
    st_test: HWND,
    bt_test: HWND,
    st_install: HWND,
    running: Vec<String>,
    mem_lines: Vec<String>,
    running_lines: Vec<String>,
    kept_lines: Vec<String>,
    last_refresh: Instant,
}

thread_local! {
    static CC: RefCell<Option<Cc>> = const { RefCell::new(None) };
    static RELAUNCH: RefCell<Option<(PathBuf, String)>> = const { RefCell::new(None) };
    static PREVIEW: RefCell<Option<Gfx>> = const { RefCell::new(None) };
    static T0: Instant = Instant::now();
}

pub fn hwnd() -> HWND {
    CC.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| c.hwnd)).unwrap_or(std::ptr::null_mut()))
}

pub fn take_relaunch() -> Option<(PathBuf, String)> {
    RELAUNCH.with(|r| r.borrow_mut().take())
}

pub fn register() {
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        for (name, proc_, bg) in [(CLASS, Some(cc_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT), WHITE_BRUSH), (PREVIEW_CLASS, Some(preview_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT), NULL_BRUSH)] {
            let cls = wide(name);
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: proc_,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: LoadIconW(hinst, 1 as _),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(bg) as HBRUSH,
                lpszMenuName: std::ptr::null(),
                lpszClassName: cls.as_ptr(),
                hIconSm: LoadIconW(hinst, 1 as _),
            };
            RegisterClassExW(&wc);
        }
    }
}

pub fn open() {
    let h = hwnd();
    unsafe {
        if !h.is_null() {
            ShowWindow(h, SW_RESTORE);
            bring_to_front(h);
            return;
        }
        // size for the monitor the cursor is on
        let (cx, cy) = win::cursor_pos();
        let mons = win::monitors();
        let m = mons.iter().find(|m| m.rect.contains(cx as f32, cy as f32)).unwrap_or(&mons[0]);
        let sc = m.dpi as f32 / 96.0;
        let mut r = RECT { left: 0, top: 0, right: (W as f32 * sc) as i32, bottom: (H as f32 * sc) as i32 };
        // Owned by the hidden host window and without WS_EX_APPWINDOW, so it
        // never gets a taskbar button: RamRonin lives in the tray only.
        let owner = crate::with_app(|a| a.host).unwrap_or(std::ptr::null_mut());
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
        AdjustWindowRectEx(&mut r, style, 0, 0);
        let (ww, hh) = (r.right - r.left, r.bottom - r.top);
        let x = (m.work.l + m.work.r) / 2 - ww / 2;
        let y = ((m.work.t + m.work.b) / 2 - hh / 2).max(m.work.t);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            wide(CLASS).as_ptr(),
            wide("RamRonin Control Center").as_ptr(),
            style,
            x,
            y,
            ww,
            hh,
            owner,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        // register as "own" so the ninja doesn't perch on his own settings... or does he?
        let cc = build(hwnd);
        CC.with(|c| *c.borrow_mut() = Some(cc));
        sync_from_cfg();
        refresh_lists(true);
        SetTimer(hwnd, TIMER_UI, 50, None);
        ShowWindow(hwnd, SW_SHOW);
        bring_to_front(hwnd);
    }
}

/// Make sure the Control Center ends up in front of other windows. It has
/// no taskbar button, so if Windows' focus-stealing rule refuses
/// SetForegroundWindow it would open hidden behind the active window. A
/// brief topmost toggle puts it on top even then.
unsafe fn bring_to_front(h: HWND) {
    if SetForegroundWindow(h) != 0 && GetForegroundWindow() == h {
        return;
    }
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
    SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, flags);
    SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
    SetForegroundWindow(h);
}

pub fn close() {
    let h = hwnd();
    if !h.is_null() {
        unsafe {
            DestroyWindow(h);
        }
    }
}

// ---------------------------------------------------------------------------
// building
// ---------------------------------------------------------------------------

struct B {
    parent: HWND,
    font: HFONT,
    font_bold: HFONT,
    font_small: HFONT,
    sc: f32,
    page: usize,
    pages: Vec<Vec<HWND>>,
    binds: Vec<Bind>,
    next_id: u16,
}

impl B {
    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.sc) as i32
    }
    fn ctl(&mut self, class: &str, text: &str, style: u32, ex: u32, x: i32, y: i32, w: i32, h: i32, id: u16) -> HWND {
        let vis = if self.page == 0 { WS_VISIBLE } else { 0 };
        let hw = unsafe {
            CreateWindowExW(
                ex,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | vis | style,
                self.s(x),
                self.s(y),
                self.s(w),
                self.s(h),
                self.parent,
                id as usize as HMENU,
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        unsafe {
            SendMessageW(hw, WM_SETFONT, self.font as WPARAM, 1);
        }
        self.pages[self.page].push(hw);
        hw
    }
    fn label(&mut self, x: i32, y: i32, w: i32, h: i32, text: &str) -> HWND {
        self.ctl("STATIC", text, 0x80u32 /* SS_NOPREFIX */, 0, x, y, w, h, 0)
    }
    fn heading(&mut self, x: i32, y: i32, w: i32, text: &str) -> HWND {
        let h = self.label(x, y, w, 22, text);
        unsafe {
            SendMessageW(h, WM_SETFONT, self.font_bold as WPARAM, 1);
        }
        h
    }
    fn note(&mut self, x: i32, y: i32, w: i32, h: i32, text: &str) -> HWND {
        let hw = self.label(x, y, w, h, text);
        unsafe {
            SendMessageW(hw, WM_SETFONT, self.font_small as WPARAM, 1);
        }
        hw
    }
    fn button(&mut self, x: i32, y: i32, w: i32, h: i32, text: &str, id: u16) -> HWND {
        self.ctl("BUTTON", text, (BS_PUSHBUTTON as u32) | WS_TABSTOP, 0, x, y, w, h, id)
    }
    fn check(&mut self, x: i32, y: i32, w: i32, text: &str, get: GetB, set: SetB) -> HWND {
        let id = self.next_id;
        self.next_id += 1;
        let h = self.ctl("BUTTON", text, (BS_AUTOCHECKBOX as u32) | WS_TABSTOP, 0, x, y, w, 24, id);
        self.binds.push(Bind { hwnd: h, kind: BindKind::Check(get, set) });
        h
    }
    #[allow(clippy::too_many_arguments)]
    fn slider(&mut self, x: i32, y: i32, w: i32, title: &'static str, min: i32, max: i32, step: i32, get: GetI, set: SetI, fmt: Fmt) -> HWND {
        let label = self.label(x, y, w, 20, title);
        let id = self.next_id;
        self.next_id += 1;
        let h = self.ctl(
            "msctls_trackbar32",
            "",
            (TBS_HORZ | TBS_NOTICKS) as u32 | WS_TABSTOP,
            0,
            x - 4,
            y + 20,
            w + 8,
            28,
            id,
        );
        unsafe {
            SendMessageW(h, TBM_SETRANGEMIN, 0, min as LPARAM);
            SendMessageW(h, TBM_SETRANGEMAX, 1, max as LPARAM);
            SendMessageW(h, TBM_SETLINESIZE, 0, step as LPARAM);
            SendMessageW(h, TBM_SETPAGESIZE, 0, (step * 5) as LPARAM);
        }
        self.binds.push(Bind { hwnd: h, kind: BindKind::Slider { label, title, get, set, fmt } });
        h
    }
    fn listbox(&mut self, x: i32, y: i32, w: i32, h: i32, id: u16, multi: bool) -> HWND {
        let style = (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32 | WS_VSCROLL | WS_TABSTOP | if multi { LBS_EXTENDEDSEL as u32 } else { 0 };
        self.ctl("LISTBOX", "", style, WS_EX_CLIENTEDGE, x, y, w, h, id)
    }
    fn edit(&mut self, x: i32, y: i32, w: i32, h: i32, text: &str, id: u16) -> HWND {
        self.ctl("EDIT", text, (ES_AUTOHSCROLL as u32) | WS_TABSTOP, WS_EX_CLIENTEDGE, x, y, w, h, id)
    }
}

fn make_font(sc: f32, pt: f32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -((pt * 96.0 / 72.0 * sc) as i32),
            0,
            0,
            0,
            if bold { 600 } else { 400 },
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            wide("Segoe UI").as_ptr(),
        )
    }
}

fn pct(v: i32) -> String {
    format!("{}%", v)
}
fn secs(v: i32) -> String {
    if v >= 60 {
        format!("{}m {:02}s", v / 60, v % 60)
    } else {
        format!("{}s", v)
    }
}

fn build(hwnd: HWND) -> Cc {
    let sc = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let font = make_font(sc, 9.5, false);
    let font_bold = make_font(sc, 10.0, true);
    let font_small = make_font(sc, 8.5, false);
    let font_big = make_font(sc, 17.0, true);
    let mut b = B { parent: hwnd, font, font_bold, font_small, sc, page: 0, pages: vec![vec![]; 6], binds: vec![], next_id: ID_BIND_BASE };

    // tab strip
    let tab = unsafe {
        let t = CreateWindowExW(
            0,
            wide("SysTabControl32").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_TABSTOP,
            b.s(14),
            b.s(HEADER + 8),
            b.s(W - 28),
            b.s(30),
            hwnd,
            ID_TAB as usize as HMENU,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        SendMessageW(t, WM_SETFONT, font as WPARAM, 1);
        for (i, name) in ["  Character  ", "  Props && Jutsu  ", "  Memory  ", "  Keep-Active  ", "  Behavior  ", "  About && Updates  "].iter().enumerate() {
            let mut txt = wide(name);
            let mut item: TCITEMW = zeroed();
            item.mask = TCIF_TEXT;
            item.pszText = txt.as_mut_ptr();
            SendMessageW(t, TCM_INSERTITEMW, i, &item as *const _ as LPARAM);
        }
        t
    };
    let y0 = HEADER + 50;

    // ---------------- page 0: Character ----------------
    b.page = 0;
    b.heading(24, y0, 280, "Character");
    let skin_combo = b.ctl("COMBOBOX", "", (CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP, 0, 24, y0 + 26, 290, 320, ID_SKIN);
    let preview = b.ctl(PREVIEW_CLASS, "", 0, WS_EX_CLIENTEDGE, 24, y0 + 62, 290, 300, 0);
    b.button(24, y0 + 372, 140, 30, "Import pack...", ID_IMPORT);
    b.button(174, y0 + 372, 140, 30, "Open packs folder", ID_PACKS);
    b.button(24, y0 + 408, 140, 30, "Reload packs", ID_RELOAD);
    b.button(174, y0 + 408, 140, 30, "Import GIF...", ID_IMPORT_GIF);
    let x1 = 344;
    b.heading(x1, y0, 460, "Look & feel");
    b.slider(x1, y0 + 28, 460, "Size", 50, 250, 5, |c| c.size_pct as i32, |c, v| c.size_pct = v as u32, pct);
    b.slider(x1, y0 + 86, 460, "Speed", 40, 250, 5, |c| c.speed_pct as i32, |c, v| c.speed_pct = v as u32, pct);
    b.slider(x1, y0 + 144, 460, "Unhinged level", 0, 100, 5, |c| c.chaos as i32, |c, v| c.chaos = v as u32, |v| {
        let mood = match v {
            0..=20 => "calm monk",
            21..=45 => "playful",
            46..=70 => "spicy",
            71..=90 => "unhinged",
            _ => "absolute gremlin",
        };
        format!("{}% ({})", v, mood)
    });
    b.check(x1, y0 + 210, 460, "Shinobi Edition - jutsu, tomoe eye, body flicker, ninja run", |c| c.shinobi, |c, v| c.shinobi = v);
    b.check(x1, y0 + 238, 460, "Speed lines && afterimages", |c| c.speed_fx, |c, v| c.speed_fx = v);
    b.check(x1, y0 + 266, 460, "Memory text clouds", |c| c.clouds, |c, v| c.clouds = v);
    b.note(
        x1,
        y0 + 304,
        460,
        130,
        "Character packs: a pack is a small .json file describing a character \
(colours, head, eyes, clothes, speed, favourite weapon and catch-phrases). \
Import GIF... turns any animated GIF into a character that runs, perches and whops like the ninja. \
GIFs with a plain or transparent background look best; others are shown as a sticker.",
    );

    // ---------------- page 1: Props & Jutsu ----------------
    b.page = 1;
    b.heading(24, y0, 240, "Weapons");
    let props_list: [(&str, GetB, SetB); 7] = [
        ("Whip", |c| c.props.whip, |c, v| c.props.whip = v),
        ("Giant mallet", |c| c.props.mallet, |c, v| c.props.mallet = v),
        ("Wrench", |c| c.props.wrench, |c, v| c.props.wrench = v),
        ("Crowbar", |c| c.props.crowbar, |c, v| c.props.crowbar = v),
        ("Boxing gloves", |c| c.props.gloves, |c, v| c.props.gloves = v),
        ("Rubber chicken", |c| c.props.chicken, |c, v| c.props.chicken = v),
        ("Megaphone (for callouts)", |c| c.props.megaphone, |c, v| c.props.megaphone = v),
    ];
    for (i, (t, g, s)) in props_list.iter().enumerate() {
        b.check(24, y0 + 28 + i as i32 * 28, 240, t, *g, *s);
    }
    b.heading(290, y0, 230, "Rides & stunts");
    let rides: [(&str, GetB, SetB); 5] = [
        ("Horse", |c| c.props.horse, |c, v| c.props.horse = v),
        ("Car", |c| c.props.car, |c, v| c.props.car = v),
        ("Pogo stick", |c| c.props.pogo, |c, v| c.props.pogo = v),
        ("Jetpack", |c| c.props.jetpack, |c, v| c.props.jetpack = v),
        ("Stunts && dancing", |c| c.stunts, |c, v| c.stunts = v),
    ];
    for (i, (t, g, s)) in rides.iter().enumerate() {
        b.check(290, y0 + 28 + i as i32 * 28, 230, t, *g, *s);
    }
    b.heading(540, y0, 280, "Jutsu (Shinobi Edition)");
    let jutsu: [(&str, GetB, SetB); 8] = [
        ("Spiral chakra orb (Rasengan-style)", |c| c.jutsu.orb, |c, v| c.jutsu.orb = v),
        ("Wind shuriken (Rasen-shuriken-style)", |c| c.jutsu.shuriken, |c, v| c.jutsu.shuriken = v),
        ("Tomoe eye (Sharingan-style)", |c| c.jutsu.eye, |c, v| c.jutsu.eye = v),
        ("Kunai volley", |c| c.jutsu.kunai, |c, v| c.jutsu.kunai = v),
        ("Shadow clones", |c| c.jutsu.clones, |c, v| c.jutsu.clones = v),
        ("Substitution log (when grabbed)", |c| c.jutsu.substitution, |c, v| c.jutsu.substitution = v),
        ("Fire breath", |c| c.jutsu.fire, |c, v| c.jutsu.fire = v),
        ("Body-flicker teleport", |c| c.jutsu.flicker, |c, v| c.jutsu.flicker = v),
    ];
    for (i, (t, g, s)) in jutsu.iter().enumerate() {
        b.check(540, y0 + 28 + i as i32 * 28, 290, t, *g, *s);
    }
    b.heading(24, y0 + 290, 400, "Try it");
    b.button(24, y0 + 318, 170, 32, "Bonk top hog now", ID_TEST_BONK);
    b.button(204, y0 + 318, 170, 32, "Random jutsu now", ID_TEST_JUTSU);
    b.button(384, y0 + 318, 170, 32, "Memory callout now", ID_TEST_CALLOUT);
    b.note(
        24,
        y0 + 370,
        780,
        60,
        "Bonks and jutsu are purely visual - RamRonin never closes, kills or slows down any app. \
Jutsu effects are original drawings inspired by ninja anime; this is a fan-made toy with no affiliation.",
    );

    // ---------------- page 2: Memory ----------------
    b.page = 2;
    b.heading(24, y0, 380, "When to bonk");
    b.slider(24, y0 + 28, 380, "Bonk apps using more than", 250, 8000, 50, |c| c.bonk_mb as i32, |c, v| c.bonk_mb = v as u32, |v| {
        if v >= 1000 {
            format!("{:.2} GB", v as f32 / 1024.0)
        } else {
            format!("{} MB", v)
        }
    });
    b.slider(24, y0 + 86, 380, "...or whenever system RAM is above", 50, 99, 1, |c| c.sys_alarm as i32, |c, v| c.sys_alarm = v as u32, pct);
    b.slider(24, y0 + 144, 380, "Bonk cooldown", 15, 900, 5, |c| c.bonk_cooldown_s as i32, |c, v| c.bonk_cooldown_s = v as u32, secs);
    b.slider(24, y0 + 202, 380, "Memory cloud every", 15, 600, 5, |c| c.callout_s as i32, |c, v| c.callout_s = v as u32, secs);
    b.slider(24, y0 + 260, 380, "Nap when RAM is below", 10, 95, 1, |c| c.nap_below as i32, |c, v| c.nap_below = v as u32, pct);
    b.label(24, y0 + 324, 380, 20, "Never bonk these apps (comma-separated exe names):");
    let ed_ignore = b.edit(24, y0 + 346, 300, 26, "", 0);
    b.button(332, y0 + 345, 72, 28, "Save", ID_IGNORE_SAVE);
    b.heading(434, y0, 380, "Live memory");
    let st_mem = b.label(434, y0 + 26, 380, 40, "Reading...");
    let lb_mem = b.listbox(434, y0 + 70, 380, 380, 0, false);
    let mono = unsafe {
        CreateFontW(-((9.0 * 96.0 / 72.0 * sc) as i32), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, (FIXED_PITCH | FF_MODERN) as u32, wide("Consolas").as_ptr())
    };
    unsafe {
        SendMessageW(lb_mem, WM_SETFONT, mono as WPARAM, 1);
    }

    // ---------------- page 3: Keep-Active ----------------
    b.page = 3;
    b.check(24, y0, 500, "Keep the selected apps active (e.g. Teams stays \"Available\")", |c| c.keep.enabled, |c, v| c.keep.enabled = v);
    b.note(
        24,
        y0 + 28,
        790,
        36,
        "While a selected app is running and you have been idle for a while, RamRonin sends a heartbeat (choose how below) \
so Windows and the app see activity. It never fires while you type or move the mouse. \
Not sure what works for your Teams? Run the self-test while you're away from the PC.",
    );
    b.label(24, y0 + 72, 300, 20, "Apps with open windows:");
    let lb_running = b.listbox(24, y0 + 94, 300, 200, ID_LB_RUNNING, true);
    b.button(336, y0 + 130, 120, 30, "Add  >>", ID_ADD);
    b.button(336, y0 + 168, 120, 30, "<<  Remove", ID_REMOVE);
    b.button(336, y0 + 220, 120, 30, "+ Teams", ID_ADD_TEAMS);
    b.label(468, y0 + 72, 340, 20, "Kept active:");
    let lb_kept = b.listbox(468, y0 + 94, 346, 160, ID_LB_KEPT, true);
    let ed_keep = b.edit(468, y0 + 264, 240, 28, "", 0);
    b.button(714, y0 + 263, 100, 30, "Add by name", ID_ADD_NAME);
    b.slider(24, y0 + 306, 360, "Heartbeat every", 15, 600, 5, |c| c.keep.interval_s as i32, |c, v| c.keep.interval_s = v as u32, secs);
    b.slider(454, y0 + 306, 360, "...but only after you've been idle for", 0, 600, 5, |c| c.keep.idle_s as i32, |c, v| c.keep.idle_s = v as u32, secs);
    b.check(24, y0 + 366, 260, "Keep PC && display awake", |c| c.keep.prevent_sleep, |c, v| c.keep.prevent_sleep = v);
    b.check(290, y0 + 366, 260, "Relaunch an app if it closes", |c| c.keep.relaunch, |c, v| c.keep.relaunch = v);
    b.check(560, y0 + 366, 260, "Ninja visits kept apps", |c| c.keep.visits, |c, v| c.keep.visits = v);
    b.label(24, y0 + 398, 90, 22, "Heartbeat:");
    b.check(114, y0 + 396, 130, "F15 key tap", |c| c.keep.key, |c, v| c.keep.key = v);
    b.check(250, y0 + 396, 240, "Mouse nudge (best for Teams)", |c| c.keep.mouse, |c, v| c.keep.mouse = v);
    b.check(496, y0 + 396, 320, "Bring the app's window forward (3 min)", |c| c.keep.wake, |c, v| c.keep.wake = v);
    let st_keep = b.label(24, y0 + 430, 600, 40, "");
    let bt_test = b.button(640, y0 + 432, 174, 32, "Run self-test...", ID_KEEP_TEST);
    let st_test = b.label(24, y0 + 472, 790, 38, "");

    // ---------------- page 4: Behavior ----------------
    b.page = 4;
    b.heading(24, y0, 380, "Behaviour");
    let beh: [(&str, GetB, SetB); 6] = [
        ("Perch, sit and run on app windows", |c| c.perch, |c, v| c.perch = v),
        ("Climb window walls && stand on ledges inside apps", |c| c.climb, |c, v| c.climb = v),
        ("Nap when RAM is calm", |c| c.nap, |c, v| c.nap = v),
        ("React when windows move or resize", |c| c.react_moves, |c, v| c.react_moves = v),
        ("Pause (he meditates in place)", |c| c.paused, |c, v| c.paused = v),
        ("Start RamRonin with Windows", |c| c.autostart, |c, v| c.autostart = v),
    ];
    for (i, (t, g, s)) in beh.iter().enumerate() {
        b.check(24, y0 + 28 + i as i32 * 28, 380, t, *g, *s);
    }
    b.slider(24, y0 + 206, 380, "Animation frame rate", 15, 60, 5, |c| c.fps as i32, |c, v| c.fps = v as u32, |v| format!("{} fps", v));
    b.slider(24, y0 + 266, 380, "Rest on the taskbar after you're away for", 0, 30, 1, |c| c.rest_after_min as i32, |c, v| c.rest_after_min = v as u32, |v| {
        if v == 0 {
            "never".into()
        } else {
            format!("{} min", v)
        }
    });
    b.button(24, y0 + 330, 190, 32, "Bring him to the mouse", ID_HOME);
    b.button(224, y0 + 330, 180, 32, "Reset all settings", ID_RESET);
    b.heading(434, y0, 380, "Controls");
    b.note(
        434,
        y0 + 28,
        380,
        260,
        "Right-click the ninja (or the tray icon) for the quick menu.\n\n\
Drag him to pick him up - fling him and he flies.\n\
Click him: he reacts. Double-click: memory callout.\n\n\
Ctrl+Alt+R  open this Control Center\n\
Ctrl+Alt+H  hide / show the ninja\n\n\
Running RamRonin.exe again also opens this window.",
    );

    // ---------------- page 5: About & Updates ----------------
    b.page = 5;
    let title = b.label(24, y0, 600, 34, &format!("RamRonin  v{}", system::VERSION));
    unsafe {
        SendMessageW(title, WM_SETFONT, font_big as WPARAM, 1);
    }
    b.note(24, y0 + 38, 780, 40, "An unhinged stickman ninja memory monitor. Single portable exe - no runtime, no admin rights, no telemetry, no network.");
    b.heading(24, y0 + 90, 380, "Install");
    let st_install = b.label(24, y0 + 116, 780, 40, "");
    b.button(24, y0 + 160, 190, 32, "Install on this PC", ID_INSTALL);
    b.button(224, y0 + 160, 150, 32, "Uninstall", ID_UNINSTALL);
    b.heading(24, y0 + 214, 380, "Updates");
    b.note(
        24,
        y0 + 240,
        780,
        40,
        "Got a newer RamRonin.exe? Pick it here: it is checked (must be a RamRonin build), swapped in and restarted. \
Your settings and character packs are kept. (Online updates from GitHub can plug in here later.)",
    );
    b.button(24, y0 + 286, 190, 32, "Update from file...", ID_UPDATE);
    b.button(224, y0 + 286, 190, 32, "Open settings folder", ID_CFGDIR);

    let fonts = vec![font, font_bold, font_small, mono];
    Cc {
        hwnd,
        fonts,
        font_big,
        binds: b.binds,
        pages: b.pages,
        tab,
        skin_combo,
        skin_ids: vec![],
        preview,
        lb_mem,
        st_mem,
        lb_running,
        lb_kept,
        ed_keep,
        ed_ignore,
        st_keep,
        st_test,
        bt_test,
        st_install,
        running: vec![],
        mem_lines: vec![],
        running_lines: vec![],
        kept_lines: vec![],
        last_refresh: Instant::now(),
    }
}

// ---------------------------------------------------------------------------
// sync
// ---------------------------------------------------------------------------

fn set_text(h: HWND, s: &str) {
    // skip unchanged text: avoids flicker on the periodic refresh
    if get_text(h) == s {
        return;
    }
    unsafe {
        SetWindowTextW(h, wide(s).as_ptr());
    }
}
fn get_text(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        let mut buf = vec![0u16; n as usize + 1];
        GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

fn sync_from_cfg() {
    let cfg = cfg_get(|c| c.clone());
    let skins = with_app(|a| a.skins.iter().map(|s| (s.id.clone(), s.name.clone())).collect::<Vec<_>>()).unwrap_or_default();
    CC.with(|c| {
        let Ok(mut c) = c.try_borrow_mut() else { return };
        let Some(cc) = c.as_mut() else { return };
        unsafe {
            for b in &cc.binds {
                match &b.kind {
                    BindKind::Check(g, _) => {
                        SendMessageW(b.hwnd, BM_SETCHECK, if g(&cfg) { BST_CHECKED as usize } else { 0 }, 0);
                    }
                    BindKind::Slider { label, title, get, fmt, .. } => {
                        let v = get(&cfg);
                        SendMessageW(b.hwnd, TBM_SETPOS, 1, v as LPARAM);
                        set_text(*label, &format!("{}:  {}", title, fmt(v)));
                    }
                }
            }
            SendMessageW(cc.skin_combo, CB_RESETCONTENT, 0, 0);
            cc.skin_ids.clear();
            let mut sel = 0;
            for (i, (id, name)) in skins.iter().enumerate() {
                SendMessageW(cc.skin_combo, CB_ADDSTRING, 0, wide(name).as_ptr() as LPARAM);
                if *id == cfg.skin {
                    sel = i;
                }
                cc.skin_ids.push(id.clone());
            }
            SendMessageW(cc.skin_combo, CB_SETCURSEL, sel, 0);
            set_text(cc.ed_ignore, &cfg.ignore.join(", "));
        }
        let inst = if system::running_from_install() {
            format!("Installed in {}  (Start Menu shortcut + Apps & features entry).", system::install_dir().display())
        } else if system::is_installed() {
            format!("An installed copy exists in {}. You're running a portable copy.", system::install_dir().display())
        } else {
            "Running as a portable exe. Install to get a Start Menu shortcut and an Apps & features (uninstall) entry.".to_string()
        };
        set_text(cc.st_install, &inst);
    });
}

fn fill_list(lb: HWND, lines: &[String], cache: &mut Vec<String>) {
    if cache.as_slice() == lines {
        return;
    }
    unsafe {
        // keep scroll position & selection
        let top = SendMessageW(lb, LB_GETTOPINDEX, 0, 0);
        let mut sel = vec![0i32; 256];
        let n = SendMessageW(lb, LB_GETSELITEMS, sel.len(), sel.as_mut_ptr() as LPARAM).max(0) as usize;
        let selected: Vec<String> = sel[..n].iter().filter_map(|&i| cache.get(i as usize).cloned()).collect();
        SendMessageW(lb, WM_SETREDRAW, 0, 0);
        SendMessageW(lb, LB_RESETCONTENT, 0, 0);
        for (i, l) in lines.iter().enumerate() {
            SendMessageW(lb, LB_ADDSTRING, 0, wide(l).as_ptr() as LPARAM);
            if selected.contains(l) {
                SendMessageW(lb, LB_SETSEL, 1, i as LPARAM);
            }
        }
        SendMessageW(lb, LB_SETTOPINDEX, top as usize, 0);
        SendMessageW(lb, WM_SETREDRAW, 1, 0);
        InvalidateRect(lb, std::ptr::null(), 1);
    }
    *cache = lines.to_vec();
}

fn ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    if s < 60 {
        format!("{}s ago", s)
    } else {
        format!("{} min ago", s / 60)
    }
}

fn refresh_lists(force: bool) {
    let cfg = cfg_get(|c| c.clone());
    let data = with_app(|a| {
        let mut names: Vec<String> = a.tracker.wins.iter().chain(a.tracker.minimized.iter()).map(|w| w.name.clone()).filter(|n| !n.is_empty()).collect();
        names.sort();
        names.dedup();
        let keep = a.keep.lock().map(|k| k.clone()).ok();
        (a.mem_cache.clone(), names, keep)
    });
    let Some((mem, names, keep)) = data else { return };
    CC.with(|c| {
        let Ok(mut c) = c.try_borrow_mut() else { return };
        let Some(cc) = c.as_mut() else { return };
        if !force && cc.last_refresh.elapsed().as_millis() < 1500 {
            return;
        }
        cc.last_refresh = Instant::now();
        // memory page
        if mem.ok {
            set_text(
                cc.st_mem,
                &format!("RAM {}% used  -  {} of {}  -  {} free", mem.load, fmt_bytes(mem.used()), fmt_bytes(mem.total), fmt_bytes(mem.avail)),
            );
        }
        let thr = cfg.bonk_mb as u64 * 1024 * 1024;
        let lines: Vec<String> = mem
            .procs
            .iter()
            .filter(|p| p.bytes > 0)
            .take(30)
            .map(|p| {
                let mark = if cfg.ignored(&p.name) {
                    "  (ignored)"
                } else if p.bytes >= thr {
                    " <- bonk"
                } else {
                    ""
                };
                format!("{:<22} {:>9}  x{:<3}{}", pretty_name(&p.name).chars().take(22).collect::<String>(), fmt_bytes(p.bytes), p.count, mark)
            })
            .collect();
        let lb = cc.lb_mem;
        fill_list(lb, &lines, &mut cc.mem_lines);
        // keep-active page
        let running: Vec<String> = names.into_iter().filter(|n| !cfg.keep.apps.contains(n)).collect();
        let rl: Vec<String> = running.iter().map(|n| format!("{}  ({})", pretty_name(n), n)).collect();
        let lbr = cc.lb_running;
        fill_list(lbr, &rl, &mut cc.running_lines);
        cc.running = running;
        let kl: Vec<String> = cfg
            .keep
            .apps
            .iter()
            .map(|a| {
                let st = keep
                    .as_ref()
                    .and_then(|k| k.running.iter().find(|r| &r.0 == a).map(|r| if r.1 { "running" } else { "not running" }))
                    .unwrap_or("...");
                format!("{}  ({})  -  {}", pretty_name(a), a, st)
            })
            .collect();
        let lbk = cc.lb_kept;
        fill_list(lbk, &kl, &mut cc.kept_lines);
        if let Some(k) = keep {
            let mut s = k.note.clone();
            if k.beats > 0 {
                s += &format!("   Heartbeats sent: {}  (last {}).", k.beats, k.last_beat.map(ago).unwrap_or_default());
            }
            if k.relaunches > 0 {
                s += &format!("   Relaunched apps: {}.", k.relaunches);
            }
            s += &format!("   You've been idle {}s.", win::idle_ms() / 1000);
            set_text(cc.st_keep, &s);
            set_text(cc.st_test, &k.test_status);
            set_text(cc.bt_test, if k.test_running { "Stop self-test" } else { "Run self-test..." });
        }
    });
}

fn selected(lb: HWND) -> Vec<usize> {
    unsafe {
        let mut sel = vec![0i32; 256];
        let n = SendMessageW(lb, LB_GETSELITEMS, sel.len(), sel.as_mut_ptr() as LPARAM).max(0) as usize;
        sel[..n].iter().map(|&i| i as usize).collect()
    }
}

fn add_kept(names: Vec<String>) {
    let names: Vec<String> = names.into_iter().map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        return;
    }
    cfg_update(|c| {
        for n in names {
            let n = if n.ends_with(".exe") { n } else { format!("{}.exe", n) };
            if !c.keep.apps.contains(&n) {
                c.keep.apps.push(n);
            }
        }
    });
    refresh_lists(true);
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

fn on_command(id: u16, code: u16, ctl: HWND) {
    let me = hwnd();
    // bindings first
    let bind = CC.with(|c| {
        let c = c.try_borrow().ok()?;
        let cc = c.as_ref()?;
        cc.binds.iter().find(|b| b.hwnd == ctl).map(|b| match b.kind {
            BindKind::Check(_, s) => Some(s),
            _ => None,
        })
    });
    if let Some(Some(set)) = bind {
        if code == BN_CLICKED as u16 {
            let on = unsafe { SendMessageW(ctl, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize;
            cfg_update(|c| set(c, on));
        }
        return;
    }
    match id {
        ID_SKIN if code == CBN_SELCHANGE as u16 => {
            let idx = unsafe { SendMessageW(ctl, CB_GETCURSEL, 0, 0) };
            let sid = CC.with(|c| c.borrow().as_ref().and_then(|cc| cc.skin_ids.get(idx as usize).cloned()));
            if let Some(sid) = sid {
                cfg_update(|c| c.skin = sid);
            }
        }
        ID_IMPORT => {
            if let Some(p) = system::open_file_dialog(me, "Import a RamRonin character pack", "Character packs (*.json)\0*.json\0All files\0*.*\0") {
                if let Some(id) = system::import_pack(me, &p) {
                    crate::reload_skins();
                    cfg_update(|c| c.skin = id);
                    sync_from_cfg();
                }
            }
        }
        ID_IMPORT_GIF => {
            if let Some(p) = system::open_file_dialog(me, "Import an animated GIF as a character", "Animated GIF (*.gif)\0*.gif\0") {
                if let Some(id) = system::import_gif(me, &p) {
                    crate::reload_skins();
                    cfg_update(|c| c.skin = id);
                    sync_from_cfg();
                }
            }
        }
        ID_PACKS => {
            win::shell_open(&packs_dir().to_string_lossy(), None);
        }
        ID_RELOAD => {
            crate::reload_skins();
            sync_from_cfg();
        }
        ID_TEST_BONK => crate::handle_cmd(crate::CMD_BONK),
        ID_TEST_CALLOUT => crate::handle_cmd(crate::CMD_CALLOUT),
        ID_TEST_JUTSU => {
            let k = (Instant::now().elapsed().subsec_nanos() as usize + T0.with(|t| t.elapsed().as_millis() as usize)) % 5;
            crate::handle_cmd(crate::CMD_JUTSU + k);
        }
        ID_ADD => {
            let names = CC.with(|c| {
                c.borrow().as_ref().map(|cc| selected(cc.lb_running).into_iter().filter_map(|i| cc.running.get(i).cloned()).collect::<Vec<_>>())
            });
            add_kept(names.unwrap_or_default());
        }
        ID_ADD_TEAMS => add_kept(vec!["ms-teams.exe".into(), "teams.exe".into()]),
        ID_KEEP_TEST => keep_test_toggle(),
        ID_ADD_NAME => {
            let t = CC.with(|c| c.borrow().as_ref().map(|cc| (get_text(cc.ed_keep), cc.ed_keep)));
            if let Some((t, ed)) = t {
                add_kept(t.split(',').map(|s| s.to_string()).collect());
                set_text(ed, "");
            }
        }
        ID_REMOVE => {
            let sel = CC.with(|c| c.borrow().as_ref().map(|cc| selected(cc.lb_kept)));
            if let Some(sel) = sel {
                cfg_update(|c| {
                    let keep: Vec<String> = c.keep.apps.iter().enumerate().filter(|(i, _)| !sel.contains(i)).map(|(_, a)| a.clone()).collect();
                    c.keep.apps = keep;
                });
                refresh_lists(true);
            }
        }
        ID_LB_RUNNING if code == LBN_DBLCLK as u16 => on_command(ID_ADD, 0, std::ptr::null_mut()),
        ID_LB_KEPT if code == LBN_DBLCLK as u16 => on_command(ID_REMOVE, 0, std::ptr::null_mut()),
        ID_IGNORE_SAVE => {
            let t = CC.with(|c| c.borrow().as_ref().map(|cc| get_text(cc.ed_ignore)));
            if let Some(t) = t {
                let list: Vec<String> = t.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
                cfg_update(|c| c.ignore = list);
                refresh_lists(true);
            }
        }
        ID_HOME => crate::handle_cmd(crate::CMD_HOME),
        ID_RESET => {
            if win::message_box(me, "Reset every RamRonin setting to its default?", "Reset settings", MB_YESNO | MB_ICONQUESTION) == IDYES {
                cfg_update(|c| {
                    *c = Config { welcomed: true, ..Config::default() };
                });
                sync_from_cfg();
            }
        }
        ID_INSTALL => {
            if let Some(dst) = system::install(me) {
                if system::running_from_install() {
                    win::message_box(me, "RamRonin is installed: Start Menu shortcut and Apps & features entry are in place.", "Install", MB_ICONINFORMATION);
                    sync_from_cfg();
                } else {
                    win::message_box(
                        me,
                        &format!("Installed to:\n{}\n\nThe installed copy will start now. You can delete this downloaded exe afterwards.", dst.display()),
                        "Install",
                        MB_ICONINFORMATION,
                    );
                    RELAUNCH.with(|r| *r.borrow_mut() = Some((dst, "--installed".into())));
                    crate::quit();
                }
            }
        }
        ID_UNINSTALL => {
            if !system::is_installed() {
                win::message_box(me, "RamRonin isn't installed on this PC (you're running the portable exe).", "Uninstall", MB_ICONINFORMATION);
            } else {
                system::launch(&system::installed_exe(), "--uninstall");
            }
        }
        ID_UPDATE => {
            if let Some(p) = system::open_file_dialog(me, "Choose the new RamRonin.exe", "RamRonin program (*.exe)\0*.exe\0") {
                if let Some(exe) = system::update_from_file(me, &p) {
                    RELAUNCH.with(|r| *r.borrow_mut() = Some((exe, "--updated".into())));
                    crate::quit();
                }
            }
        }
        ID_CFGDIR => {
            win::shell_open(&data_dir().to_string_lossy(), None);
        }
        _ => {}
    }
}

fn on_scroll(ctl: HWND) {
    let b = CC.with(|c| {
        let c = c.try_borrow().ok()?;
        let cc = c.as_ref()?;
        cc.binds.iter().find(|b| b.hwnd == ctl).and_then(|b| match b.kind {
            BindKind::Slider { label, title, set, fmt, .. } => Some((label, title, set, fmt)),
            _ => None,
        })
    });
    if let Some((label, title, set, fmt)) = b {
        let v = unsafe { SendMessageW(ctl, WM_USER, 0, 0) } as i32;
        set_text(label, &format!("{}:  {}", title, fmt(v)));
        cfg_update(|c| set(c, v));
    }
}

fn show_page(p: usize) {
    CC.with(|c| {
        let Ok(c) = c.try_borrow() else { return };
        let Some(cc) = c.as_ref() else { return };
        for (i, ctrls) in cc.pages.iter().enumerate() {
            for h in ctrls {
                unsafe {
                    ShowWindow(*h, if i == p { SW_SHOW } else { SW_HIDE });
                }
            }
        }
    });
    refresh_lists(true);
}

// ---------------------------------------------------------------------------
// painting
// ---------------------------------------------------------------------------

unsafe fn blit(hdc: HDC, g: &Gfx, x: i32, y: i32) {
    let (w, h) = (g.w() as i32, g.h() as i32);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    g.copy_bgra(&mut buf);
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    SetDIBitsToDevice(hdc, x, y, w as u32, h as u32, 0, 0, 0, h as u32, buf.as_ptr() as *const c_void, &bmi, DIB_RGB_COLORS);
}

/// Animated character demo used by the preview box and the header.
fn draw_demo(g: &mut Gfx, w: f32, h: f32, t: f32, sc: f32, compact: bool) {
    let Some((look, shinobi, eye)) = with_app(|a| (a.look.clone(), cfg_get(|c| c.shinobi), cfg_get(|c| c.jutsu.eye))) else { return };
    let s = if compact { 0.62 * sc } else { 1.55 * sc * look.size.min(1.4) };
    let ground = (w / 2.0, h - if compact { 6.0 } else { 42.0 } * sc);
    if !compact {
        g.line((14.0, ground.1), (w - 14.0, ground.1), 2.0 * sc, Col::rgb(205, 208, 216));
    }
    let cycle = if compact { 2.0 } else { t % 14.0 };
    let phase = t * 9.0;
    let mut item = Item::None;
    let mut swing = 0.0;
    let mut vel = (0.0, 0.0);
    let mut facing = 1.0;
    let pose = if compact {
        poses::stance(t)
    } else if cycle < 2.5 {
        poses::stance(t)
    } else if cycle < 5.0 {
        vel = (520.0, 0.0);
        look.sprint(shinobi || look.ninja_run, phase)
    } else if cycle < 6.0 {
        let u = cycle - 5.0;
        let mut p = poses::tuck(-360.0 * ease_io(u));
        p.dy = -(u * std::f32::consts::PI).sin() * 40.0;
        p
    } else if cycle < 8.6 {
        item = match look.favorite.as_str() {
            "mallet" => Item::Mallet,
            "wrench" => Item::Wrench,
            "gloves" => Item::Gloves,
            "chicken" => Item::Chicken,
            "crowbar" => Item::Crowbar,
            _ => Item::Whip,
        };
        let per = if item == Item::Gloves { 0.42 } else { 0.6 };
        swing = ((cycle - 6.0) % per) / per;
        if item == Item::Gloves {
            poses::punches(swing)
        } else {
            poses::swing(swing)
        }
    } else if cycle < 11.4 {
        if shinobi {
            poses::charge(t)
        } else {
            poses::dance(t, 0.7)
        }
    } else if cycle < 12.4 {
        facing = -1.0;
        poses::taunt(t)
    } else {
        poses::sit(t)
    };
    let root = if !compact && cycle >= 12.4 { (ground.0, ground.1 - 2.0) } else { ground };
    let sk = rig::solve(&pose, s, facing, root);
    if vel.0 > 0.0 {
        // speed lines in the demo
        for i in 0..6 {
            let y = ground.1 - (8.0 + i as f32 * 13.0) * s;
            let off = ((t * 3.0 + i as f32 * 0.37) % 1.0) * 90.0 * s;
            g.line((root.0 - 40.0 * s - off - 50.0 * s, y), (root.0 - 40.0 * s - off, y), 1.6 * sc, Col::rgba(120, 120, 130, 120));
        }
    }
    let fx = FigFx { t, vel, tomoe: shinobi && eye && (item != Item::None || vel.0 > 0.0 || (cycle >= 8.6 && cycle < 11.4)) };
    rig::draw_figure(g, &sk, &look, &fx);
    props::draw_item(g, item, &sk, t, swing);
    if !compact && shinobi && (8.6..11.4).contains(&cycle) {
        let r = 11.0 * s * ((cycle - 8.6) / 1.0).min(1.0);
        let hpt = sk.hand_f;
        props::chakra_orb(g, (hpt.0 + r * 0.6, hpt.1 + r * 0.2), r.max(2.0), t, look.aura);
    }
}

unsafe extern "system" fn preview_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(h, &mut ps);
            let mut rc: RECT = zeroed();
            GetClientRect(h, &mut rc);
            let (w, hh) = (rc.right.max(1), rc.bottom.max(1));
            let sc = GetDpiForWindow(h).max(96) as f32 / 96.0;
            let t = T0.with(|t| t.elapsed().as_secs_f32());
            PREVIEW.with(|p| {
                let mut p = p.borrow_mut();
                let g = p.get_or_insert_with(|| Gfx::new(w as u32, hh as u32));
                g.reset(0.0, 0.0, w as u32, hh as u32, Some(Col::rgb(246, 247, 250)));
                draw_demo(g, w as f32, hh as f32, t, sc, false);
                blit(hdc, g, 0, 0);
            });
            EndPaint(h, &ps);
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe fn paint_header(h: HWND) {
    let mut ps: PAINTSTRUCT = zeroed();
    let hdc = BeginPaint(h, &mut ps);
    let sc = GetDpiForWindow(h).max(96) as f32 / 96.0;
    let mut rc: RECT = zeroed();
    GetClientRect(h, &mut rc);
    let hh = (HEADER as f32 * sc) as i32;
    let head = RECT { left: 0, top: 0, right: rc.right, bottom: hh };
    let br = CreateSolidBrush(0x0022_1a1a);
    FillRect(hdc, &head, br);
    DeleteObject(br);
    // accent stripe
    let stripe = RECT { left: 0, top: hh - (3.0 * sc) as i32, right: rc.right, bottom: hh };
    let br2 = CreateSolidBrush(0x0020_1edc);
    FillRect(hdc, &stripe, br2);
    DeleteObject(br2);
    // mini ninja logo
    let lw = (64.0 * sc) as u32;
    let lh = (hh as f32 - 3.0 * sc) as u32;
    let mut g = Gfx::new(lw, lh);
    g.reset(0.0, 0.0, lw, lh, Some(Col::rgb(26, 26, 34)));
    draw_demo(&mut g, lw as f32, lh as f32, T0.with(|t| t.elapsed().as_secs_f32()), sc, true);
    blit(hdc, &g, (12.0 * sc) as i32, 0);
    // title
    let (big, small) = CC.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| (c.font_big, c.fonts[2])))).unwrap_or((std::ptr::null_mut(), std::ptr::null_mut()));
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, 0x00ff_ffff);
    let old = SelectObject(hdc, big);
    let t = wide("RamRonin");
    TextOutW(hdc, (88.0 * sc) as i32, (8.0 * sc) as i32, t.as_ptr(), (t.len() - 1) as i32);
    SelectObject(hdc, small);
    SetTextColor(hdc, 0x00b4_b4c0);
    let sub = wide(&format!("v{}  -  the stickman ninja who bonks memory hogs", system::VERSION));
    TextOutW(hdc, (90.0 * sc) as i32, (40.0 * sc) as i32, sub.as_ptr(), (sub.len() - 1) as i32);
    SelectObject(hdc, old);
    EndPaint(h, &ps);
}

unsafe extern "system" fn cc_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint_header(h);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let hdc = wp as HDC;
            SetBkColor(hdc, 0x00ff_ffff);
            SetBkMode(hdc, TRANSPARENT as i32);
            GetStockObject(WHITE_BRUSH) as LRESULT
        }
        WM_COMMAND => {
            let id = (wp & 0xffff) as u16;
            let code = ((wp >> 16) & 0xffff) as u16;
            on_command(id, code, lp as HWND);
            0
        }
        WM_HSCROLL => {
            on_scroll(lp as HWND);
            0
        }
        WM_NOTIFY => {
            let hdr = &*(lp as *const NMHDR);
            if hdr.idFrom == ID_TAB as usize && hdr.code == TCN_SELCHANGE {
                let p = SendMessageW(hdr.hwndFrom, TCM_GETCURSEL, 0, 0);
                show_page(p.max(0) as usize);
            }
            0
        }
        WM_TIMER if wp == TIMER_UI => {
            let preview = CC.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| c.preview)));
            if let Some(p) = preview {
                if IsWindowVisible(p) != 0 {
                    InvalidateRect(p, std::ptr::null(), 0);
                }
            }
            // animate the header logo
            let sc = GetDpiForWindow(h).max(96) as f32 / 96.0;
            let r = RECT { left: 0, top: 0, right: (90.0 * sc) as i32, bottom: (HEADER as f32 * sc) as i32 };
            InvalidateRect(h, &r, 0);
            refresh_lists(false);
            0
        }
        WM_CLOSE => {
            DestroyWindow(h);
            0
        }
        WM_DESTROY => {
            KillTimer(h, TIMER_UI);
            let cc = CC.with(|c| c.try_borrow_mut().ok().and_then(|mut c| c.take()));
            if let Some(cc) = cc {
                for f in cc.fonts {
                    DeleteObject(f);
                }
                DeleteObject(cc.font_big);
                let _ = cc.tab;
            }
            PREVIEW.with(|p| *p.borrow_mut() = None);
            0
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

/// Start or stop the Keep-Active self-test.
fn keep_test_toggle() {
    let Some(keep) = with_app(|a| a.keep.clone()) else { return };
    let running = keep.lock().map(|k| k.test_running).unwrap_or(false);
    if running {
        if let Ok(mut k) = keep.lock() {
            k.test_cmd = Some(false);
        }
        return;
    }
    let total = 46;
    let q = format!(
        "Keep-Active self-test (about {} minutes)\n\n\
RamRonin tries each heartbeat method in turn while you're away: F15 key only, mouse nudge only, bringing the Teams window forward, \
and finally nothing at all (to confirm Teams does go Away). It records the Windows idle timer and your Teams presence \
(from Teams' own log files) in keep-test.log, which opens when it's done.\n\n\
Before you start:\n\
  - Teams must be running and signed in, showing you as Available.\n\
  - Close the Teams app on your phone (it would keep you Available by itself).\n\
  - Then don't touch the mouse or keyboard until it's finished.\n\n\
Start the test now?",
        total
    );
    if win::message_box(hwnd(), &q, "RamRonin self-test", MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    if let Ok(mut k) = keep.lock() {
        k.test_cmd = Some(true);
        k.test_status = "Self-test starting...".into();
    };
}
