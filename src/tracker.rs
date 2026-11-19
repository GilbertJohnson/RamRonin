//! Window tracker: which app windows are on screen, where, in what z-order,
//! and which parts of their top edges are actually visible (so the ninja only
//! stands on edges you can see).

use crate::util::*;
use crate::win::{self, Monitor, RectI};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Surf {
    Floor(usize),
    Win(isize),
}

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
            last: Instant::now() - Duration::from_secs(5),
            last_mon: Instant::now(),
            names: HashMap::new(),
            names_at: Instant::now(),
            my_pid: std::process::id(),
            own: vec![],
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

        // visible top-edge segments
        let mut segs = Vec::new();
        for (zi, slot) in order.iter().enumerate() {
            let Some(wi) = slot else { continue };
            let w = &wins[*wi];
            let y = w.rect.t;
            // must have head-room on its monitor
            let Some(mon) = self.mons.iter().find(|m| {
                let cx = (w.rect.l + w.rect.r) / 2;
                cx >= m.rect.l && cx < m.rect.r && y >= m.rect.t && y < m.rect.b
            }) else {
                continue;
            };
            if y < mon.rect.t + (95 * mon.dpi / 96) as i32 || y > mon.work.b - 30 {
                continue;
            }
            let mut ivs = vec![(w.rect.l.max(mon.rect.l) as f32 + 6.0, w.rect.r.min(mon.rect.r) as f32 - 6.0)];
            for occ in &occluders[..zi] {
                if occ.w() <= 0 {
                    continue;
                }
                // covers the strip just above / on the edge?
                if occ.t <= y + 2 && occ.b > y - 6 {
                    let (a, b) = (occ.l as f32 - 4.0, occ.r as f32 + 4.0);
                    let mut next = Vec::new();
                    for (x0, x1) in ivs {
                        if b <= x0 || a >= x1 {
                            next.push((x0, x1));
                        } else {
                            if a > x0 {
                                next.push((x0, a));
                            }
                            if b < x1 {
                                next.push((b, x1));
                            }
                        }
                    }
                    ivs = next;
                }
            }
            for (x0, x1) in ivs {
                if x1 - x0 >= 70.0 {
                    segs.push(Seg { y: y as f32, x0, x1, surf: Surf::Win(w.hwnd) });
                }
            }
        }
        for (i, m) in self.mons.iter().enumerate() {
            segs.push(Seg { y: m.work.b as f32, x0: m.work.l as f32, x1: m.work.r as f32, surf: Surf::Floor(i) });
        }
        self.wins = wins;
        self.minimized = minimized;
        self.segs = segs;
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
}
