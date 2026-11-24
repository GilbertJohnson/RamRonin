//! Install / uninstall / update-from-file / start-with-Windows / single
//! instance. Everything is per-user (HKCU + %LOCALAPPDATA%), so no admin
//! rights and no UAC prompts are ever needed.

use crate::util::*;
use crate::win::{self, message_box};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const APP_NAME: &str = "RamRonin";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RamRonin";
pub const HOST_CLASS: &str = "RamRoninHost";

// ---------------------------------------------------------------------------
// single instance
// ---------------------------------------------------------------------------

static mut MUTEX: HANDLE = std::ptr::null_mut();

/// Returns false if another instance is already running. If `wait` is true
/// (we were just started by an update/install), wait up to 10s for the old
/// instance to exit first.
pub fn acquire_single_instance(wait: bool) -> bool {
    let name = wide("Local\\RamRonin.SingleInstance");
    let tries = if wait { 40 } else { 1 };
    for _ in 0..tries {
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS {
                MUTEX = h;
                return true;
            }
            if !h.is_null() {
                CloseHandle(h);
            }
        }
        if wait {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    false
}

pub fn release_single_instance() {
    unsafe {
        if !MUTEX.is_null() {
            CloseHandle(MUTEX);
            MUTEX = std::ptr::null_mut();
        }
    }
}

/// True if a RamRonin host window already exists anywhere in this session
/// (works even across app-container / packaged-app boundaries, unlike the
/// mutex, whose namespace can be private to a package).
pub fn other_instance_window() -> bool {
    unsafe { !FindWindowW(wide(HOST_CLASS).as_ptr(), std::ptr::null()).is_null() }
}

// ---------------------------------------------------------------------------
// packaged-parent breakout
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentPackageFullName(len: *mut u32, name: *mut u16) -> i32;
}

/// When RamRonin is started by a packaged (MSIX / Store) app, Windows runs
/// it inside that app's container: its %APPDATA% writes and HKCU registry
/// changes are silently redirected to the other app's private storage, so
/// Explorer can't open the settings folder and Install / autostart would not
/// be visible to the real system. RamRonin itself is never packaged, so if we
/// have package identity, ask the Windows shell (explorer.exe) to start us
/// again as a normal desktop app and return true (the caller should exit).
pub fn packaged() -> bool {
    const APPMODEL_ERROR_NO_PACKAGE: i32 = 15700;
    let mut n = 0u32;
    let r = unsafe { GetCurrentPackageFullName(&mut n, std::ptr::null_mut()) };
    r != APPMODEL_ERROR_NO_PACKAGE
}

/// Are our %APPDATA% writes being redirected into some packaged app's
/// private folder? (Happens to processes started inside an MSIX app's
/// container even when they have no package identity of their own.)
/// Probe: write a uniquely named file and look for it under
/// %LOCALAPPDATA%\Packages\*\LocalCache\Roaming\RamRonin.
pub fn appdata_redirected() -> bool {
    let name = format!(
        "probe-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let probe = data_dir().join(&name);
    if std::fs::write(&probe, b"x").is_err() {
        return false;
    }
    let mut found = false;
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if let Ok(rd) = std::fs::read_dir(PathBuf::from(local).join("Packages")) {
            for e in rd.flatten() {
                if e.path().join("LocalCache").join("Roaming").join("RamRonin").join(&name).exists() {
                    found = true;
                    break;
                }
            }
        }
    }
    let _ = std::fs::remove_file(&probe);
    found
}

pub fn break_out_of_package() -> bool {
    if !packaged() && !appdata_redirected() {
        return false;
    }
    // loop guard: if we already tried a moment ago, just run here
    let marker = std::env::temp_dir().join("RamRonin.breakout");
    if let Ok(m) = std::fs::metadata(&marker).and_then(|m| m.modified()) {
        if m.elapsed().map(|e| e.as_secs() < 20).unwrap_or(false) {
            let _ = std::fs::remove_file(&marker);
            return false;
        }
    }
    let _ = std::fs::write(&marker, b"1");
    let exe = win::exe_path();
    if exe.is_empty() {
        return false;
    }
    let explorer = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("explorer.exe");
    std::process::Command::new(explorer).arg(&exe).spawn().is_ok()
}

/// Ask the running instance to show the ninja and open its Control Center.
/// Returns true if it answered. False means there is no host window, or
/// the instance is hung (it didn't answer within a few seconds).
pub fn signal_existing() -> bool {
    unsafe {
        let h = FindWindowW(wide(HOST_CLASS).as_ptr(), std::ptr::null());
        if h.is_null() {
            return false;
        }
        // We were just started by the user, so we may set the foreground
        // window; pass that right on. Without this, Windows' focus-stealing
        // rule makes the Control Center open *behind* the current window,
        // and since it has no taskbar button it looks like nothing happened.
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid != 0 {
            AllowSetForegroundWindow(pid);
        }
        let mut res = 0usize;
        SendMessageTimeoutW(h, crate::WM_APP_SHOW, 0, 0, SMTO_ABORTIFHUNG, 5000, &mut res) != 0
    }
}

/// Wait up to `ms` for another instance's host window to appear (it may
/// still be starting up).
pub fn wait_for_other_window(ms: u64) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        if other_instance_window() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// Process IDs of other running RamRonin copies (any exe whose file name
/// starts with "ramronin", e.g. "RamRonin.exe" or "RamRonin (1).exe").
pub fn other_instance_pids() -> Vec<u32> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let me = std::process::id();
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                let name = from_wide(&pe.szExeFile).to_lowercase();
                if pe.th32ProcessID != me && name.starts_with("ramronin") && name.ends_with(".exe") {
                    out.push(pe.th32ProcessID);
                }
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// End the given processes (our own stuck copies). Returns how many ended.
pub fn end_processes(pids: &[u32]) -> usize {
    use windows_sys::Win32::System::Threading::*;
    let mut n = 0;
    for &pid in pids {
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
            if h.is_null() {
                continue;
            }
            if TerminateProcess(h, 1) != 0 {
                WaitForSingleObject(h, 3000);
                n += 1;
            }
            CloseHandle(h);
        }
    }
    n
}

/// Another copy is running but can't be reached (hung, or stuck before it
/// made its window). Tell the user instead of exiting silently, and offer to
/// end it. Returns true if the old copy was ended and we may start.
pub fn handle_stuck_instance() -> bool {
    let pids = other_instance_pids();
    crate::diag::log(format!("another instance is running but not responding; pids {:?}", pids));
    if pids.is_empty() {
        message_box(
            std::ptr::null_mut(),
            "Another copy of RamRonin seems to be running, but it isn't responding.\n\nOpen Task Manager, end every RamRonin task, then start RamRonin again.",
            APP_NAME,
            MB_ICONWARNING | MB_SETFOREGROUND,
        );
        return false;
    }
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
    let q = format!(
        "RamRonin is already running (process ID {}), but it isn't responding, so nothing is shown on screen.\n\nEnd it and start a fresh RamRonin?",
        list
    );
    if message_box(std::ptr::null_mut(), &q, APP_NAME, MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND) != IDYES {
        crate::diag::log("user chose to keep the stuck instance");
        return false;
    }
    let n = end_processes(&pids);
    crate::diag::log(format!("ended {} stuck instance(s)", n));
    true
}

// ---------------------------------------------------------------------------
// registry helpers
// ---------------------------------------------------------------------------

fn reg_set_str(key: &str, name: &str, val: &str) -> bool {
    let v = wide(val);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(name).as_ptr(),
            REG_SZ,
            v.as_ptr() as *const c_void,
            (v.len() * 2) as u32,
        ) == 0
    }
}

fn reg_set_dword(key: &str, name: &str, val: u32) -> bool {
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr(), REG_DWORD, &val as *const u32 as *const c_void, 4) == 0
    }
}

fn reg_get_str(key: &str, name: &str) -> Option<String> {
    unsafe {
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
        );
        if r == 0 {
            Some(from_wide(&buf))
        } else {
            None
        }
    }
}

fn reg_del_value(key: &str, name: &str) {
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr());
    }
}

// ---------------------------------------------------------------------------
// autostart
// ---------------------------------------------------------------------------

/// Flag passed by the Run-key entry, so a launch at sign-in stays quiet
/// (no "RamRonin is running" notification).
pub const AUTOSTART_ARG: &str = "--autostart";

fn run_value(exe: &str) -> String {
    format!("\"{}\" {}", exe, AUTOSTART_ARG)
}

pub fn set_autostart(on: bool) {
    if on {
        reg_set_str(RUN_KEY, APP_NAME, &run_value(&win::exe_path()));
    } else {
        reg_del_value(RUN_KEY, APP_NAME);
    }
}

pub fn autostart_enabled() -> bool {
    reg_get_str(RUN_KEY, APP_NAME).is_some()
}

/// Bring an older Run-key entry (without the --autostart flag) up to date.
pub fn refresh_autostart_entry() {
    if let Some(v) = reg_get_str(RUN_KEY, APP_NAME) {
        if !v.contains(AUTOSTART_ARG) {
            let exe = v.trim().trim_matches('"').to_string();
            reg_set_str(RUN_KEY, APP_NAME, &run_value(&exe));
        }
    }
}

// ---------------------------------------------------------------------------
// install / uninstall
// ---------------------------------------------------------------------------

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Programs").join(APP_NAME)
}

pub fn installed_exe() -> PathBuf {
    install_dir().join("RamRonin.exe")
}

fn start_menu_lnk() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Microsoft\\Windows\\Start Menu\\Programs\\RamRonin.lnk")
}

pub fn is_installed() -> bool {
    installed_exe().exists()
}

pub fn running_from_install() -> bool {
    let me = PathBuf::from(win::exe_path());
    let inst = installed_exe();
    me.to_string_lossy().eq_ignore_ascii_case(&inst.to_string_lossy())
}

fn copy_file(from: &Path, to: &Path) -> bool {
    unsafe { CopyFileW(wide(&from.to_string_lossy()).as_ptr(), wide(&to.to_string_lossy()).as_ptr(), 0) != 0 }
}

/// Create a Start Menu shortcut via the IShellLinkW COM interface.
fn create_shortcut(lnk: &Path, target: &Path, desc: &str) -> bool {
    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
    const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);
    type Fn1 = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
    type FnQi = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    type FnSave = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    type FnRel = unsafe extern "system" fn(*mut c_void) -> u32;
    type FnIcon = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let mut link: *mut c_void = std::ptr::null_mut();
        if CoCreateInstance(&CLSID_SHELL_LINK, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ISHELL_LINK_W, &mut link) != 0 || link.is_null() {
            return false;
        }
        let vt = *(link as *const *const usize);
        let call1 = |idx: usize, s: &str| {
            let f: Fn1 = std::mem::transmute(*vt.add(idx));
            let w = wide(s);
            f(link, w.as_ptr())
        };
        call1(20, &target.to_string_lossy()); // SetPath
        call1(7, desc); // SetDescription
        call1(9, &target.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()); // SetWorkingDirectory
        let icon: FnIcon = std::mem::transmute(*vt.add(17)); // SetIconLocation
        let ip = wide(&target.to_string_lossy());
        icon(link, ip.as_ptr(), 0);
        let qi: FnQi = std::mem::transmute(*vt.add(0));
        let mut pf: *mut c_void = std::ptr::null_mut();
        let mut ok = false;
        if qi(link, &IID_IPERSIST_FILE, &mut pf) == 0 && !pf.is_null() {
            let pvt = *(pf as *const *const usize);
            let save: FnSave = std::mem::transmute(*pvt.add(6));
            let lp = wide(&lnk.to_string_lossy());
            ok = save(pf, lp.as_ptr(), 1) == 0;
            let rel: FnRel = std::mem::transmute(*pvt.add(2));
            rel(pf);
        }
        let rel: FnRel = std::mem::transmute(*vt.add(2));
        rel(link);
        ok
    }
}

/// Copy this exe to %LOCALAPPDATA%\Programs\RamRonin, add a Start Menu
/// shortcut and an "Apps & features" entry. Returns the installed exe path.
pub fn install(owner: HWND) -> Option<PathBuf> {
    let dir = install_dir();
    let _ = std::fs::create_dir_all(&dir);
    let me = PathBuf::from(win::exe_path());
    let dst = installed_exe();
    if !running_from_install() {
        if dst.exists() {
            let old = dst.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&dst, &old);
        }
        if !copy_file(&me, &dst) {
            message_box(owner, "Could not copy RamRonin into its install folder.", APP_NAME, MB_ICONERROR);
            return None;
        }
    }
    create_shortcut(&start_menu_lnk(), &dst, "RamRonin - stickman ninja memory monitor");
    let d = dst.to_string_lossy().to_string();
    reg_set_str(UNINSTALL_KEY, "DisplayName", "RamRonin");
    reg_set_str(UNINSTALL_KEY, "DisplayVersion", VERSION);
    reg_set_str(UNINSTALL_KEY, "Publisher", "RamRonin Project");
    reg_set_str(UNINSTALL_KEY, "DisplayIcon", &format!("{},0", d));
    reg_set_str(UNINSTALL_KEY, "InstallLocation", &dir.to_string_lossy());
    reg_set_str(UNINSTALL_KEY, "UninstallString", &format!("\"{}\" --uninstall", d));
    reg_set_dword(UNINSTALL_KEY, "NoModify", 1);
    reg_set_dword(UNINSTALL_KEY, "NoRepair", 1);
    reg_set_dword(UNINSTALL_KEY, "EstimatedSize", (std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0) / 1024) as u32);
    if autostart_enabled() {
        reg_set_str(RUN_KEY, APP_NAME, &run_value(&d));
    }
    Some(dst)
}

/// Remove shortcut, registry entries and the installed exe. Settings are only
/// removed if the user agrees.
pub fn uninstall_flow() {
    let r = message_box(
        std::ptr::null_mut(),
        "Uninstall RamRonin?\n\nThis removes the Start Menu shortcut, autostart entry and the installed program.",
        "Uninstall RamRonin",
        MB_YESNO | MB_ICONQUESTION,
    );
    if r != IDYES {
        return;
    }
    // ask a running instance to quit
    unsafe {
        let h = FindWindowW(wide(HOST_CLASS).as_ptr(), std::ptr::null());
        if !h.is_null() {
            PostMessageW(h, crate::WM_APP_QUIT, 0, 0);
            std::thread::sleep(std::time::Duration::from_millis(1200));
        }
    }
    let _ = std::fs::remove_file(start_menu_lnk());
    reg_del_value(RUN_KEY, APP_NAME);
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr());
    }
    let dir = install_dir();
    let me = PathBuf::from(win::exe_path());
    if running_from_install() {
        // a running exe can't delete itself, but it can be moved out of the
        // way to %TEMP%, which Windows cleans up later
        let tmp = std::env::temp_dir().join(format!("RamRonin-uninstalled-{}.old", std::process::id()));
        let _ = std::fs::rename(&me, &tmp);
    }
    let _ = std::fs::remove_dir_all(&dir);
    let r = message_box(std::ptr::null_mut(), "Also delete your RamRonin settings and character packs?", "Uninstall RamRonin", MB_YESNO | MB_ICONQUESTION);
    if r == IDYES {
        let _ = std::fs::remove_dir_all(data_dir());
    }
    message_box(std::ptr::null_mut(), "RamRonin was uninstalled. Bye! *poof*", "Uninstall RamRonin", MB_ICONINFORMATION);
}

/// Delete leftovers from a previous update ("RamRonin.exe.old").
pub fn cleanup_old() {
    let me = PathBuf::from(win::exe_path());
    let old = PathBuf::from(format!("{}.old", me.to_string_lossy()));
    if old.exists() {
        let _ = std::fs::remove_file(old);
    }
}

// ---------------------------------------------------------------------------
// versions & update from file
// ---------------------------------------------------------------------------

pub fn parse_version(v: &str) -> [u32; 4] {
    let mut out = [0u32; 4];
    for (i, p) in v.trim().split('.').take(4).enumerate() {
        out[i] = p.trim().parse().unwrap_or(0);
    }
    out
}

/// (ProductName, version) read from an exe's VERSIONINFO resource.
pub fn file_version(path: &Path) -> Option<(String, [u32; 4])> {
    unsafe {
        let p = wide(&path.to_string_lossy());
        let size = GetFileVersionInfoSizeW(p.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(p.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(buf.as_ptr() as *const c_void, wide("\\").as_ptr(), &mut ptr, &mut len) == 0 || len == 0 {
            return None;
        }
        let ffi = &*(ptr as *const VS_FIXEDFILEINFO);
        let ver = [ffi.dwFileVersionMS >> 16, ffi.dwFileVersionMS & 0xffff, ffi.dwFileVersionLS >> 16, ffi.dwFileVersionLS & 0xffff];
        let mut name = String::new();
        if VerQueryValueW(buf.as_ptr() as *const c_void, wide("\\StringFileInfo\\040904b0\\ProductName").as_ptr(), &mut ptr, &mut len) != 0 && len > 0 {
            let sl = std::slice::from_raw_parts(ptr as *const u16, len as usize);
            name = from_wide(sl);
        }
        Some((name, ver))
    }
}

pub fn fmt_ver(v: [u32; 4]) -> String {
    format!("{}.{}.{}", v[0], v[1], v[2])
}

/// Swap the running exe for `new_exe`. Returns the path to relaunch.
pub fn update_from_file(owner: HWND, new_exe: &Path) -> Option<PathBuf> {
    let Some((name, ver)) = file_version(new_exe) else {
        message_box(owner, "That file has no version information - it doesn't look like a RamRonin build.", "Update", MB_ICONWARNING);
        return None;
    };
    if name != APP_NAME {
        message_box(owner, &format!("That file is \"{}\", not RamRonin. Update cancelled.", if name.is_empty() { "unknown" } else { &name }), "Update", MB_ICONWARNING);
        return None;
    }
    let cur = parse_version(VERSION);
    let q = if ver > cur {
        format!("Update RamRonin {} -> {}?", VERSION, fmt_ver(ver))
    } else if ver == cur {
        format!("That file is the same version ({}). Reinstall it anyway?", VERSION)
    } else {
        format!("That file is OLDER ({}) than the running version ({}). Downgrade?", fmt_ver(ver), VERSION)
    };
    if message_box(owner, &q, "Update RamRonin", MB_YESNO | MB_ICONQUESTION) != IDYES {
        return None;
    }
    let me = PathBuf::from(win::exe_path());
    let old = PathBuf::from(format!("{}.old", me.to_string_lossy()));
    let _ = std::fs::remove_file(&old);
    if std::fs::rename(&me, &old).is_err() {
        message_box(owner, "Could not replace the running program (is the folder read-only?).", "Update", MB_ICONERROR);
        return None;
    }
    if !copy_file(new_exe, &me) {
        let _ = std::fs::rename(&old, &me);
        message_box(owner, "Copying the new version failed. Nothing was changed.", "Update", MB_ICONERROR);
        return None;
    }
    if running_from_install() {
        reg_set_str(UNINSTALL_KEY, "DisplayVersion", &fmt_ver(ver));
    }
    Some(me)
}

/// Import a character pack (.json) into the packs folder.
pub fn import_pack(owner: HWND, file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let Some(list) = crate::skins::parse_pack(&text) else {
        message_box(owner, "That file isn't a valid RamRonin character pack.\nSee packs-examples in the project for the format.", "Import pack", MB_ICONWARNING);
        return None;
    };
    let name = file.file_name()?.to_string_lossy().to_string();
    let dst = packs_dir().join(name);
    if std::fs::write(&dst, text).is_err() {
        return None;
    }
    Some(list[0].id.clone())
}

/// Turn an animated GIF into a character pack: copy it into the packs folder
/// and write a .json next to it. Returns the new character's id.
pub fn import_gif(owner: HWND, file: &Path) -> Option<String> {
    use crate::sprite::{analyze, Bg};
    let Some((bg, w, h, frames)) = analyze(file) else {
        message_box(owner, "That file couldn't be read as an animated GIF.", "Import GIF", MB_ICONWARNING);
        return None;
    };
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "gif".into());
    let mut slug: String = stem.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    slug = slug.trim_matches('_').to_string();
    if slug.is_empty() {
        slug = "gif".into();
    }
    let dir = packs_dir();
    let mut name = slug.clone();
    let mut n = 2;
    while dir.join(format!("{}.gif", name)).exists() || dir.join(format!("{}.json", name)).exists() {
        name = format!("{}_{}", slug, n);
        n += 1;
    }
    if !copy_file(file, &dir.join(format!("{}.gif", name))) {
        message_box(owner, "Couldn't copy the GIF into the packs folder.", "Import GIF", MB_ICONERROR);
        return None;
    }
    let left = message_box(
        owner,
        &format!(
            "{} ({}x{}, {} frames)\n\nDoes the person in this GIF face or run to the LEFT?\n\nYes = left, No = right (or straight at you).\n\
You can change it later: \"gif_faces\" in {}.json in the packs folder.",
            stem, w, h, frames, name
        ),
        "Import GIF character",
        MB_YESNO | MB_ICONQUESTION,
    ) == IDYES;
    let pretty: String = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let low = stem.to_lowercase();
    let phrases: Vec<&str> = if low.contains("vadivel") {
        vec!["Aaha... kilambitaangayaa!", "Vada pochey! (and so did your RAM)", "Mudiyala... RAM mudiyala!", "Naanum rowdy dhaan!", "Ennama ipdi panreengalema?", "Edhaiyum plan panni pannanum."]
    } else {
        vec![]
    };
    let pack = serde_json::json!({
        "id": format!("gif_{}", name),
        "name": if pretty.is_empty() { "GIF character".to_string() } else { pretty },
        "body": "#111116",
        "gif": format!("{}.gif", name),
        "gif_key": if bg == Bg::AutoKey { "auto" } else { "" },
        "gif_sticker": bg == Bg::Sticker,
        "gif_faces": if left { "left" } else { "right" },
        "gif_scale": 1.25,
        "gait": "run",
        "favorite": "chicken",
        "phrases": phrases,
    });
    let text = serde_json::to_string_pretty(&pack).ok()?;
    if std::fs::write(dir.join(format!("{}.json", name)), text).is_err() {
        return None;
    }
    crate::diag::log(format!("imported GIF character {} ({:?}, {}x{}, {} frames)", name, bg, w, h, frames));
    Some(format!("gif_{}", name))
}

/// Standard "Open file" dialog. `filter` like "Programs\0*.exe\0".
pub fn open_file_dialog(owner: HWND, title: &str, filter: &str) -> Option<PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    unsafe {
        let mut buf = vec![0u16; 1024];
        let filt: Vec<u16> = filter.encode_utf16().chain([0, 0]).collect();
        let t = wide(title);
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = owner;
        ofn.lpstrFilter = filt.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = t.as_ptr();
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
        if GetOpenFileNameW(&mut ofn) != 0 {
            Some(PathBuf::from(from_wide(&buf)))
        } else {
            None
        }
    }
}

pub fn launch(path: &Path, args: &str) -> bool {
    win::shell_open(&path.to_string_lossy(), if args.is_empty() { None } else { Some(args) })
}
