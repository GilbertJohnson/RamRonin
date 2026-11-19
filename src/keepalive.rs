//! Keep-Active watchdog (background thread).
//!
//! While at least one selected app is running:
//!  * asks Windows not to sleep / blank the display (SetThreadExecutionState),
//!  * when the user has been idle for `idle_s`, taps F15 every `interval_s`
//!    seconds. F15 exists on no normal keyboard and no app binds it, but it
//!    resets the system idle timer that Teams/Slack/etc. use for "Away".
//!    It never fires while you are actively typing or moving the mouse.
//!  * optionally relaunches a selected app if it closes.

use crate::config::Keep;
use crate::memory;
use crate::win;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use windows_sys::Win32::System::Power::*;

#[derive(Default, Clone)]
pub struct KeepState {
    pub cfg: Keep,
    pub running: Vec<(String, bool)>,
    pub beats: u32,
    pub last_beat: Option<SystemTime>,
    pub relaunches: u32,
    pub note: String,
}

pub type Shared = Arc<Mutex<KeepState>>;

pub fn spawn(shared: Shared, mem: memory::Shared, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("ramronin-keepactive".into())
        .spawn(move || {
            let mut awake_set = false;
            let mut last_beat = Instant::now() - Duration::from_secs(3600);
            let mut paths: HashMap<String, String> = HashMap::new();
            let mut gone_since: HashMap<String, Instant> = HashMap::new();
            let mut last_relaunch: HashMap<String, Instant> = HashMap::new();
            while !stop.load(Ordering::Relaxed) {
                let cfg = shared.lock().map(|s| s.cfg.clone()).unwrap_or_default();
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

                // 1. keep the PC (and display) awake
                let want_awake = active && cfg.prevent_sleep;
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

                // 2. idle heartbeat
                let mut beat = false;
                if active {
                    let idle = win::idle_ms() / 1000;
                    if idle >= cfg.idle_s && last_beat.elapsed().as_secs() >= cfg.interval_s.max(10) as u64 {
                        win::tap_f15();
                        last_beat = Instant::now();
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
                }
                for _ in 0..20 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        })
        .ok();
}
