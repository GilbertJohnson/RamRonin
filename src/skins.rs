//! Character skins. A skin is pure data (JSON-serialisable), so new characters
//! can be added as "character packs": drop a .json file into
//! %APPDATA%\RamRonin\packs (or use Control Center > Import character pack).

use crate::util::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Skin {
    pub id: String,
    pub name: String,
    pub author: String,
    /// Stick colour, "#rrggbb".
    pub body: String,
    /// Head fill (defaults to body colour).
    pub head_fill: Option<String>,
    /// Line thickness at 100% size.
    pub line_width: f32,
    /// "round" | "square" | "masked"
    pub head: String,
    /// "dot" | "tomoe" | "visor" | "shades" | "slit"
    pub eye: String,
    pub eye_color: Option<String>,
    /// Headband colour (ninja band). Tails flutter in the wind if `tails`.
    pub headband: Option<String>,
    pub tails: bool,
    pub scarf: Option<String>,
    pub afro: Option<String>,
    pub beard: Option<String>,
    pub cap: Option<String>,
    pub antenna: bool,
    /// Chakra/aura colour for jutsu glow.
    pub aura: Option<String>,
    /// Movement speed multiplier.
    pub speed: f32,
    /// Size multiplier.
    pub size: f32,
    /// Uses the arms-back ninja sprint.
    pub ninja_run: bool,
    /// Running style: "ninja" (arms-back sprint), "run" (normal), "panic"
    /// (arms flailing, knees high, comedy escape run). Empty: `ninja_run`
    /// decides.
    pub gait: String,
    /// Long curly hair down to the neck.
    pub curls: Option<String>,
    pub mustache: Option<String>,
    /// "thick" | "twirl"
    pub mustache_style: String,
    /// Short-sleeved shirt.
    pub shirt: Option<String>,
    /// Undershirt / vest showing at an open collar.
    pub vest: Option<String>,
    /// Lungi / dhoti, folded up above the knees.
    pub lungi: Option<String>,
    /// Border stripe on the lungi.
    pub lungi_trim: Option<String>,
    /// Gold chain.
    pub chain: Option<String>,
    pub crown: Option<String>,
    /// Favourite weapon: whip|mallet|wrench|crowbar|gloves|chicken|orb|shuriken|kunai
    pub favorite: String,
    pub phrases: Vec<String>,
}

impl Default for Skin {
    fn default() -> Self {
        Skin {
            id: "custom".into(),
            name: "Custom".into(),
            author: String::new(),
            body: "#111116".into(),
            head_fill: None,
            line_width: 4.6,
            head: "round".into(),
            eye: "dot".into(),
            eye_color: None,
            headband: None,
            tails: false,
            scarf: None,
            afro: None,
            beard: None,
            cap: None,
            antenna: false,
            aura: None,
            speed: 1.0,
            size: 1.0,
            ninja_run: false,
            gait: String::new(),
            curls: None,
            mustache: None,
            mustache_style: "thick".into(),
            shirt: None,
            vest: None,
            lungi: None,
            lungi_trim: None,
            chain: None,
            crown: None,
            favorite: String::new(),
            phrases: vec![],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Gait {
    Run,
    Ninja,
    Panic,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HeadStyle {
    Round,
    Square,
    Masked,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EyeStyle {
    Dot,
    Tomoe,
    Visor,
    Shades,
    Slit,
}

/// Resolved skin (colours parsed) used by the renderer.
#[derive(Clone, Debug)]
pub struct Look {
    pub id: String,
    pub name: String,
    pub body: Col,
    pub head_fill: Col,
    pub lw: f32,
    pub head: HeadStyle,
    pub eye: EyeStyle,
    pub eye_col: Col,
    pub band: Option<Col>,
    pub tails: bool,
    pub scarf: Option<Col>,
    pub afro: Option<Col>,
    pub beard: Option<Col>,
    pub cap: Option<Col>,
    pub antenna: bool,
    pub aura: Col,
    pub speed: f32,
    pub size: f32,
    pub ninja_run: bool,
    pub gait: Gait,
    pub curls: Option<Col>,
    pub mustache: Option<Col>,
    pub twirl: bool,
    pub shirt: Option<Col>,
    pub vest: Option<Col>,
    pub lungi: Option<Col>,
    pub lungi_trim: Option<Col>,
    pub chain: Option<Col>,
    pub crown: Option<Col>,
    pub favorite: String,
    pub phrases: Vec<String>,
}

impl Look {
    /// The running pose to use for a run/sprint picked by the behaviour code.
    pub fn sprint(&self, ninja: bool, phase: f32) -> crate::rig::Pose {
        use crate::rig::poses;
        match self.gait {
            Gait::Panic => poses::panic_run(phase),
            _ if ninja => poses::ninja_run(phase),
            _ => poses::run(phase),
        }
    }
}

impl Skin {
    pub fn look(&self) -> Look {
        let body = Col::hex(&self.body).unwrap_or(BLACK);
        let c = |o: &Option<String>| o.as_deref().and_then(Col::hex);
        Look {
            id: self.id.clone(),
            name: self.name.clone(),
            body,
            head_fill: c(&self.head_fill).unwrap_or(body),
            lw: self.line_width.clamp(1.5, 12.0),
            head: match self.head.as_str() {
                "square" => HeadStyle::Square,
                "masked" => HeadStyle::Masked,
                _ => HeadStyle::Round,
            },
            eye: match self.eye.as_str() {
                "tomoe" => EyeStyle::Tomoe,
                "visor" => EyeStyle::Visor,
                "shades" => EyeStyle::Shades,
                "slit" => EyeStyle::Slit,
                _ => EyeStyle::Dot,
            },
            eye_col: c(&self.eye_color).unwrap_or(WHITE),
            band: c(&self.headband),
            tails: self.tails,
            scarf: c(&self.scarf),
            afro: c(&self.afro),
            beard: c(&self.beard),
            cap: c(&self.cap),
            antenna: self.antenna,
            aura: c(&self.aura).unwrap_or(Col::rgb(90, 190, 255)),
            speed: self.speed.clamp(0.3, 3.0),
            size: self.size.clamp(0.4, 3.0),
            ninja_run: self.ninja_run || self.gait == "ninja",
            gait: match self.gait.as_str() {
                "panic" => Gait::Panic,
                "ninja" => Gait::Ninja,
                "run" => Gait::Run,
                _ if self.ninja_run => Gait::Ninja,
                _ => Gait::Run,
            },
            curls: c(&self.curls),
            mustache: c(&self.mustache),
            twirl: self.mustache_style == "twirl",
            shirt: c(&self.shirt),
            vest: c(&self.vest),
            lungi: c(&self.lungi),
            lungi_trim: c(&self.lungi_trim),
            chain: c(&self.chain),
            crown: c(&self.crown),
            favorite: self.favorite.clone(),
            phrases: self.phrases.clone(),
        }
    }
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}
fn phrases(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

pub fn builtin() -> Vec<Skin> {
    vec![
        Skin {
            id: "shinobi".into(),
            name: "Shinobi (red band)".into(),
            author: "RamRonin".into(),
            body: "#0e0e12".into(),
            headband: s("#dc1e1e"),
            tails: true,
            ninja_run: true,
            favorite: "orb".into(),
            speed: 1.15,
            phrases: phrases(&["Believe it... or don't.", "Dattebonk!", "My ninja way: bonking RAM.", "Shadow mode: ON"]),
            ..Default::default()
        },
        Skin {
            id: "crimson_eye".into(),
            name: "Crimson Eye".into(),
            author: "RamRonin".into(),
            body: "#18141c".into(),
            eye: "tomoe".into(),
            eye_color: s("#e01818"),
            headband: s("#3a3f4a"),
            tails: true,
            scarf: s("#2b2b6e"),
            ninja_run: true,
            favorite: "kunai".into(),
            aura: s("#8a5cff"),
            phrases: phrases(&["I see your RAM.", "Your process is already dead. (jk)", "Foolish browser tab..."]),
            ..Default::default()
        },
        Skin {
            id: "fighter".into(),
            name: "Stickman Fighter".into(),
            author: "RamRonin".into(),
            body: "#141414".into(),
            favorite: "gloves".into(),
            phrases: phrases(&["Come at me, Chrome!", "One punch. One megabyte.", "Hiyaaa!"]),
            ..Default::default()
        },
        Skin {
            id: "shadow_ninja".into(),
            name: "Shadow Ninja".into(),
            author: "RamRonin".into(),
            body: "#1a2140".into(),
            head: "masked".into(),
            eye: "slit".into(),
            scarf: s("#c62828"),
            ninja_run: true,
            favorite: "kunai".into(),
            speed: 1.2,
            phrases: phrases(&["...", "*silent bonk*", "The shadows see your tabs."]),
            ..Default::default()
        },
        Skin {
            id: "robo".into(),
            name: "Robo-Bonk 3000".into(),
            author: "RamRonin".into(),
            body: "#5b6472".into(),
            head_fill: s("#8d97a5"),
            head: "square".into(),
            eye: "visor".into(),
            eye_color: s("#29e0ff"),
            antenna: true,
            line_width: 5.2,
            favorite: "wrench".into(),
            speed: 0.9,
            phrases: phrases(&["BEEP. MEMORY. TOO. HIGH.", "Initiating bonk.exe", "01000010 01001111 01001110 01001011"]),
            ..Default::default()
        },
        Skin {
            id: "grandpa".into(),
            name: "Grandpa Rampage".into(),
            author: "RamRonin".into(),
            body: "#3b2a20".into(),
            head_fill: s("#f0c8a0"),
            beard: s("#f4f4f4"),
            cap: s("#4e6b3a"),
            favorite: "mallet".into(),
            speed: 0.8,
            phrases: phrases(&["Back in my day we had 640K!", "Kids and their 40 tabs...", "Get off my taskbar!"]),
            ..Default::default()
        },
        Skin {
            id: "disco".into(),
            name: "Neon Disco".into(),
            author: "RamRonin".into(),
            body: "#ff2bd6".into(),
            head_fill: s("#ffd27a"),
            afro: s("#3a1e0e"),
            eye: "shades".into(),
            favorite: "chicken".into(),
            speed: 1.1,
            phrases: phrases(&["Groovy RAM, baby!", "Disco bonk!", "Free memory, free spirit."]),
            ..Default::default()
        },
        // Fan tribute to the Tamil comedy legend Vadivelu and his famous
        // flailing escape run. Fan-made, not affiliated with him.
        Skin {
            id: "vadivelu_sprint".into(),
            name: "Vadivelu: Yellow Shirt Sprint".into(),
            author: "RamRonin (fan tribute)".into(),
            body: "#6e4429".into(),
            head_fill: s("#c68a5e"),
            curls: s("#17110d"),
            mustache: s("#17110d"),
            shirt: s("#f2c230"),
            vest: s("#d42a2a"),
            lungi: s("#262b40"),
            lungi_trim: s("#b9bccb"),
            chain: s("#e8c04a"),
            gait: "panic".into(),
            favorite: "chicken".into(),
            speed: 1.15,
            phrases: phrases(&[
                "Aaha... kilambitaangayaa!",
                "Vada pochey! (and so did your RAM)",
                "Mudiyala... RAM mudiyala!",
                "Edhaiyum plan panni pannanum.",
                "Ennama ipdi panreengalema?",
            ]),
            ..Default::default()
        },
        Skin {
            id: "vadivelu_lungi".into(),
            name: "Vadivelu: Lungi Don".into(),
            author: "RamRonin (fan tribute)".into(),
            body: "#6e4429".into(),
            head_fill: s("#c68a5e"),
            curls: s("#17110d"),
            mustache: s("#17110d"),
            shirt: s("#f3f1ea"),
            lungi: s("#2f7a4c"),
            lungi_trim: s("#f2c230"),
            scarf: s("#c0392b"),
            gait: "panic".into(),
            favorite: "mallet".into(),
            speed: 1.1,
            phrases: phrases(&["Naanum rowdy dhaan!", "Ennama ipdi panreengalema?", "Aaha... kilambitaangayaa!", "Mudiyala!"]),
            ..Default::default()
        },
        Skin {
            id: "vadivelu_emperor".into(),
            name: "Vadivelu: Emperor".into(),
            author: "RamRonin (fan tribute)".into(),
            body: "#6e4429".into(),
            head_fill: s("#c68a5e"),
            curls: s("#17110d"),
            mustache: s("#17110d"),
            mustache_style: "twirl".into(),
            crown: s("#e8b923"),
            shirt: s("#8e1b3a"),
            vest: s("#e8c04a"),
            lungi: s("#efe3c2"),
            lungi_trim: s("#c9a227"),
            chain: s("#e8c04a"),
            gait: "panic".into(),
            favorite: "whip".into(),
            speed: 0.95,
            phrases: phrases(&["Royal decree: close some tabs!", "The emperor demands free RAM!", "Mudiyala!", "Aaha... kilambitaangayaa!"]),
            ..Default::default()
        },
    ]
}

/// Example packs shipped inside the exe; copied into the packs folder once so
/// they show up as regular, editable packs.
pub const EXAMPLE_PACKS: &[(&str, &str)] = &[
    ("sand-ninja.json", include_str!("../packs-examples/sand-ninja.json")),
    ("retro-pack.json", include_str!("../packs-examples/retro-pack.json")),
];

pub fn seed_examples() {
    let dir = packs_dir();
    for (name, text) in EXAMPLE_PACKS {
        let p = dir.join(name);
        if !p.exists() {
            let _ = std::fs::write(p, text);
        }
    }
}

/// Built-in skins + every valid pack in the packs folder.
pub fn load_all() -> Vec<Skin> {
    let mut v = builtin();
    if let Ok(rd) = std::fs::read_dir(packs_dir()) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for p in files {
            if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("json")) != Some(true) {
                continue;
            }
            if let Some(list) = std::fs::read_to_string(&p).ok().and_then(|t| parse_pack(&t)) {
                for sk in list {
                    v.retain(|o| o.id != sk.id);
                    v.push(sk);
                }
            }
        }
    }
    v
}

/// A pack file is either one skin object or {"characters":[...]}.
pub fn parse_pack(text: &str) -> Option<Vec<Skin>> {
    #[derive(Deserialize)]
    struct Pack {
        characters: Vec<Skin>,
    }
    let list = if let Ok(p) = serde_json::from_str::<Pack>(text) {
        p.characters
    } else {
        vec![serde_json::from_str::<Skin>(text).ok()?]
    };
    let list: Vec<Skin> = list
        .into_iter()
        .filter(|s| !s.id.trim().is_empty() && Col::hex(&s.body).is_some())
        .collect();
    if list.is_empty() {
        None
    } else {
        Some(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_packs_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packs-examples");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let list = parse_pack(&text).unwrap_or_else(|| panic!("bad pack {:?}", e.path()));
            for s in &list {
                let _ = s.look();
            }
            n += list.len();
        }
        assert_eq!(n, 3);
    }

    #[test]
    fn builtins_are_valid() {
        for s in builtin() {
            assert!(Col::hex(&s.body).is_some(), "{}", s.id);
        }
        assert!(parse_pack("{\"id\":\"\",\"body\":\"#000\"}").is_none());
    }
}
