//! Memory telemetry on a background thread: system RAM via
//! GlobalMemoryStatusEx and per-app working set via Toolhelp +
//! GetProcessMemoryInfo, grouped by exe name (all chrome.exe processes count
//! as one "Chrome").

use crate::util::from_wide;
use std::collections::HashMap;
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::ProcessStatus::*;
use windows_sys::Win32::System::SystemInformation::*;
use windows_sys::Win32::System::Threading::*;

#[derive(Clone, Debug, Default)]
pub struct ProcStat {
    /// lower-case exe name, e.g. "chrome.exe"
    pub name: String,
    pub bytes: u64,
    pub count: u32,
    pub path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MemSnap {
    pub total: u64,
    pub avail: u64,
    pub load: u32,
    pub procs: Vec<ProcStat>,
    pub at: Instant,
    pub ok: bool,
}

impl Default for MemSnap {
    fn default() -> Self {
        MemSnap { total: 0, avail: 0, load: 0, procs: vec![], at: Instant::now(), ok: false }
    }
}

impl MemSnap {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.avail)
    }
    pub fn get(&self, name: &str) -> Option<&ProcStat> {
        self.procs.iter().find(|p| p.name == name)
    }
}

pub type Shared = Arc<Mutex<MemSnap>>;

pub fn sample() -> MemSnap {
    let mut snap = MemSnap { at: Instant::now(), ..Default::default() };
    unsafe {
        let mut ms: MEMORYSTATUSEX = zeroed();
        ms.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut ms) != 0 {
            snap.total = ms.ullTotalPhys;
            snap.avail = ms.ullAvailPhys;
            snap.load = ms.dwMemoryLoad;
            snap.ok = true;
        }
        let h = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if h == INVALID_HANDLE_VALUE {
            return snap;
        }
        let mut pe: PROCESSENTRY32W = zeroed();
        pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut map: HashMap<String, ProcStat> = HashMap::new();
        if Process32FirstW(h, &mut pe) != 0 {
            loop {
                let pid = pe.th32ProcessID;
                if pid > 4 {
                    let name = from_wide(&pe.szExeFile).to_lowercase();
                    let ph = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                    if !ph.is_null() {
                        let mut pmc: PROCESS_MEMORY_COUNTERS = zeroed();
                        pmc.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                        let bytes = if K32GetProcessMemoryInfo(ph, &mut pmc, pmc.cb) != 0 { pmc.WorkingSetSize as u64 } else { 0 };
                        let e = map.entry(name.clone()).or_insert_with(|| ProcStat { name: name.clone(), ..Default::default() });
                        e.bytes += bytes;
                        e.count += 1;
                        if e.path.is_none() {
                            let mut buf = [0u16; 1024];
                            let mut n = buf.len() as u32;
                            if QueryFullProcessImageNameW(ph, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) != 0 {
                                e.path = Some(from_wide(&buf[..n as usize]));
                            }
                        }
                        CloseHandle(ph);
                    } else {
                        let e = map.entry(name.clone()).or_insert_with(|| ProcStat { name: name.clone(), ..Default::default() });
                        e.count += 1;
                    }
                }
                if Process32NextW(h, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(h);
        let mut v: Vec<ProcStat> = map.into_values().collect();
        v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        snap.procs = v;
    }
    snap
}

pub fn spawn(shared: Shared, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("ramronin-memory".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let s = sample();
                if let Ok(mut g) = shared.lock() {
                    *g = s;
                }
                for _ in 0..20 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })
        .ok();
}
