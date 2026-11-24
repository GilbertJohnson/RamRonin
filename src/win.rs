//! Thin safe-ish wrappers over the Win32 calls we need for window/monitor info.

use crate::util::{from_wide, wide};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RectI {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}
impl RectI {
    pub fn w(&self) -> i32 {
        self.r - self.l
    }
    pub fn h(&self) -> i32 {
        self.b - self.t
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l as f32 && x < self.r as f32 && y >= self.t as f32 && y < self.b as f32
    }
    fn from(r: RECT) -> Self {
        RectI { l: r.left, t: r.top, r: r.right, b: r.bottom }
    }
}

#[derive(Clone, Debug)]
pub struct Monitor {
    pub rect: RectI,
    pub work: RectI,
    pub dpi: u32,
    pub primary: bool,
}

/// Visual bounds of a window (without the invisible resize borders of Win10/11).
pub fn window_rect(h: HWND) -> Option<RectI> {
    unsafe {
        let mut r: RECT = zeroed();
        let hr = DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS as _,
            &mut r as *mut _ as *mut c_void,
            size_of::<RECT>() as u32,
        );
        if hr == 0 {
            return Some(RectI::from(r));
        }
        if GetWindowRect(h, &mut r) != 0 {
            return Some(RectI::from(r));
        }
        None
    }
}

pub fn is_cloaked(h: HWND) -> bool {
    unsafe {
        let mut v: u32 = 0;
        let hr = DwmGetWindowAttribute(h, DWMWA_CLOAKED as _, &mut v as *mut _ as *mut c_void, 4);
        hr == 0 && v != 0
    }
}

pub fn window_title(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

pub fn class_name(h: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 128];
        GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

pub fn window_pid(h: HWND) -> u32 {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        pid
    }
}

/// All top-level windows in z-order (topmost first).
pub fn enum_windows() -> Vec<HWND> {
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l as *mut Vec<HWND>);
        v.push(h);
        1
    }
    let mut v: Vec<HWND> = Vec::with_capacity(256);
    unsafe {
        EnumWindows(Some(cb), &mut v as *mut _ as LPARAM);
    }
    v
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn cb(m: HMONITOR, _dc: HDC, _r: *mut RECT, l: LPARAM) -> BOOL {
        let v = &mut *(l as *mut Vec<Monitor>);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) != 0 {
            let (mut dx, mut dy) = (96u32, 96u32);
            if GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
                dx = 96;
            }
            v.push(Monitor {
                rect: RectI::from(mi.rcMonitor),
                work: RectI::from(mi.rcWork),
                dpi: dx.max(96),
                primary: mi.dwFlags & 1 != 0,
            });
        }
        1
    }
    let mut v: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(cb), &mut v as *mut _ as LPARAM);
    }
    if v.is_empty() {
        v.push(Monitor {
            rect: RectI { l: 0, t: 0, r: 1920, b: 1080 },
            work: RectI { l: 0, t: 0, r: 1920, b: 1040 },
            dpi: 96,
            primary: true,
        });
    }
    v
}

/// Milliseconds since the user last touched keyboard or mouse.
pub fn idle_ms() -> u32 {
    unsafe {
        let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if GetLastInputInfo(&mut li) == 0 {
            return 0;
        }
        GetTickCount().wrapping_sub(li.dwTime)
    }
}

pub fn cursor_pos() -> (i32, i32) {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        (p.x, p.y)
    }
}

/// Full image path of a process (works for most user processes without admin).
pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(from_wide(&buf[..n as usize]))
    }
}

pub fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

pub fn message_box(owner: HWND, text: &str, title: &str, flags: u32) -> i32 {
    unsafe { MessageBoxW(owner, wide(text).as_ptr(), wide(title).as_ptr(), flags) }
}

/// Tap F15 - a real key that no application uses. It resets the system idle
/// timer without ever changing what the user types. Returns how many input
/// events Windows accepted (2 = both; 0 = blocked, e.g. on the lock screen).
pub fn tap_f15() -> u32 {
    unsafe {
        let mut inputs: [INPUT; 2] = zeroed();
        inputs[0].r#type = INPUT_KEYBOARD;
        inputs[0].Anonymous.ki = KEYBDINPUT { wVk: VK_F15, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 };
        inputs[1].r#type = INPUT_KEYBOARD;
        inputs[1].Anonymous.ki = KEYBDINPUT { wVk: VK_F15, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 };
        SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32)
    }
}

/// Nudge the mouse one pixel right and back (plus a zero-distance move).
/// Some apps only count mouse movement as activity. The cursor ends up where
/// it was. Returns how many input events Windows accepted (3 = all).
pub fn nudge_mouse() -> u32 {
    unsafe {
        let before = cursor_pos();
        let mv = |dx: i32| {
            let mut i: INPUT = zeroed();
            i.r#type = INPUT_MOUSE;
            i.Anonymous.mi = MOUSEINPUT { dx, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 };
            i
        };
        let inputs = [mv(0), mv(1), mv(-1)];
        let n = SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
        // with pointer acceleration +1/-1 may not cancel out exactly
        std::thread::sleep(std::time::Duration::from_millis(40));
        if cursor_pos() != before {
            SetCursorPos(before.0, before.1);
        }
        n
    }
}

/// Is the session on the lock screen / secure desktop? (Injected input goes
/// nowhere then, and every chat app shows you as Away anyway.)
pub fn session_locked() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::*;
    unsafe {
        let d = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if d.is_null() {
            return true;
        }
        let ok = SwitchDesktop(d) != 0;
        CloseDesktop(d);
        !ok
    }
}

/// Bring `h` to the foreground even from a background process (attaches to
/// the current foreground thread's input queue for the call).
pub fn force_foreground(h: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == h {
            return true;
        }
        let me = GetCurrentThreadId();
        let other = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
        let attached = other != 0 && other != me && AttachThreadInput(me, other, 1) != 0;
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        BringWindowToTop(h);
        SetForegroundWindow(h);
        if attached {
            AttachThreadInput(me, other, 0);
        }
        GetForegroundWindow() == h
    }
}

/// The main window of the first running process with this exe name.
pub fn main_window_of(exe: &str) -> Option<HWND> {
    let want = exe.to_lowercase();
    let mut best: Option<(HWND, i64)> = None;
    for h in enum_windows() {
        unsafe {
            if IsWindowVisible(h) == 0 && IsIconic(h) == 0 {
                continue;
            }
            if !GetWindow(h, GW_OWNER).is_null() || window_title(h).is_empty() {
                continue;
            }
        }
        let Some(path) = process_path(window_pid(h)) else { continue };
        if file_name(&path).to_lowercase() != want {
            continue;
        }
        let area = window_rect(h).map(|r| r.w() as i64 * r.h() as i64).unwrap_or(0);
        if best.map(|b| area > b.1).unwrap_or(true) {
            best = Some((h, area));
        }
    }
    best.map(|b| b.0)
}

pub fn shell_open(target: &str, args: Option<&str>) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    unsafe {
        let op = wide("open");
        let t = wide(target);
        let a = args.map(wide);
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            t.as_ptr(),
            a.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null()),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        r as isize > 32
    }
}

pub fn exe_path() -> String {
    unsafe {
        let mut buf = vec![0u16; 2048];
        let n = windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        );
        from_wide(&buf[..n as usize])
    }
}
