//! Where is an app shown on the taskbar? Uses UI Automation on the shell's
//! taskbar windows (primary + secondary monitors, including tray icons) from
//! a background thread, so the ninja can whop a minimized / hidden app's
//! taskbar button. Only Explorer's taskbar is queried (cheap, ~50 elements).

use crate::util::*;
use crate::win::RectI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;

#[derive(Clone, Debug)]
pub struct TbButton {
    pub name: String,
    pub rect: RectI,
}

pub type Shared = Arc<Mutex<Vec<TbButton>>>;

pub fn spawn(shared: Shared, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("ramronin-taskbar".into())
        .spawn(move || unsafe {
            if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                return;
            }
            let uia: Option<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
            if let Some(uia) = uia {
                while !stop.load(Ordering::Relaxed) {
                    let v = scan(&uia);
                    if let Ok(mut s) = shared.lock() {
                        *s = v;
                    }
                    for _ in 0..50 {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            CoUninitialize();
        })
        .ok();
}

unsafe fn scan(uia: &IUIAutomation) -> Vec<TbButton> {
    let mut out = vec![];
    let null = std::ptr::null_mut();
    let mut bars = vec![];
    let primary = FindWindowExW(null, null, wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
    if !primary.is_null() {
        bars.push(primary);
    }
    let sec = wide("Shell_SecondaryTrayWnd");
    let mut after = null;
    loop {
        let h = FindWindowExW(null, after, sec.as_ptr(), std::ptr::null());
        if h.is_null() || bars.len() > 8 {
            break;
        }
        bars.push(h);
        after = h;
    }
    let Ok(cond) = uia.CreateTrueCondition() else { return out };
    for h in bars {
        let Ok(el) = uia.ElementFromHandle(HWND(h)) else { continue };
        let Ok(arr) = el.FindAll(TreeScope_Descendants, &cond) else { continue };
        let n = arr.Length().unwrap_or(0).min(400);
        for i in 0..n {
            let Ok(e) = arr.GetElement(i) else { continue };
            let ct = e.CurrentControlType().map(|c| c.0).unwrap_or(0);
            // Button, MenuItem (Win11 tray), ListItem (Win11 task list)
            if ct != UIA_ButtonControlTypeId.0 && ct != UIA_MenuItemControlTypeId.0 && ct != UIA_ListItemControlTypeId.0 {
                continue;
            }
            let Ok(r) = e.CurrentBoundingRectangle() else { continue };
            if r.right - r.left < 8 || r.bottom - r.top < 8 {
                continue;
            }
            let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            if name.trim().is_empty() {
                continue;
            }
            out.push(TbButton { name, rect: RectI { l: r.left, t: r.top, r: r.right, b: r.bottom } });
        }
    }
    out
}

const GENERIC: &[&str] = &["start", "search", "task view", "show desktop", "action center", "notification", "widgets", "chat", "copilot"];

/// Find the taskbar (or tray) button of an app: matched by its friendly
/// name, exe stem, or the app-name part of one of its window titles.
pub fn find(buttons: &[TbButton], exe: &str, titles: &[String]) -> Option<RectI> {
    let mut keys: Vec<String> = vec![];
    let stem = exe.trim_end_matches(".exe").to_lowercase();
    if stem.len() >= 3 {
        keys.push(stem.replace(['-', '_'], " "));
        keys.push(stem);
    }
    let pretty = pretty_name(exe).to_lowercase();
    if pretty.len() >= 3 {
        keys.push(pretty);
    }
    for t in titles {
        let t = t.to_lowercase();
        if let Some(i) = t.rfind(" - ") {
            let app = t[i + 3..].trim().to_string();
            if app.len() >= 3 {
                keys.push(app);
            }
        }
        if t.len() >= 4 {
            keys.push(t);
        }
    }
    let mut best: Option<(usize, RectI)> = None;
    for b in buttons {
        let n = b.name.to_lowercase();
        if GENERIC.iter().any(|g| n == *g || n.starts_with(&format!("{g} "))) {
            continue;
        }
        for k in &keys {
            if n.contains(k.as_str()) {
                // prefer the longest (most specific) key
                if best.map(|b| k.len() > b.0).unwrap_or(true) {
                    best = Some((k.len(), b.rect));
                }
            }
        }
    }
    best.map(|b| b.1)
}
