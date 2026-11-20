//! Persistent settings in %APPDATA%\RamRonin\config.json.

use crate::util::data_dir;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Props {
    pub whip: bool,
    pub mallet: bool,
    pub wrench: bool,
    pub crowbar: bool,
    pub gloves: bool,
    pub chicken: bool,
    pub megaphone: bool,
    pub horse: bool,
    pub car: bool,
    pub pogo: bool,
    pub jetpack: bool,
}
impl Default for Props {
    fn default() -> Self {
        Props {
            whip: true,
            mallet: true,
            wrench: true,
            crowbar: true,
            gloves: true,
            chicken: true,
            megaphone: true,
            horse: true,
            car: true,
            pogo: true,
            jetpack: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Jutsu {
    pub orb: bool,
    pub shuriken: bool,
    pub eye: bool,
    pub kunai: bool,
    pub clones: bool,
    pub substitution: bool,
    pub fire: bool,
    pub flicker: bool,
}
impl Default for Jutsu {
    fn default() -> Self {
        Jutsu { orb: true, shuriken: true, eye: true, kunai: true, clones: true, substitution: true, fire: true, flicker: true }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Keep {
    pub enabled: bool,
    /// lower-case exe names, e.g. "ms-teams.exe"
    pub apps: Vec<String>,
    pub interval_s: u32,
    pub idle_s: u32,
    pub prevent_sleep: bool,
    pub relaunch: bool,
    pub visits: bool,
    /// heartbeat methods (any mix; at least one is used)
    pub key: bool,
    pub mouse: bool,
    /// briefly bring the app's window forward (for apps that only count
    /// activity in their own window)
    pub wake: bool,
}
impl Default for Keep {
    fn default() -> Self {
        Keep { enabled: false, apps: vec![], interval_s: 60, idle_s: 60, prevent_sleep: true, relaunch: false, visits: true, key: true, mouse: true, wake: false }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub skin: String,
    pub size_pct: u32,
    pub speed_pct: u32,
    pub chaos: u32,
    pub shinobi: bool,
    pub speed_fx: bool,
    pub clouds: bool,
    pub props: Props,
    pub jutsu: Jutsu,
    pub bonk_mb: u32,
    pub sys_alarm: u32,
    pub callout_s: u32,
    pub bonk_cooldown_s: u32,
    pub nap_below: u32,
    pub ignore: Vec<String>,
    pub keep: Keep,
    pub perch: bool,
    pub nap: bool,
    pub stunts: bool,
    pub react_moves: bool,
    pub autostart: bool,
    pub fps: u32,
    pub paused: bool,
    pub welcomed: bool,
    /// example packs already copied into the packs folder (once only, so a
    /// pack you delete stays deleted)
    pub packs_seeded: bool,
    /// climb window sides and stand on ledges inside app windows
    pub climb: bool,
    /// go rest on the taskbar after you've been away this many minutes (0 = never)
    pub rest_after_min: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            skin: "shinobi".into(),
            size_pct: 100,
            speed_pct: 100,
            chaos: 60,
            shinobi: true,
            speed_fx: true,
            clouds: true,
            props: Props::default(),
            jutsu: Jutsu::default(),
            bonk_mb: 1500,
            sys_alarm: 85,
            callout_s: 60,
            bonk_cooldown_s: 75,
            nap_below: 55,
            ignore: ["system", "registry", "memory compression", "secure system", "system idle process", "ramronin.exe"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            keep: Keep::default(),
            perch: true,
            nap: true,
            stunts: true,
            react_moves: true,
            autostart: false,
            fps: 30,
            paused: false,
            welcomed: false,
            packs_seeded: false,
            climb: true,
            rest_after_min: 3,
        }
    }
}

impl Config {
    pub fn path() -> std::path::PathBuf {
        data_dir().join("config.json")
    }
    pub fn load() -> Config {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let tmp = Self::path().with_extension("json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, Self::path());
            }
        }
    }
    pub fn ignored(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        self.ignore.iter().any(|i| {
            let i = i.trim().to_lowercase();
            !i.is_empty() && (n == i || n.trim_end_matches(".exe") == i.trim_end_matches(".exe"))
        })
    }
}
