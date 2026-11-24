//! Keep-Active watchdog (background thread).
//!
//! While at least one selected app is running:
//!  * asks Windows not to sleep / blank the display (SetThreadExecutionState),
//!  * when the user has been idle for `idle_s`, sends a heartbeat every
//!    `interval_s` seconds (never while you are typing or moving the mouse).
//!    A heartbeat is any mix of: an F15 key tap (F15 exists on no normal
//!    keyboard and no app uses it); a mouse nudge (1 pixel right and back,
//!    the cursor stays put); and briefly bringing the kept app's window
//!    forward (at most every 3 minutes), for apps like the new Teams that
//!    may only count activity in their own window.
//!  * optionally relaunches a selected app if it closes.
//!
//! There is also a self-test (`KeepState::test_cmd`): it tries each method in
//! turn while you're away and writes keep-test.log with the Windows idle
//! timer and the Teams presence (read from Teams' own log files), so you can
//! see which method keeps Teams "Available" on your PC.

use crate::config::Keep;
use crate::memory;
use crate::util::data_dir;
use crate::win;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use windows_sys::Win32::System::Power::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, ShowWindow, SW_SHOWMINNOACTIVE};

#[derive(Default, Clone)]
pub struct KeepState {
    pub cfg: Keep,
    pub running: Vec<(String, bool)>,
    pub beats: u32,
    pub last_beat: Option<SystemTime>,
    pub relaunches: u32,
    pub note: String,
    /// set by the UI: Some(true) = start the self-test, Some(false) = stop it
    pub test_cmd: Option<bool>,
    pub test_running: bool,
    pub test_status: String,
}

pub type Shared = Arc<Mutex<KeepState>>;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Methods {
    key: bool,
    mouse: bool,
    wake: bool,
}

/// Bring the app's window forward at most this often.
const WAKE_EVERY: Duration = Duration::from_secs(180);

/// Send one heartbeat. Returns a short description for the test log.
fn heartbeat(m: Methods, apps: &[String], last_wake: &mut Option<Instant>) -> String {
    let mut done = vec![];
    if m.wake && last_wake.map(|t| t.elapsed() >= WAKE_EVERY).unwrap_or(true) {
        done.push(wake_window(apps));
        *last_wake = Some(Instant::now());
    }
    if m.key {
        done.push(format!("F15 key ({}/2 accepted)", win::tap_f15()));
    }
    if m.mouse {
        done.push(format!("mouse nudge ({}/3 accepted)", win::nudge_mouse()));
    }
    if done.is_empty() {
        "nothing".into()
    } else {
        done.join(", ")
    }
}

/// Bring the first kept app's main window to the front for a moment (with a
/// key tap and mouse nudge while it has focus), then give focus back to
/// whatever you had in front and re-minimize the app if it was minimized.
fn wake_window(apps: &[String]) -> String {
    let Some((app, h)) = apps.iter().find_map(|a| win::main_window_of(a).map(|h| (a.clone(), h))) else {
        return "window wake: no window found".into();
    };
    unsafe {
        let prev = GetForegroundWindow();
        if prev == h {
            win::tap_f15();
            return format!("window wake: {} already in front", app);
        }
        let was_min = windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(h) != 0;
        let got = win::force_foreground(h);
        std::thread::sleep(Duration::from_millis(400));
        win::tap_f15();
        win::nudge_mouse();
        std::thread::sleep(Duration::from_millis(1200));
        // only put things back if you didn't take over in the meantime
        if GetForegroundWindow() == h && win::idle_ms() > 500 {
            if !prev.is_null() {
                win::force_foreground(prev);
            }
            if was_min {
                ShowWindow(h, SW_SHOWMINNOACTIVE);
            }
        }
        format!("window wake: {} brought forward={}{}", app, got, if was_min { " (was minimized)" } else { "" })
    }
}

// ---------------------------------------------------------------------------
// Teams presence, read from Teams' own log files
// ---------------------------------------------------------------------------

const STATES: &[&str] = &[
    "PresenceUnknown",
    "DoNotDisturb",
    "BeRightBack",
    "Available",
    "Presenting",
    "InAMeeting",
    "Focusing",
    "OffWork",
    "InACall",
    "Offline",
    "Away",
    "Busy",
];

fn newest_file(dir: PathBuf, prefix: &str, ext: &str) -> Option<(PathBuf, SystemTime)> {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with(prefix) || !name.ends_with(ext) {
            continue;
        }
        let Ok(t) = e.metadata().and_then(|m| m.modified()) else { continue };
        if best.as_ref().map(|b| t > b.1).unwrap_or(true) {
            best = Some((e.path(), t));
        }
    }
    best
}

fn tail(path: &PathBuf, bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(bytes))).ok()?;
    let mut buf = vec![];
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// (state, short excerpt of the log line it came from, which log)
fn teams_presence() -> Option<(String, String, &'static str)> {
    let mut found: Vec<(SystemTime, String, String, &'static str)> = vec![];
    // new Teams (ms-teams.exe)
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\Microsoft\\MSTeams\\Logs");
        if let Some((p, t)) = newest_file(dir, "msteams", ".log") {
            if let Some(text) = tail(&p, 2 * 1024 * 1024) {
                for line in text.lines().rev() {
                    let low = line.to_lowercase();
                    let Some(i) = low.find("availability") else { continue };
                    let after = &line[i..];
                    let window: String = after.chars().take(80).collect();
                    let wl = window.to_lowercase();
                    if let Some(s) = STATES.iter().find(|s| wl.contains(&s.to_lowercase())) {
                        found.push((t, s.to_string(), window, "new Teams"));
                        break;
                    }
                }
            }
        }
    }
    // classic Teams (Teams.exe)
    if let Some(roam) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(roam).join("Microsoft\\Teams\\logs.txt");
        if let (Ok(t), Some(text)) = (std::fs::metadata(&p).and_then(|m| m.modified()), tail(&p, 1024 * 1024)) {
            for line in text.lines().rev() {
                let Some(i) = line.find("StatusIndicatorStateService: Added ") else { continue };
                let after = &line[i + 35..];
                let word: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                if !word.is_empty() {
                    let excerpt: String = line[i..].chars().take(80).collect();
                    found.push((t, word, excerpt, "classic Teams"));
                    break;
                }
            }
        }
    }
    found.sort_by_key(|f| f.0);
    found.pop().map(|f| (f.1, f.2, f.3))
}

/// Why the presence couldn't be read: which Teams log was found and whether
/// it mentions presence at all (counts only, no log content).
fn presence_diag() -> String {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return "no LOCALAPPDATA".into() };
    let dir = PathBuf::from(local).join("Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\Microsoft\\MSTeams\\Logs");
    if !dir.exists() {
        return "new Teams log folder not found".into();
    }
    let Some((p, _)) = newest_file(dir, "msteams", ".log") else { return "no MSTeams_*.log in the new Teams log folder".into() };
    let text = tail(&p, 2 * 1024 * 1024).unwrap_or_default().to_lowercase();
    let count = |k: &str| text.matches(k).count();
    format!(
        "newest log {}: {} KB read, lines with 'availability' {}, 'presence' {}, 'away' {}, 'available' {}",
        p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
        text.len() / 1024,
        count("availability"),
        count("presence"),
        count("away"),
        count("available")
    )
}

// ---------------------------------------------------------------------------
// self-test
// ---------------------------------------------------------------------------

struct Phase {
    name: &'static str,
    m: Methods,
    mins: u64,
}

const PHASES: &[Phase] = &[
    Phase { name: "A: F15 key only (what RamRonin 1.1 did)", m: Methods { key: true, mouse: false, wake: false }, mins: 12 },
    Phase { name: "B: mouse nudge only", m: Methods { key: false, mouse: true, wake: false }, mins: 12 },
    Phase { name: "C: bring Teams window forward + key + mouse", m: Methods { key: true, mouse: true, wake: true }, mins: 12 },
    Phase { name: "D: nothing (baseline - Teams should go Away)", m: Methods { key: false, mouse: false, wake: false }, mins: 10 },
];
/// heartbeat cadence during the test (Teams goes Away after ~5 min)
const TEST_EVERY: Duration = Duration::from_secs(30);

pub fn test_log_path() -> PathBuf {
    data_dir().join("keep-test.log")
}

fn tlog(msg: &str) {
    crate::diag::log_to("keep-test.log", msg);
}

struct Test {
    phase: usize,
    phase_start: Instant,
    /// last heartbeat slot (also in the baseline phase, where nothing is sent)
    last_inject: Option<Instant>,
    /// last time input was really sent
    sent_at: Option<Instant>,
    /// input was sent during the current tick
    sent_now: bool,
    last_wake: Option<Instant>,
    last_presence_check: Option<Instant>,
    presence: Option<String>,
    locked: bool,
    user_warned: bool,
    /// per phase: presence at start, every change (minute, state), real-input warnings
    results: Vec<(Option<String>, Vec<(u64, String)>, bool)>,
}

impl Test {
    fn start(apps: &[String]) -> Test {
        let _ = std::fs::remove_file(test_log_path());
        tlog(&format!("==== RamRonin {} Keep-Active self-test ====", crate::system::VERSION));
        tlog(&format!("Kept apps: {:?}", apps));
        let teams: Vec<&str> = ["ms-teams.exe", "teams.exe"].into_iter().filter(|e| win::main_window_of(e).is_some()).collect();
        tlog(&format!("Teams windows found for: {:?}", teams));
        let p = teams_presence();
        match &p {
            Some((s, excerpt, src)) => tlog(&format!("Teams presence now: {} (from {} log: \"{}\")", s, src, excerpt)),
            None => {
                tlog("Teams presence: could not read it from Teams' log files. Watch your status on screen instead.");
                tlog(&format!("  (presence diagnostics: {})", presence_diag()));
            }
        }
        let total: u64 = PHASES.iter().map(|p| p.mins).sum();
        tlog(&format!("Phases ({} min total), heartbeat every {}s once you're idle:", total, TEST_EVERY.as_secs()));
        for ph in PHASES {
            tlog(&format!("  {} - {} min", ph.name, ph.mins));
        }
        tlog("Please don't touch the mouse or keyboard until it's done.");
        let mut t = Test {
            phase: 0,
            phase_start: Instant::now(),
            last_inject: None,
            sent_at: None,
            sent_now: false,
            last_wake: None,
            last_presence_check: None,
            presence: p.map(|p| p.0),
            locked: false,
            user_warned: false,
            results: vec![],
        };
        t.begin_phase();
        t
    }

    fn begin_phase(&mut self) {
        self.phase_start = Instant::now();
        self.user_warned = false;
        self.last_wake = None;
        self.results.push((self.presence.clone(), vec![], false));
        tlog(&format!("---- Phase {} ----", PHASES[self.phase].name));
    }

    fn status(&self) -> String {
        let ph = &PHASES[self.phase];
        let left = (ph.mins * 60).saturating_sub(self.phase_start.elapsed().as_secs());
        format!(
            "Self-test running: phase {} of {} ({}), {}:{:02} left in this phase. Teams presence: {}. Don't touch the mouse or keyboard.",
            self.phase + 1,
            PHASES.len(),
            ph.name,
            left / 60,
            left % 60,
            self.presence.as_deref().unwrap_or("unknown")
        )
    }

    /// One tick (every ~2 s). Returns false when the test is over.
    fn tick(&mut self, apps: &[String]) -> bool {
        let mins = self.phase_start.elapsed().as_secs() / 60;
        // presence changes
        if self.last_presence_check.map(|t| t.elapsed() >= Duration::from_secs(10)).unwrap_or(true) {
            self.last_presence_check = Some(Instant::now());
            if let Some((s, _, _)) = teams_presence() {
                if self.presence.as_deref() != Some(s.as_str()) {
                    tlog(&format!("Teams presence changed: {} -> {}", self.presence.as_deref().unwrap_or("unknown"), s));
                    if let Some(r) = self.results.last_mut() {
                        r.1.push((mins, s.clone()));
                    }
                    self.presence = Some(s);
                }
            }
        }
        // lock screen
        let locked = win::session_locked();
        if locked != self.locked {
            self.locked = locked;
            tlog(if locked { "PC is LOCKED (lock screen) - input can't reach apps now" } else { "PC unlocked" });
        }
        // real input from you (idle timer reset that wasn't us)
        let idle = win::idle_ms();
        self.sent_now = false;
        let since_inject = self.sent_at.map(|t| t.elapsed().as_millis() as u32).unwrap_or(u32::MAX);
        if idle < 2500 && since_inject > 5000 && !self.user_warned {
            self.user_warned = true;
            if let Some(r) = self.results.last_mut() {
                r.2 = true;
            }
            tlog("Real mouse/keyboard input detected - Teams may count that, so this phase is less reliable.");
        }
        // heartbeat
        let ph = &PHASES[self.phase];
        if idle >= 25_000 && self.last_inject.map(|t| t.elapsed() >= TEST_EVERY).unwrap_or(true) {
            if ph.m == (Methods { key: false, mouse: false, wake: false }) {
                self.last_inject = Some(Instant::now());
                tlog(&format!("idle {:.0}s, sending nothing (baseline)", idle as f32 / 1000.0));
            } else {
                let did = heartbeat(ph.m, apps, &mut self.last_wake);
                self.last_inject = Some(Instant::now());
                self.sent_at = self.last_inject;
                self.sent_now = true;
                std::thread::sleep(Duration::from_millis(150));
                let after = win::idle_ms();
                tlog(&format!(
                    "idle {:.0}s -> sent {} -> Windows idle timer now {:.1}s ({})",
                    idle as f32 / 1000.0,
                    did,
                    after as f32 / 1000.0,
                    if after < 2000 { "reset OK" } else { "NOT reset" }
                ));
            }
        }
        // next phase?
        if self.phase_start.elapsed() >= Duration::from_secs(ph.mins * 60) {
            self.phase += 1;
            if self.phase >= PHASES.len() {
                self.finish(false);
                return false;
            }
            self.begin_phase();
        }
        true
    }

    fn finish(&mut self, cancelled: bool) {
        tlog(if cancelled { "==== Test stopped early ====" } else { "==== Test finished ====" });
        tlog("Summary (Teams presence per phase, from Teams' own log):");
        for (i, (start, changes, touched)) in self.results.iter().enumerate() {
            let ch = if changes.is_empty() {
                "no change".to_string()
            } else {
                changes.iter().map(|(m, s)| format!("{} after {} min", s, m)).collect::<Vec<_>>().join(", ")
            };
            tlog(&format!(
                "  {}: started {}, {}{}",
                PHASES[i].name,
                start.as_deref().unwrap_or("unknown"),
                ch,
                if *touched { "  [you touched the PC]" } else { "" }
            ));
        }
        tlog("A method works if Teams stayed (or became) Available during its phase. If the baseline never went Away, Teams' timeout may be longer than 10 min or something else kept it active.");
    }
}

pub fn spawn(shared: Shared, mem: memory::Shared, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("ramronin-keepactive".into())
        .spawn(move || {
            let mut awake_set = false;
            // None = no tap yet. (Not `Instant::now() - 1h`: Instant counts
            // from boot on Windows, so that panics in the first hour of uptime.)
            let mut last_beat: Option<Instant> = None;
            let mut last_wake: Option<Instant> = None;
            let mut test: Option<Test> = None;
            let mut paths: HashMap<String, String> = HashMap::new();
            let mut gone_since: HashMap<String, Instant> = HashMap::new();
            let mut last_relaunch: HashMap<String, Instant> = HashMap::new();
            while !stop.load(Ordering::Relaxed) {
                let (cfg, cmd) = shared.lock().map(|mut s| (s.cfg.clone(), s.test_cmd.take())).unwrap_or_default();
                let snap = mem.lock().map(|m| m.clone()).unwrap_or_default();
                let mut running = vec![];
                for app in &cfg.apps {
                    let p = snap.get(app);
                    let is = p.map(|p| p.count > 0).unwrap_or(false);
                    if let Some(path) = p.and_then(|p| p.path.clone()) {
                        paths.insert(app.clone(), path);
                    }
                    running.push((app.clone(), is));
                }
                let any = running.iter().any(|r| r.1);
                let active = cfg.enabled && snap.ok && any;
                let note: String;

                // self-test start / stop
                let mut test_done = None;
                match cmd {
                    Some(true) if test.is_none() => {
                        let mut apps = cfg.apps.clone();
                        for t in ["ms-teams.exe", "teams.exe"] {
                            if !apps.iter().any(|a| a == t) {
                                apps.push(t.into());
                            }
                        }
                        test = Some(Test::start(&apps));
                    }
                    Some(false) => {
                        if let Some(mut t) = test.take() {
                            t.finish(true);
                            test_done = Some("Self-test stopped. Results so far are in keep-test.log.".to_string());
                        }
                    }
                    _ => {}
                }

                // 1. keep the PC (and display) awake (always during the test)
                let want_awake = (active && cfg.prevent_sleep) || test.is_some();
                if want_awake != awake_set {
                    unsafe {
                        if want_awake {
                            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
                        } else {
                            SetThreadExecutionState(ES_CONTINUOUS);
                        }
                    }
                    awake_set = want_awake;
                }

                // 2. idle heartbeat (the self-test runs its own)
                let mut beat = false;
                let mut test_status = None;
                if let Some(t) = test.as_mut() {
                    let mut apps = cfg.apps.clone();
                    apps.extend(["ms-teams.exe".to_string(), "teams.exe".to_string()]);
                    let more = t.tick(&apps);
                    beat = t.sent_now;
                    if more {
                        test_status = Some(t.status());
                    } else {
                        test = None;
                        test_done = Some("Self-test finished. The results opened in Notepad (keep-test.log).".into());
                        win::shell_open(&test_log_path().to_string_lossy(), None);
                    }
                    note = "Self-test running.".into();
                } else if active {
                    let idle = win::idle_ms() / 1000;
                    if idle >= cfg.idle_s && last_beat.map(|b| b.elapsed().as_secs() >= cfg.interval_s.max(10) as u64).unwrap_or(true) {
                        let m = Methods { key: cfg.key, mouse: cfg.mouse, wake: cfg.wake };
                        let m = if !m.key && !m.mouse && !m.wake { Methods { key: true, ..m } } else { m };
                        heartbeat(m, &cfg.apps, &mut last_wake);
                        last_beat = Some(Instant::now());
                        beat = true;
                    }
                    note = if idle < cfg.idle_s {
                        "You're active - no heartbeat needed.".into()
                    } else {
                        "You're idle - heartbeat running.".into()
                    };
                } else if !cfg.enabled {
                    note = "Keep-Active is off.".into();
                } else if cfg.apps.is_empty() {
                    note = "Pick at least one app.".into();
                } else {
                    note = "None of the selected apps are running.".into();
                }

                // 3. relaunch closed apps
                let mut relaunched = 0;
                if cfg.enabled && cfg.relaunch {
                    for (app, is) in &running {
                        if *is {
                            gone_since.remove(app);
                            continue;
                        }
                        let Some(path) = paths.get(app) else { continue };
                        let since = *gone_since.entry(app.clone()).or_insert_with(Instant::now);
                        let recent = last_relaunch.get(app).map(|t| t.elapsed() < Duration::from_secs(120)).unwrap_or(false);
                        if since.elapsed() > Duration::from_secs(15) && !recent {
                            let mut ok = win::shell_open(path, None);
                            if !ok && app.contains("teams") {
                                ok = win::shell_open("msteams:", None);
                            }
                            last_relaunch.insert(app.clone(), Instant::now());
                            if ok {
                                relaunched += 1;
                            }
                        }
                    }
                }

                if let Ok(mut s) = shared.lock() {
                    s.running = running;
                    s.note = note;
                    if beat {
                        s.beats += 1;
                        s.last_beat = Some(SystemTime::now());
                    }
                    s.relaunches += relaunched;
                    s.test_running = test.is_some();
                    if let Some(st) = test_status.or(test_done) {
                        s.test_status = st;
                    }
                }
                for _ in 0..20 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            if let Some(mut t) = test.take() {
                t.finish(true);
            }
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        })
        .ok();
}
