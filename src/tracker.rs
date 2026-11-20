//! Window tracker: which app windows are on screen, where, in what z-order,
//! and which parts of their top edges are actually visible (so the ninja only
//! stands on edges you can see).

use crate::util::*;
use crate::win::{self, Monitor, RectI};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug)]
pub struct WinInfo {
    pub hwnd: isize,
    pub rect: RectI,
    pub pid: u32,
    pub name: String,
    pub title: String,
    pub maximized: bool,
}

/// Something the ninja can stand on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Surf {
    /// the taskbar edge / bottom of a monitor's work area
    Floor(usize),
    /// a window: (hwnd, dy). dy == 0 is the window's top edge (title bar /
    /// tab strip); dy > 0 is an inner ledge: the top edge of a section inside
    /// the app (toolbar, pane, browser content area...), relative to the top.
    Win(isize, i32),
}

impl Surf {
    pub fn hwnd(&self) -> Option<isize> {
        match self {
            Surf::Win(h, _) => Some(*h),
            _ => None,
        }
    }
    pub fn is_ledge(&self) -> bool {
        matches!(self, Surf::Win(_, d) if *d != 0)
    }
}

/// Inner ledge relative to its window: (dy, x0, x1).
type Ledge = (i32, i32, i32);

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub y: f32,
    pub x0: f32,
    pub x1: f32,
    pub surf: Surf,
}

pub struct Tracker {
    pub wins: Vec<WinInfo>,
    pub minimized: Vec<WinInfo>,
    pub mons: Vec<Monitor>,
    pub segs: Vec<Seg>,
    pub vs: RectI,
    /// set for one frame after a refresh
    pub fresh: bool,
    last: Instant,
    last_mon: Instant,
    names: HashMap<u32, String>,
    names_at: Instant,
    my_pid: u32,
    /// window handles we own (overlay, control center) – never targets
    pub own: Vec<isize>,
    /// look for ledges inside windows (child windows)
    pub ledges_on: bool,
    ledge_cache: HashMap<isize, (Instant, RectI, Vec<Ledge>)>,
}

const SKIP_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "XamlExplorerHostIslandWindow",
];

impl Tracker {
    pub fn new() -> Tracker {
        let mut t = Tracker {
            wins: vec![],
            minimized: vec![],
            mons: win::monitors(),
            segs: vec![],
            vs: RectI::default(),
            fresh: false,
            // refresh() below sets this; avoid `Instant::now() - 5s`, which
            // panics if Windows booted less than 5 seconds ago
            last: Instant::now(),
            last_mon: Instant::now(),
            names: HashMap::new(),
            names_at: Instant::now(),
            my_pid: std::process::id(),
            own: vec![],
            ledges_on: true,
            ledge_cache: HashMap::new(),
        };
        t.compute_vs();
        t.refresh();
        t
    }

    fn compute_vs(&mut self) {
        let mut v = self.mons[0].rect;
        for m in &self.mons {
            v.l = v.l.min(m.rect.l);
            v.t = v.t.min(m.rect.t);
            v.r = v.r.max(m.rect.r);
            v.b = v.b.max(m.rect.b);
        }
        self.vs = v;
    }

    pub fn tick(&mut self) {
        self.fresh = false;
        if self.last.elapsed() >= Duration::from_millis(160) {
            self.refresh();
        }
    }

    fn name_of(&mut self, pid: u32) -> String {
        if self.names_at.elapsed() > Duration::from_secs(120) {
            self.names.clear();
            self.names_at = Instant::now();
        }
        if let Some(n) = self.names.get(&pid) {
            return n.clone();
        }
        let n = win::process_path(pid).map(|p| win::file_name(&p).to_lowercase()).unwrap_or_default();
        self.names.insert(pid, n.clone());
        n
    }

    pub fn refresh(&mut self) {
        self.last = Instant::now();
        self.fresh = true;
        if self.last_mon.elapsed() > Duration::from_secs(3) {
            self.mons = win::monitors();
            self.compute_vs();
            self.last_mon = Instant::now();
        }
        let mut wins = Vec::new();
        let mut minimized = Vec::new();
        let mut occluders: Vec<RectI> = Vec::new(); // in z-order, parallel to `order`
        let mut order: Vec<Option<usize>> = Vec::new(); // index into wins for candidate windows
        for h in win::enum_windows() {
            unsafe {
                if IsWindowVisible(h) == 0 {
                    continue;
                }
            }
            if self.own.contains(&(h as isize)) {
                continue;
            }
            if win::is_cloaked(h) {
                continue;
            }
            let class = win::class_name(h);
            let iconic = unsafe { IsIconic(h) != 0 };
            let Some(rect) = win::window_rect(h) else { continue };
            if class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd" {
                occluders.push(rect);
                order.push(None);
                continue;
            }
            if SKIP_CLASSES.contains(&class.as_str()) {
                continue;
            }
            let ex = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32;
            let owner = unsafe { GetWindow(h, GW_OWNER) };
            let app_window = ex & WS_EX_APPWINDOW != 0;
            let tool = ex & WS_EX_TOOLWINDOW != 0;
            let candidate = (!tool && owner.is_null()) || app_window;
            if !iconic && rect.w() > 8 && rect.h() > 8 {
                // anything visible can hide an edge (menus, popups, other tool windows)
                occluders.push(rect);
            } else {
                occluders.push(RectI::default());
            }
            if !candidate {
                order.push(None);
                continue;
            }
            let title = win::window_title(h);
            if title.is_empty() {
                order.push(None);
                continue;
            }
            let pid = win::window_pid(h);
            if pid == self.my_pid {
                order.push(None);
                continue;
            }
            let name = self.name_of(pid);
            let info = WinInfo {
                hwnd: h as isize,
                rect,
                pid,
                name,
                title,
                maximized: unsafe { IsZoomed(h) != 0 },
            };
            if iconic {
                minimized.push(info);
                order.push(None);
                continue;
            }
            if rect.w() < 100 || rect.h() < 50 {
                order.push(None);
                continue;
            }
            wins.push(info);
            order.push(Some(wins.len() - 1));
        }

        // visible top-edge segments (+ inner ledges of the topmost windows)
        let mut segs = Vec::new();
        let mut ledge_budget = 10;
        let mut seen = vec![];
        for (zi, slot) in order.iter().enumerate() {
            let Some(wi) = slot else { continue };
            let w = &wins[*wi];
            seen.push(w.hwnd);
            let Some(mon) = self.mons.iter().find(|m| m.rect.contains((w.rect.l + w.rect.r) as f32 / 2.0, (w.rect.t + 10) as f32)).cloned() else {
                continue;
            };
            let y = w.rect.t;
            let above = &occluders[..zi];
            // top edge: needs head-room on its monitor
            if !w.maximized && y >= mon.rect.t + (95 * mon.dpi / 96) as i32 && y <= mon.work.b - 30 {
                for (x0, x1) in visible_spans(y, w.rect.l.max(mon.rect.l) as f32 + 6.0, w.rect.r.min(mon.rect.r) as f32 - 6.0, above) {
                    if x1 - x0 >= 70.0 {
                        segs.push(Seg { y: y as f32, x0, x1, surf: Surf::Win(w.hwnd, 0) });
                    }
                }
            }
            // inner ledges
            if self.ledges_on && ledge_budget > 0 {
                ledge_budget -= 1;
                let stale = self.ledge_cache.get(&w.hwnd).map(|c| c.0.elapsed() > Duration::from_millis(900) || c.1.w() != w.rect.w() || c.1.h() != w.rect.h()).unwrap_or(true);
                if stale {
                    let l = find_ledges(w.hwnd as HWND, w.rect);
                    self.ledge_cache.insert(w.hwnd, (Instant::now(), w.rect, l));
                }
                if let Some((_, _, ls)) = self.ledge_cache.get(&w.hwnd) {
                    for &(dy, lx0, lx1) in ls {
                        let ly = y + dy;
                        if ly < mon.rect.t + (60 * mon.dpi / 96) as i32 || ly > mon.work.b - 40 {
                            continue;
                        }
                        let x0 = ((w.rect.l + lx0).max(mon.rect.l)) as f32 + 4.0;
                        let x1 = ((w.rect.l + lx1).min(mon.rect.r)) as f32 - 4.0;
                        for (a, b) in visible_spans(ly, x0, x1, above) {
                            if b - a >= 70.0 {
                                segs.push(Seg { y: ly as f32, x0: a, x1: b, surf: Surf::Win(w.hwnd, dy) });
                            }
                        }
                    }
                }
            }
        }
        self.ledge_cache.retain(|h, _| seen.contains(h));
        for (i, m) in self.mons.iter().enumerate() {
            segs.push(Seg { y: m.work.b as f32, x0: m.work.l as f32, x1: m.work.r as f32, surf: Surf::Floor(i) });
        }
        self.wins = wins;
        self.minimized = minimized;
        self.segs = segs;
    }

    /// Where a window surface is right now: (y, left, right). None if the
    /// window vanished; for the top edge also None when maximized.
    pub fn live_surface(h: isize, dy: i32) -> Option<(f32, f32, f32, RectI)> {
        let (r, maxed) = Self::live_rect(h)?;
        if dy == 0 && maxed {
            return None;
        }
        Some(((r.t + dy) as f32, r.l as f32, r.r as f32, r))
    }

    pub fn floor_y(&self, m: usize) -> f32 {
        self.mons.get(m).map(|m| m.work.b as f32).unwrap_or(1000.0)
    }

    pub fn floor_range(&self, m: usize) -> (f32, f32) {
        self.mons.get(m).map(|m| (m.work.l as f32, m.work.r as f32)).unwrap_or((0.0, 1000.0))
    }

    pub fn primary(&self) -> usize {
        self.mons.iter().position(|m| m.primary).unwrap_or(0)
    }

    /// Monitor containing the point, else the nearest one.
    pub fn mon_at(&self, x: f32, y: f32) -> usize {
        if let Some(i) = self.mons.iter().position(|m| m.rect.contains(x, y)) {
            return i;
        }
        let mut best = (f32::MAX, 0);
        for (i, m) in self.mons.iter().enumerate() {
            let cx = x.clamp(m.rect.l as f32, m.rect.r as f32 - 1.0);
            let cy = y.clamp(m.rect.t as f32, m.rect.b as f32 - 1.0);
            let d = dist((x, y), (cx, cy));
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }

    pub fn dpi_scale(&self, x: f32, y: f32) -> f32 {
        self.mons[self.mon_at(x, y)].dpi as f32 / 96.0
    }

    /// The segment of a surface that contains x (with tolerance).
    pub fn seg_at(&self, surf: Surf, x: f32, tol: f32) -> Option<Seg> {
        self.segs.iter().copied().find(|s| s.surf == surf && x >= s.x0 - tol && x <= s.x1 + tol)
    }

    pub fn segs_of(&self, surf: Surf) -> impl Iterator<Item = &Seg> {
        self.segs.iter().filter(move |s| s.surf == surf)
    }

    /// Live window rect (queried right now, not from the cache), None if the
    /// window vanished, got minimized or hidden.
    pub fn live_rect(h: isize) -> Option<(RectI, bool)> {
        let hw = h as HWND;
        unsafe {
            if IsWindow(hw) == 0 || IsWindowVisible(hw) == 0 || IsIconic(hw) != 0 {
                return None;
            }
        }
        if win::is_cloaked(hw) {
            return None;
        }
        let r = win::window_rect(hw)?;
        Some((r, unsafe { IsZoomed(hw) != 0 }))
    }

    pub fn win(&self, h: isize) -> Option<&WinInfo> {
        self.wins.iter().find(|w| w.hwnd == h)
    }

    /// Windows that belong to a process name, biggest first.
    pub fn windows_of(&self, name: &str) -> Vec<&WinInfo> {
        let mut v: Vec<&WinInfo> = self.wins.iter().filter(|w| w.name == name).collect();
        v.sort_by_key(|w| -(w.rect.w() as i64 * w.rect.h() as i64));
        v
    }

    pub fn has_window(&self, name: &str) -> bool {
        self.wins.iter().any(|w| w.name == name) || self.minimized.iter().any(|w| w.name == name)
    }

    /// Window titles of an app (visible and minimized).
    pub fn titles_of(&self, name: &str) -> Vec<String> {
        self.wins.iter().chain(self.minimized.iter()).filter(|w| w.name == name).map(|w| w.title.clone()).collect()
    }
}

/// Parts of the horizontal line y in [x0, x1] that no window above covers.
fn visible_spans(y: i32, x0: f32, x1: f32, above: &[RectI]) -> Vec<(f32, f32)> {
    let mut ivs = vec![(x0, x1)];
    for occ in above {
        if occ.w() <= 0 {
            continue;
        }
        // covers the strip just above / on the edge?
        if occ.t <= y + 2 && occ.b > y - 6 {
            let (a, b) = (occ.l as f32 - 4.0, occ.r as f32 + 4.0);
            let mut next = Vec::new();
            for (s0, s1) in ivs {
                if b <= s0 || a >= s1 {
                    next.push((s0, s1));
                } else {
                    if a > s0 {
                        next.push((s0, a));
                    }
                    if b < s1 {
                        next.push((b, s1));
                    }
                }
            }
            ivs = next;
        }
    }
    ivs
}

/// Ledges inside a window: top edges of its visible child windows (panes,
/// toolbars, list views, a browser's page area...). Relative to `wr`.
fn find_ledges(h: HWND, wr: RectI) -> Vec<Ledge> {
    unsafe extern "system" fn cb(c: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l as *mut Vec<HWND>);
        if v.len() < 600 {
            v.push(c);
        }
        1
    }
    let mut kids: Vec<HWND> = vec![];
    unsafe {
        EnumChildWindows(h, Some(cb), &mut kids as *mut _ as LPARAM);
    }
    let min_w = 120.max(wr.w() / 8);
    let mut raw: Vec<Ledge> = vec![];
    for c in kids {
        unsafe {
            if IsWindowVisible(c) == 0 {
                continue;
            }
            let mut r: RECT = std::mem::zeroed();
            if GetWindowRect(c, &mut r) == 0 {
                continue;
            }
            let (l, t, rr, b) = (r.left.max(wr.l), r.top, r.right.min(wr.r), r.bottom.min(wr.b));
            if rr - l < min_w || b - t < 40 {
                continue;
            }
            let dy = t - wr.t;
            if dy < 26 || t > wr.b - 70 {
                continue;
            }
            raw.push((dy, l - wr.l, rr - wr.l));
        }
    }
    // merge ledges at (almost) the same height
    raw.sort();
    let mut out: Vec<Ledge> = vec![];
    for (dy, a, b) in raw {
        if let Some(last) = out.last_mut() {
            if (dy - last.0).abs() <= 3 && a <= last.2 + 10 && b >= last.1 - 10 {
                last.1 = last.1.min(a);
                last.2 = last.2.max(b);
                continue;
            }
        }
        out.push((dy, a, b));
    }
    // keep the widest few
    out.sort_by_key(|l| -(l.2 - l.1));
    out.truncate(6);
    out
}
