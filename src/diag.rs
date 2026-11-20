//! Startup diagnostics, so a failed start is never silent.
//!
//! * `log()` appends a time-stamped line to %APPDATA%\RamRonin\startup.log
//!   (one short block per launch; the file is cut back when it gets big).
//!   It records what each launch decided: started normally, handed off to
//!   an already running copy, relaunched itself, created its windows, drew
//!   its first frame...
//! * `install_crash_handlers()` turns a Rust panic or a hard crash (access
//!   violation and the like) into a line in crash.log plus a message box,
//!   instead of the process just vanishing.

use crate::util::{data_dir, wide};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::Diagnostics::Debug::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const MAX_LOG: u64 = 128 * 1024;

fn stamp() -> String {
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

fn append(file: &str, line: &str) {
    let path = data_dir().join(file);
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG).unwrap_or(false) {
        // keep the newest half
        if let Ok(old) = std::fs::read_to_string(&path) {
            let cut = old.len() / 2;
            let cut = old[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
            let _ = std::fs::write(&path, &old[cut..]);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(format!("[{}] pid {} {}\n", stamp(), std::process::id(), line).as_bytes()));
}

/// One line in another log file in the data folder.
pub fn log_to(file: &str, msg: &str) {
    append(file, msg);
}

/// One line in startup.log.
pub fn log(msg: impl AsRef<str>) {
    append("startup.log", msg.as_ref());
}

static SHOWN: AtomicBool = AtomicBool::new(false);

fn crash_box(text: &str) {
    // only one box, even if several threads go down at once
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    let body = format!(
        "RamRonin {} stopped because of an error:\n\n{}\n\nDetails were saved to:\n{}\n\nPlease attach crash.log and startup.log from that folder when you report this.",
        crate::system::VERSION,
        text,
        data_dir().display()
    );
    unsafe {
        MessageBoxW(std::ptr::null_mut(), wide(&body).as_ptr(), wide("RamRonin crashed").as_ptr(), MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST);
    }
}

unsafe extern "system" fn on_exception(info: *const EXCEPTION_POINTERS) -> i32 {
    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    let (code, addr) = if info.is_null() || (*info).ExceptionRecord.is_null() {
        (0u32, 0usize)
    } else {
        let r = &*(*info).ExceptionRecord;
        (r.ExceptionCode as u32, r.ExceptionAddress as usize)
    };
    let base = GetModuleHandleW(std::ptr::null()) as usize;
    let what = match code {
        0xC0000005 => "access violation",
        0xC00000FD => "stack overflow",
        0xC000001D => "illegal instruction",
        0xC0000094 => "integer divide by zero",
        0xC0000409 => "stack buffer overrun / fail-fast",
        _ => "unhandled exception",
    };
    let msg = format!("{} (code 0x{:08X}) at RamRonin.exe+0x{:X}", what, code, addr.wrapping_sub(base));
    append("crash.log", &format!("RamRonin {} crashed: {}", crate::system::VERSION, msg));
    log(format!("CRASH: {}", msg));
    crash_box(&msg);
    EXCEPTION_CONTINUE_SEARCH
}

pub fn install_crash_handlers() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("main").to_string();
        let msg = format!("panic in thread '{}': {}", thread, info);
        append("crash.log", &format!("RamRonin {} crashed: {}", crate::system::VERSION, msg));
        log(format!("CRASH: {}", msg));
        crash_box(&msg);
    }));
    unsafe {
        SetUnhandledExceptionFilter(Some(on_exception));
    }
}
