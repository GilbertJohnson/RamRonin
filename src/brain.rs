//! The ninja's brain: an action queue + state machine. Each `Act` is a small
//! behaviour with its own update and pose; `decide` picks the next plan from
//! memory pressure, keep-active visits, mood (chaos) and what's on screen.

use crate::config::Config;
use crate::fx::{Fx, Kind};
use crate::gfx::{self, Gfx};
use crate::memory::{MemSnap, ProcStat};
use crate::props::{self, Item};
use crate::rig::{self, poses, FigFx, Pose, Skel};
use crate::skins::Look;
use crate::taskbar::{self, TbButton};
use crate::tracker::{Seg, Surf, Tracker};
use crate::util::*;
use crate::win::RectI;
use std::collections::{HashMap, VecDeque};
use std::f32::consts::PI;

const MB: u64 = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Gait {
    Walk,
    Sneak,
    Moonwalk,
    Run,
    Ninja,
    Horse,
    Car,
    Pogo,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LeapStyle {
    Flip,
    Hero,
    Jet,
    Flicker,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StuntK {
    Backflip,
    Cartwheel,
    Handstand,
    Headspin,
    Split,
    Scratch,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JutsuK {
    Orb,
    Shuriken,
    Kunai,
    Clones,
    Fire,
}

#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub surf: Surf,
    /// absolute x for floors; offset from window left for windows
    pub x: f32,
}

#[derive(Clone, Debug)]
pub enum Act {
    Idle { dur: f32, v: u8 },
    Walk { tx: f32, gait: Gait },
    Leap { to: Spot, dur: f32, style: LeapStyle, from: P },
    Sit { dur: f32 },
    Sleep { dur: f32 },
    Meditate,
    Whop { item: Item, dur: f32 },
    Jutsu { k: JutsuK, dur: f32, n: u32 },
    Stunt { k: StuntK, dur: f32 },
    Dance { dur: f32 },
    Shout { dur: f32 },
    Poke { dur: f32 },
    Taunt { dur: f32 },
    Say { lines: Vec<String>, dur: f32, think: bool },
    Fall,
    Stumble { dur: f32 },
    Land { dur: f32 },
    Dragged,
    /// climb up (or down) the left (side -1) / right (+1) wall of a window
    Climb { h: isize, side: f32, down: bool },
    /// fast asleep on the taskbar until the user comes back
    Rest,
    /// red-eye stare (no attack attached)
    Glare { dur: f32 },
}

/// What an attack is aimed at.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    /// a window (aim at its top edge, near the ninja)
    Win(isize),
    /// a fixed rectangle on screen (a taskbar / tray button)
    Rect(RectI),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum CloneK {
    Dance,
    Whop,
    Flips,
    Handstand,
    Pace,
    Orb,
    Kunai,
    Meditate,
    Headspin,
    Taunt,
    Sit,
}

pub struct Cloud {
    pub lines: Vec<String>,
    pub t: f32,
    pub dur: f32,
    pub think: bool,
}

/// A shadow clone: lives on its own spot for ~10 s doing its own thing.
struct CloneN {
    spot: Spot,
    home: f32,
    k: CloneK,
    item: Item,
    facing: f32,
    t: f32,
    dur: f32,
    phase: f32,
    cd: f32,
    /// resolved ground point this frame
    at: Option<P>,
}

pub struct World<'a> {
    pub tr: &'a Tracker,
    pub mem: &'a MemSnap,
    pub cfg: &'a Config,
    pub look: &'a Look,
    /// taskbar / tray buttons (name + rect)
    pub tb: &'a [TbButton],
    /// seconds since the user last touched mouse / keyboard
    pub user_idle: f32,
}

/// One clone, ready to draw.
pub struct CloneDraw {
    pub skel: Skel,
    pub item: Item,
    pub swing: f32,
    pub orb: bool,
}

/// Everything needed to draw one frame (computed before drawing so the
/// overlay can size itself).
pub struct Layout {
    pub skel: Skel,
    pub pose: Pose,
    pub item: Item,
    pub gait: Option<Gait>,
    pub ground: P,
    pub hidden: bool,
    pub cloud: Option<(f32, f32, f32, f32)>,
    pub clones: Vec<CloneDraw>,
    pub tomoe: bool,
    pub swing: f32,
}

pub struct Ninja {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub facing: f32,
    pub on: Option<Surf>,
    off: f32,
    last_rect: Option<RectI>,
    pub act: Act,
    pub act_t: f32,
    plan: VecDeque<Act>,
    phase: f32,
    pub t: f32,
    pub s: f32,
    pub ui: f32,
    vel: P,
    pub cloud: Option<Cloud>,
    pub fx: Vec<Fx>,
    clones: Vec<CloneN>,
    trail: VecDeque<(Skel, f32)>,
    eye_trail: VecDeque<(P, f32)>,
    rng: Rng,
    last_callout: f32,
    last_bonk: f32,
    bonked: HashMap<String, f32>,
    last_visit: f32,
    quip_cd: f32,
    fx_cd: f32,
    no_land: Option<(Surf, f32)>,
    press: Option<(P, f32)>,
    dragging: bool,
    grab_samples: VecDeque<(P, f32)>,
    pub hit: (f32, f32, f32, f32),
    chaos: f32,
    shinobi: bool,
    clouds_on: bool,
    /// what the current attack is aimed at
    target: Option<Target>,
    /// last roam choices (to avoid repeating the same thing)
    recent: VecDeque<u8>,
    /// earliest time for the next show-off jutsu / stunt / ride
    next_jutsu: f32,
    next_stunt: f32,
    next_ride: f32,
    car_col: Col,
}

fn sgn(v: f32) -> f32 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

impl Ninja {
    pub fn new(tr: &Tracker) -> Ninja {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
        let m = &tr.mons[tr.primary()];
        let x = (m.work.l + m.work.r) as f32 / 2.0;
        Ninja {
            x,
            y: m.work.t as f32 + 60.0,
            vx: 0.0,
            vy: 0.0,
            facing: 1.0,
            on: None,
            off: 0.0,
            last_rect: None,
            act: Act::Fall,
            act_t: 0.0,
            plan: VecDeque::new(),
            phase: 0.0,
            t: 0.0,
            s: 1.0,
            ui: 1.0,
            vel: (0.0, 0.0),
            cloud: None,
            fx: vec![],
            clones: vec![],
            trail: VecDeque::new(),
            eye_trail: VecDeque::new(),
            rng: Rng::new(seed),
            last_callout: 8.0 - 60.0,
            last_bonk: -30.0,
            bonked: HashMap::new(),
            last_visit: 0.0,
            quip_cd: 0.0,
            fx_cd: 0.0,
            no_land: None,
            press: None,
            dragging: false,
            grab_samples: VecDeque::new(),
            hit: (0.0, 0.0, 0.0, 0.0),
            chaos: 0.6,
            shinobi: true,
            clouds_on: true,
            target: None,
            recent: VecDeque::new(),
            next_jutsu: 25.0,
            next_stunt: 10.0,
            next_ride: 30.0,
            car_col: props::CAR_COLORS[0],
        }
    }

    // ------------------------------------------------------------------
    // clouds & quips
    // ------------------------------------------------------------------

    pub fn say(&mut self, lines: Vec<String>, dur: f32, think: bool) {
        if !self.clouds_on {
            return;
        }
        self.cloud = Some(Cloud { lines, t: 0.0, dur, think });
    }

    fn quip(&mut self, options: &[&str], p: f32) {
        if self.quip_cd > 0.0 || !self.rng.chance(p) {
            return;
        }
        let q = self.rng.pick(options).to_string();
        self.quip_cd = 3.0;
        self.say(vec![q], 2.0, false);
    }

    pub fn memory_lines(&mut self, w: &World) -> Vec<String> {
        let m = w.mem;
        if !m.ok {
            return vec!["Reading memory...".into()];
        }
        let mut v = vec![format!("RAM {}%  ({} / {})", m.load, fmt_bytes(m.used()), fmt_bytes(m.total))];
        if let Some(p) = m.procs.iter().find(|p| !w.cfg.ignored(&p.name)) {
            v.push(format!("Top: {} {}", pretty_name(&p.name), fmt_bytes(p.bytes)));
        }
        let flavor: &[&str] = match m.load {
            0..=49 => &["All chill. Nap time?", "Plenty of room!", "Your RAM is on vacation."],
            50..=74 => &["Looking healthy.", "Could be worse.", "Vibes: acceptable."],
            75..=89 => &["Getting crowded...", "Bonk time soon.", "I smell a memory hog."],
            _ => &["EVERYBODY PANIC", "Who opened 300 tabs?!", "RAM is ON FIRE"],
        };
        if !w.look.phrases.is_empty() && self.rng.chance(0.25) {
            v.push(self.rng.pick(&w.look.phrases).clone());
        } else {
            v.push(self.rng.pick(flavor).to_string());
        }
        v
    }

    // ------------------------------------------------------------------
    // surfaces
    // ------------------------------------------------------------------

    fn resolve(&self, spot: Spot, tr: &Tracker) -> Option<P> {
        match spot.surf {
            Surf::Floor(m) => {
                let (l, r) = tr.floor_range(m);
                Some((spot.x.clamp(l + 10.0, r - 10.0), tr.floor_y(m)))
            }
            Surf::Win(h, dy) => {
                let (y, l, r, _) = Tracker::live_surface(h, dy)?;
                Some(((l + spot.x).clamp(l + 8.0, r - 8.0), y))
            }
        }
    }

    fn set_on(&mut self, surf: Surf, p: P) -> bool {
        match surf {
            Surf::Win(h, dy) => {
                let Some((y, l, r, rect)) = Tracker::live_surface(h, dy) else { return false };
                self.last_rect = Some(rect);
                self.x = p.0.clamp(l + 8.0, r - 8.0);
                self.y = y;
                self.off = self.x - l;
            }
            Surf::Floor(_) => {
                self.x = p.0;
                self.y = p.1;
                self.last_rect = None;
            }
        }
        self.on = Some(surf);
        self.vx = 0.0;
        self.vy = 0.0;
        true
    }

    fn fall_off(&mut self, quip: Option<&[&str]>) {
        if let Some(s @ Surf::Win(..)) = self.on {
            self.no_land = Some((s, self.t + 0.35));
        }
        self.on = None;
        self.plan.clear();
        self.act = Act::Fall;
        self.act_t = 0.0;
        self.vy = -160.0 * self.ui;
        self.vx = self.rng.range(-60.0, 60.0) * self.ui;
        if let Some(q) = quip {
            self.quip(q, 0.8);
        }
    }

    fn busy_with_motion(&self) -> bool {
        matches!(self.act, Act::Fall | Act::Dragged | Act::Leap { .. } | Act::Climb { .. })
    }

    fn follow_surface(&mut self, dt: f32, w: &World) {
        let tr = w.tr;
        match self.on {
            Some(Surf::Win(h, dy)) => match Tracker::live_rect(h) {
                None => self.fall_off(Some(&["Where'd it go?!", "WHOA-", "The floor left me!"])),
                Some((r, maxed)) => {
                    let sy = (r.t + dy) as f32;
                    let mon = &tr.mons[tr.mon_at((r.l + r.r) as f32 / 2.0, sy + 5.0)];
                    if (dy == 0 && (maxed || r.t < mon.rect.t + 30)) || sy > mon.work.b as f32 - 20.0 {
                        self.fall_off(Some(&["WHOA!", "Maximized?! RUDE.", "AAAAH"]));
                        return;
                    }
                    if let Some(last) = self.last_rect {
                        if last != r {
                            let (dx, dy) = ((r.l - last.l) as f32, (r.t - last.t) as f32);
                            let sp = (dx * dx + dy * dy).sqrt() / dt.max(0.001);
                            if dy.abs() > 260.0 {
                                self.fall_off(Some(&["WHOA!"]));
                                return;
                            }
                            if w.cfg.react_moves
                                && sp > 1300.0 * self.ui
                                && !matches!(self.act, Act::Stumble { .. } | Act::Whop { .. } | Act::Jutsu { .. })
                            {
                                if matches!(self.act, Act::Sleep { .. } | Act::Sit { .. } | Act::Idle { .. } | Act::Walk { .. }) {
                                    self.plan.clear();
                                }
                                self.act = Act::Stumble { dur: 0.7 };
                                self.act_t = 0.0;
                                self.quip(&["WHOA!", "Easy with the mouse!", "Earthquake!", "Wheee!", "I'm gonna hurl"], 0.5);
                            }
                        }
                    }
                    let width = r.w() as f32;
                    if width < 60.0 {
                        self.fall_off(Some(&["Too narrow!"]));
                        return;
                    }
                    let m = 10.0 * self.ui;
                    if self.off > width - m {
                        self.off = width - m;
                        if !matches!(self.act, Act::Stumble { .. }) && self.rng.chance(0.03) {
                            self.quip(&["Hey! I'm standing here!", "Stop pushing!"], 1.0);
                        }
                    }
                    if self.off < m {
                        self.off = m;
                    }
                    self.x = r.l as f32 + self.off;
                    self.y = sy;
                    self.last_rect = Some(r);
                    // someone put a window on top of my spot (or the pane I
                    // stand on went away)?
                    if tr.fresh && tr.win(h).is_some() && tr.seg_at(Surf::Win(h, dy), self.x, 16.0 * self.ui).is_none() {
                        self.fall_off(Some(&["Rude!", "HEY, I was there!", "Squished!"]));
                    }
                }
            },
            Some(Surf::Floor(m)) => {
                let m = if m < tr.mons.len() { m } else { tr.mon_at(self.x, self.y) };
                self.on = Some(Surf::Floor(m));
                self.y = tr.floor_y(m);
                let (l, r) = tr.floor_range(m);
                self.x = self.x.clamp(l + 8.0, r - 8.0);
            }
            None => {}
        }
    }

    fn walk_range(&self, tr: &Tracker) -> (f32, f32) {
        let m = 12.0 * self.s;
        match self.on {
            Some(Surf::Floor(i)) => {
                let (l, r) = tr.floor_range(i);
                (l + m, r - m)
            }
            Some(s @ Surf::Win(..)) => {
                if let Some(seg) = tr.seg_at(s, self.x, 20.0) {
                    (seg.x0 + m, seg.x1 - m)
                } else if let Some(r) = self.last_rect {
                    (r.l as f32 + m, r.r as f32 - m)
                } else {
                    (self.x, self.x)
                }
            }
            None => (self.x, self.x),
        }
    }

    // ------------------------------------------------------------------
    // main update
    // ------------------------------------------------------------------

    pub fn update(&mut self, dt: f32, w: &World) {
        let dt = dt.clamp(0.0, 0.1);
        self.t += dt;
        self.quip_cd -= dt;
        self.fx_cd -= dt;
        self.chaos = w.cfg.chaos as f32 / 100.0;
        self.shinobi = w.cfg.shinobi;
        self.clouds_on = w.cfg.clouds;
        self.ui = w.tr.dpi_scale(self.x, self.y - 20.0);
        self.s = w.look.size * w.cfg.size_pct as f32 / 100.0 * self.ui;
        let (px, py) = (self.x, self.y);

        if !matches!(self.act, Act::Dragged) {
            self.follow_surface(dt, w);
        }
        if w.cfg.paused {
            if !matches!(self.act, Act::Fall | Act::Dragged | Act::Meditate | Act::Land { .. } | Act::Climb { .. } | Act::Leap { .. }) {
                self.plan.clear();
                self.begin(Act::Meditate, w);
            }
        } else if matches!(self.act, Act::Meditate) {
            self.next(w);
        }

        self.act_t += dt;
        if self.step(dt, w) {
            self.next(w);
        }

        // visual velocity (drives ribbons, speed lines, trails)
        if dt > 0.0 && !matches!(self.act, Act::Leap { style: LeapStyle::Flicker, .. }) {
            let iv = ((self.x - px) / dt, (self.y - py) / dt);
            let iv = (iv.0.clamp(-3000.0, 3000.0), iv.1.clamp(-3000.0, 3000.0));
            self.vel = lerp_p(self.vel, iv, 0.25);
        }

        // effects
        let mut spawned = vec![];
        for f in self.fx.iter_mut() {
            spawned.extend(f.update(dt));
        }
        self.fx.retain(|f| !f.done);
        self.fx.extend(spawned);
        if self.fx.len() > 160 {
            let n = self.fx.len() - 160;
            self.fx.drain(0..n);
        }
        if let Some(c) = &mut self.cloud {
            c.t += dt;
            if c.t >= c.dur {
                self.cloud = None;
            }
        }
        self.update_clones(dt, w);
        for tr in self.trail.iter_mut() {
            tr.1 += dt;
        }
        self.trail.retain(|t| t.1 < 0.14);
        for e in self.eye_trail.iter_mut() {
            e.1 += dt;
        }
        self.eye_trail.retain(|e| e.1 < 0.18);
    }

    // ------------------------------------------------------------------
    // shadow clones: each one does its own thing for 10 s
    // ------------------------------------------------------------------

    fn spawn_clones(&mut self, w: &World) {
        let Some(here) = self.spot_here() else { return };
        let (lo, hi) = self.walk_range(w.tr);
        let n = 2 + (self.chaos > 0.45) as usize + (self.chaos > 0.8) as usize;
        let mut kinds = vec![
            CloneK::Dance,
            CloneK::Whop,
            CloneK::Flips,
            CloneK::Handstand,
            CloneK::Pace,
            CloneK::Meditate,
            CloneK::Headspin,
            CloneK::Taunt,
            CloneK::Sit,
        ];
        if self.shinobi {
            kinds.push(CloneK::Orb);
            kinds.push(CloneK::Kunai);
        }
        for i in (1..kinds.len()).rev() {
            let j = self.rng.idx(i + 1);
            kinds.swap(i, j);
        }
        let s = self.s;
        let base = self.x - here.x; // window left (0 for floors)
        for i in 0..n {
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let ax = (self.x + side * (55.0 + 55.0 * (i / 2) as f32 + self.rng.range(0.0, 25.0)) * s).clamp(lo, hi.max(lo));
            let k = kinds[i % kinds.len()];
            let item = match k {
                CloneK::Whop => self.pick_weapon(w),
                CloneK::Kunai => Item::Kunai,
                _ => Item::None,
            };
            let x = ax - base;
            let facing = if k == CloneK::Whop || k == CloneK::Kunai { -side } else { self.rng.sign() };
            self.clones.push(CloneN {
                spot: Spot { surf: here.surf, x },
                home: x,
                k,
                item,
                facing,
                t: 0.0,
                dur: 10.0,
                phase: self.rng.range(0.0, 6.0),
                cd: self.rng.range(0.2, 1.0),
                at: None,
            });
            self.fx.push(Fx::new(Kind::Poof, (ax, self.y - 25.0 * s), 0.5, s));
        }
    }

    fn update_clones(&mut self, dt: f32, w: &World) {
        if self.clones.is_empty() {
            return;
        }
        let s = self.s;
        let mut fx = vec![];
        let mut clones = std::mem::take(&mut self.clones);
        for c in clones.iter_mut() {
            let t0 = c.t;
            c.t += dt;
            c.cd -= dt;
            if c.k == CloneK::Pace {
                let off = (c.t * 1.2).sin() * 70.0 * s;
                c.facing = sgn((c.t * 1.2).cos());
                c.spot.x = c.home + off;
                c.phase += dt * 8.0;
            }
            c.at = self.resolve(c.spot, w.tr);
            let Some(p) = c.at else {
                c.t = c.dur;
                continue;
            };
            match c.k {
                CloneK::Whop => {
                    // same swing timing as the pose: hit at 55% of each 0.6 s swing
                    let (a, b) = ((t0 % 0.6) / 0.6, (c.t % 0.6) / 0.6);
                    if (a < 0.55 && b >= 0.55) || (b < a && b >= 0.55) {
                        let impact = (p.0 + c.facing * 30.0 * s, p.1 - 30.0 * s);
                        let word = self.rng.pick(c.item.hit_word()).to_string();
                        fx.push(Fx::new(Kind::Burst { word, col: Col::rgb(255, 225, 60) }, impact, 0.7, s * 0.8).rot(self.rng.range(-0.3, 0.3)));
                    }
                }
                CloneK::Kunai => {
                    let (a, b) = ((t0 % 0.9) / 0.9, (c.t % 0.9) / 0.9);
                    if b < a {
                        let from = (p.0 + c.facing * 14.0 * s, p.1 - 45.0 * s);
                        let to = (p.0 + c.facing * self.rng.range(60.0, 120.0) * s, p.1 + 3.0 * s);
                        fx.push(Fx::new(Kind::Kunai { to, from, stick: 0.0 }, from, 1.6, s));
                    }
                }
                CloneK::Flips | CloneK::Taunt if c.cd <= 0.0 => {
                    c.cd = 2.0;
                    if self.rng.chance(0.5) {
                        c.facing = -c.facing;
                    }
                }
                _ => {}
            }
            if c.t >= c.dur {
                fx.push(Fx::new(Kind::Poof, (p.0, p.1 - 25.0 * s), 0.45, s));
            }
        }
        clones.retain(|c| c.t < c.dur);
        self.clones = clones;
        self.fx.extend(fx);
    }

    fn clone_pose(&self, c: &CloneN) -> (Pose, Item, f32, bool) {
        let t = c.t + c.phase;
        match c.k {
            CloneK::Dance => (poses::dance(t, self.chaos), Item::None, 0.0, false),
            CloneK::Whop => {
                let sw = (c.t % 0.6) / 0.6;
                let p = if c.item == Item::Gloves { poses::punches(sw) } else { poses::swing(sw) };
                (p, c.item, sw, false)
            }
            CloneK::Flips => {
                let u = (c.t % 2.0) / 2.0;
                if u < 0.45 {
                    let k = u / 0.45;
                    let mut p = poses::tuck(-360.0 * ease_io(k));
                    p.dy = -(k * PI).sin() * 42.0;
                    (p, Item::None, 0.0, false)
                } else {
                    (poses::taunt(t), Item::None, 0.0, false)
                }
            }
            CloneK::Handstand => (poses::handstand(t), Item::None, 0.0, false),
            CloneK::Pace => (poses::walk(c.phase), Item::None, 0.0, false),
            CloneK::Orb => (poses::charge(t), Item::None, 0.0, true),
            CloneK::Kunai => {
                let u = (c.t % 0.9) / 0.9;
                if u < 0.4 {
                    (poses::throw(u / 0.4), Item::Kunai, 0.0, false)
                } else {
                    (poses::stance(t), Item::Kunai, 0.0, false)
                }
            }
            CloneK::Meditate => (poses::meditate(t), Item::None, 0.0, false),
            CloneK::Headspin => (poses::headspin(t), Item::None, 0.0, false),
            CloneK::Taunt => (poses::taunt(t), Item::None, 0.0, false),
            CloneK::Sit => (poses::sit(t), Item::None, 0.0, false),
        }
    }

    pub fn clone_count(&self) -> usize {
        self.clones.len()
    }

    fn next(&mut self, w: &World) {
        let a = match self.plan.pop_front() {
            Some(a) => a,
            None => {
                let mut v = self.decide(w);
                if v.is_empty() {
                    v.push(Act::Idle { dur: 1.0, v: 1 });
                }
                let first = v.remove(0);
                self.plan.extend(v);
                first
            }
        };
        self.begin(a, w);
    }

    fn begin(&mut self, mut a: Act, w: &World) {
        self.act_t = 0.0;
        match &mut a {
            Act::Leap { from, style, .. } => {
                *from = (self.x, self.y);
                if let Some(s @ Surf::Win(..)) = self.on {
                    self.no_land = Some((s, self.t + 0.3));
                }
                self.on = None;
                if *style == LeapStyle::Flicker {
                    self.fx.push(Fx::new(Kind::Poof, (self.x, self.y - 25.0 * self.s), 0.45, self.s));
                }
            }
            Act::Walk { gait: g @ (Gait::Horse | Gait::Car), .. } => {
                if *g == Gait::Car {
                    self.car_col = *self.rng.pick(props::CAR_COLORS);
                }
                self.fx.push(Fx::new(Kind::Poof, (self.x, self.y - 20.0 * self.s), 0.4, self.s * 1.3));
            }
            Act::Say { lines, dur, think } => {
                let (l, d, t) = (lines.clone(), *dur, *think);
                self.say(l, d, t);
            }
            Act::Shout { .. } => {
                let lines = self.memory_lines(w);
                self.say(lines, 5.0, false);
            }
            Act::Whop { .. } | Act::Jutsu { .. } => {
                if let Some(p) = self.target_point() {
                    if (p.0 - self.x).abs() > 6.0 {
                        self.facing = sgn(p.0 - self.x);
                    }
                }
            }
            Act::Glare { .. } => {
                let hp = (self.x, self.y - (rig::HEIGHT + 22.0) * self.s);
                self.fx.push(Fx::new(Kind::EyeFlash, hp, 1.1, self.s));
            }
            Act::Climb { side, .. } => {
                self.facing = -*side;
                self.on = None;
            }
            Act::Rest => {
                let line = self.rng.pick(&["Nobody's home... nap time.", "You left? Taskbar nap!", "Wake me when you're back."]).to_string();
                self.say(vec![line], 3.0, true);
            }
            Act::Sleep { .. } => {
                if self.rng.chance(0.5) {
                    let pct = w.mem.load;
                    self.say(vec![format!("Zzz... RAM only {}%", pct)], 3.0, true);
                }
            }
            _ => {}
        }
        self.act = a;
    }

    /// Advance the current act. Returns true when it is finished.
    fn step(&mut self, dt: f32, w: &World) -> bool {
        let tr = w.tr;
        let speed_mult = w.cfg.speed_pct as f32 / 100.0 * w.look.speed;
        let at = self.act_t;
        match self.act.clone() {
            Act::Idle { dur, .. }
            | Act::Sit { dur }
            | Act::Dance { dur }
            | Act::Shout { dur }
            | Act::Poke { dur }
            | Act::Taunt { dur }
            | Act::Stumble { dur }
            | Act::Glare { dur }
            | Act::Land { dur } => at >= dur,
            Act::Say { .. } => true,
            Act::Meditate => false,
            Act::Rest => {
                if self.fx_cd <= 0.0 {
                    self.fx_cd = 1.4;
                    let hp = (self.x + self.facing * -6.0 * self.s, self.y - 20.0 * self.s);
                    self.fx.push(Fx::new(Kind::Zzz, hp, 2.4, self.s));
                }
                // wake up when the user is back (or RAM is on fire)
                if w.user_idle < 1.0 || (w.mem.ok && w.mem.load >= w.cfg.sys_alarm) {
                    self.plan.clear();
                    self.plan.push_back(Act::Say { lines: vec![self.rng.pick(&["Huh?! I'm up! I'm up!", "Welcome back!", "I wasn't sleeping. I was meditating."]).to_string()], dur: 2.2, think: false });
                    self.plan.push_back(Act::Stunt { k: StuntK::Scratch, dur: 1.2 });
                    return true;
                }
                false
            }
            Act::Climb { h, side, down } => {
                let Some((r, maxed)) = Tracker::live_rect(h) else {
                    self.fall_off(Some(&["Where'd the wall go?!"]));
                    return false;
                };
                if maxed && !down {
                    self.fall_off(Some(&["Maximized?! RUDE."]));
                    return false;
                }
                let wall = if side < 0.0 { r.l as f32 } else { r.r as f32 };
                self.x = wall + side * 7.0 * self.s;
                self.facing = -side;
                let sp = (if self.shinobi || w.look.ninja_run { 210.0 } else { 140.0 }) * speed_mult * self.ui;
                self.phase += dt * sp / (16.0 * self.s);
                if down {
                    self.y += sp * 1.2 * dt;
                    let m = tr.mon_at(self.x, self.y);
                    let fy = tr.floor_y(m);
                    if self.y >= fy {
                        self.set_on(Surf::Floor(m), (self.x, fy));
                        self.act = Act::Land { dur: 0.2 };
                        self.act_t = 0.0;
                    } else if self.y >= r.b as f32 - 4.0 {
                        // end of the wall: let go
                        self.act = Act::Fall;
                        self.act_t = 0.0;
                        self.vx = side * 40.0 * self.ui;
                        self.vy = 0.0;
                    }
                } else {
                    // below the wall's bottom: a quick jump up onto it first
                    let boost = if self.y > r.b as f32 - 6.0 { 3.2 } else { 1.0 };
                    self.y -= sp * boost * dt;
                    if self.y <= r.t as f32 + 2.0 {
                        // vault over the edge onto the top
                        let p = (wall - side * 16.0 * self.s, r.t as f32);
                        if self.set_on(Surf::Win(h, 0), p) {
                            self.no_land = None;
                            self.act = Act::Land { dur: 0.22 };
                            self.act_t = 0.0;
                            self.quip(&["Parkour!", "Wall run!", "Ninja climb!"], 0.25);
                        } else {
                            self.fall_off(None);
                        }
                    }
                }
                if self.fx_cd <= 0.0 && self.rng.chance(0.15) {
                    self.fx_cd = 0.3;
                    self.fx.push(Fx::new(Kind::Dust, (wall, self.y - 4.0 * self.s), 0.35, self.s).vel(side * 30.0, 20.0));
                }
                false
            }
            Act::Sleep { dur } => {
                if self.fx_cd <= 0.0 {
                    self.fx_cd = 1.1;
                    let hp = (self.x + self.facing * -6.0 * self.s, self.y - 20.0 * self.s);
                    self.fx.push(Fx::new(Kind::Zzz, hp, 2.2, self.s));
                }
                at >= dur || (w.mem.ok && w.mem.load >= w.cfg.sys_alarm)
            }
            Act::Walk { tx, gait } => {
                let Some(surf) = self.on else { return true };
                let goal_abs = match surf {
                    Surf::Floor(_) => tx,
                    Surf::Win(..) => self.last_rect.map(|r| r.l as f32).unwrap_or(0.0) + tx,
                };
                let (lo, hi) = self.walk_range(tr);
                let goal = if lo < hi { goal_abs.clamp(lo, hi) } else { self.x };
                let base = match gait {
                    Gait::Walk => 60.0,
                    Gait::Sneak => 45.0,
                    Gait::Moonwalk => 55.0,
                    Gait::Run => 190.0,
                    Gait::Ninja => 360.0,
                    Gait::Horse => 280.0,
                    Gait::Car => 340.0,
                    Gait::Pogo => 150.0,
                };
                let sp = base * speed_mult * self.ui;
                let d = goal - self.x;
                if d.abs() < 2.5 {
                    if matches!(gait, Gait::Horse | Gait::Car) {
                        self.fx.push(Fx::new(Kind::Poof, (self.x, self.y - 20.0 * self.s), 0.4, self.s * 1.3));
                    }
                    return true;
                }
                self.facing = if gait == Gait::Moonwalk { -sgn(d) } else { sgn(d) };
                let step = (sp * dt).min(d.abs());
                self.x += sgn(d) * step;
                if let (Some(Surf::Win(..)), Some(r)) = (self.on, self.last_rect) {
                    self.off = self.x - r.l as f32;
                }
                let stride = match gait {
                    Gait::Walk | Gait::Sneak | Gait::Moonwalk => 19.0,
                    Gait::Run => 30.0,
                    Gait::Ninja => 38.0,
                    _ => 30.0,
                } * self.s;
                self.phase += step / stride * PI;
                if gait == Gait::Pogo {
                    self.phase = self.t * 7.0;
                }
                // effects
                if self.fx_cd <= 0.0 {
                    if w.cfg.speed_fx && sp > 250.0 * self.ui {
                        self.fx_cd = 0.03;
                        let len = self.rng.range(25.0, 70.0) * self.s;
                        let y = self.y - self.rng.range(5.0, 60.0) * self.s;
                        let x0 = self.x - self.facing * self.rng.range(20.0, 40.0) * self.s;
                        let x0 = if self.facing > 0.0 { x0 - len } else { x0 };
                        self.fx.push(Fx::new(Kind::Line { len }, (x0, y), 0.25, self.s));
                        if self.rng.chance(0.3) {
                            self.fx.push(
                                Fx::new(Kind::Dust, (self.x - self.facing * 8.0 * self.s, self.y - 2.0), 0.4, self.s)
                                    .vel(-self.facing * 30.0, -20.0),
                            );
                        }
                    }
                    if gait == Gait::Car {
                        self.fx_cd = 0.08;
                        let ex = self.x - self.facing * 36.0 * self.s;
                        self.fx.push(Fx::new(Kind::Exhaust, (ex, self.y - 8.0 * self.s), 0.6, self.s).vel(-self.facing * 40.0, -15.0));
                    }
                }
                false
            }
            Act::Leap { to, dur, style, from } => {
                let Some(tp) = self.resolve(to, tr) else {
                    self.fall_off(Some(&["Where'd it go?!"]));
                    return false;
                };
                let u = (at / dur).min(1.0);
                self.facing = sgn(tp.0 - from.0);
                if style == LeapStyle::Flicker {
                    let p = if u < 0.5 { from } else { tp };
                    self.x = p.0;
                    self.y = p.1;
                    if u >= 0.5 && self.fx_cd <= 0.0 && (at - dt) / dur < 0.5 {
                        self.fx.push(Fx::new(Kind::Poof, (tp.0, tp.1 - 25.0 * self.s), 0.45, self.s));
                    }
                } else {
                    let d = dist(from, tp);
                    let up = (from.1 - tp.1).max(0.0);
                    let arc = if style == LeapStyle::Jet { 30.0 * self.ui + up * 0.1 } else { 50.0 * self.ui + 0.22 * d + up * 0.35 };
                    let e = if style == LeapStyle::Jet { ease_io(u) } else { u };
                    let base = lerp_p(from, tp, e);
                    self.x = base.0;
                    self.y = base.1 - 4.0 * u * (1.0 - u) * arc;
                    if style == LeapStyle::Jet && self.fx_cd <= 0.0 {
                        self.fx_cd = 0.05;
                        self.fx.push(Fx::new(Kind::Exhaust, (self.x - self.facing * 10.0 * self.s, self.y - 20.0 * self.s), 0.5, self.s).vel(0.0, 60.0));
                    }
                }
                if u >= 1.0 {
                    if self.set_on(to.surf, tp) {
                        if style != LeapStyle::Flicker {
                            for k in 0..4 {
                                let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
                                self.fx.push(Fx::new(Kind::Dust, (self.x, self.y - 2.0), 0.45, self.s).vel(dir * self.rng.range(30.0, 90.0), -self.rng.range(10.0, 40.0)));
                            }
                        }
                        self.act = Act::Land { dur: if style == LeapStyle::Flicker { 0.12 } else { 0.25 } };
                        self.act_t = 0.0;
                    } else {
                        self.fall_off(None);
                    }
                }
                false
            }
            Act::Whop { dur, item } => {
                let per = if item == Item::Gloves { 0.42 } else { 0.6 };
                let p0 = ((at - dt).max(0.0) % per) / per;
                let p1 = (at % per) / per;
                let hit_at = if item == Item::Gloves { 0.25 } else { 0.55 };
                let crossed = (p0 < hit_at && p1 >= hit_at) || (p1 < p0 && hit_at <= p1);
                if crossed && at < dur - 0.2 {
                    let mut impact = (self.x + self.facing * 30.0 * self.s, self.y - 8.0 * self.s);
                    // whopping a taskbar / tray button: hit the button itself
                    if let Some(Target::Rect(r)) = self.target {
                        let c = ((r.l + r.r) as f32 / 2.0, (r.t + r.b) as f32 / 2.0);
                        if dist(c, impact) < 140.0 * self.ui {
                            impact = (c.0, r.t as f32 + 6.0 * self.ui);
                            self.fx.push(Fx::new(Kind::Flash { w: r.w() as f32, h: r.h() as f32 }, c, 0.45, self.s));
                        }
                    }
                    let word = self.rng.pick(item.hit_word()).to_string();
                    let rot = self.rng.range(-0.3, 0.3);
                    self.fx.push(Fx::new(Kind::Burst { word, col: Col::rgb(255, 225, 60) }, (impact.0, impact.1 - 22.0 * self.s), 0.8, self.s).rot(rot));
                    for _ in 0..3 {
                        let v = (self.rng.range(-120.0, 120.0), self.rng.range(-160.0, -40.0));
                        self.fx.push(Fx::new(Kind::Star, impact, 0.5, self.s).vel(v.0, v.1));
                    }
                    if self.chaos > 0.7 && self.rng.chance(0.3) {
                        self.fx.push(Fx::new(Kind::Sweat, (self.x, self.y - 60.0 * self.s), 0.6, self.s).vel(-self.facing * 50.0, -150.0));
                    }
                }
                at >= dur
            }
            Act::Jutsu { k, dur, n } => {
                self.step_jutsu(k, dur, n, at, dt, w);
                at >= dur
            }
            Act::Stunt { k, dur } => {
                if k == StuntK::Cartwheel {
                    let (lo, hi) = self.walk_range(tr);
                    let nx = (self.x + self.facing * 110.0 * self.s * dt / dur).clamp(lo, hi.max(lo));
                    self.x = nx;
                    if let (Some(Surf::Win(..)), Some(r)) = (self.on, self.last_rect) {
                        self.off = self.x - r.l as f32;
                    }
                }
                if k == StuntK::Headspin && ((at / 0.09) as i32) != (((at - dt) / 0.09) as i32) {
                    self.facing = -self.facing;
                }
                at >= dur
            }
            Act::Fall => {
                self.step_fall(dt, tr);
                false
            }
            Act::Dragged => false,
        }
    }

    fn step_jutsu(&mut self, k: JutsuK, _dur: f32, _n: u32, at: f32, dt: f32, w: &World) {
        let s = self.s;
        let crossed = |t: f32| at >= t && at - dt < t;
        let aura = w.look.aura;
        match k {
            JutsuK::Orb => {
                if at > 1.35 && at < 1.7 {
                    let (lo, hi) = self.walk_range(w.tr);
                    self.x = (self.x + self.facing * 160.0 * self.ui * dt).clamp(lo, hi.max(lo));
                    if let (Some(Surf::Win(..)), Some(r)) = (self.on, self.last_rect) {
                        self.off = self.x - r.l as f32;
                    }
                }
                if crossed(1.65) {
                    let p = (self.x + self.facing * 28.0 * s, self.y - 18.0 * s);
                    self.fx.push(Fx::new(Kind::Boom { col: aura }, p, 0.8, s));
                    self.fx.push(Fx::new(Kind::Burst { word: "RASEN-BONK!".into(), col: Col::rgb(150, 220, 255) }, (p.0, p.1 - 40.0 * s), 1.1, s));
                }
            }
            JutsuK::Shuriken => {
                if crossed(1.75) {
                    let from = (self.x + self.facing * 4.0 * s, self.y - 85.0 * s);
                    // aimed at the hog (window / taskbar button) when it's elsewhere
                    let to = self
                        .far_target()
                        .filter(|p| dist(*p, from) < 1150.0 * self.ui)
                        .unwrap_or((self.x + self.facing * 150.0 * s, self.y - 10.0 * s));
                    let fly = (dist(from, to) / (1300.0 * self.ui)).clamp(0.5, 1.0);
                    let mut f = Fx::new(Kind::Shuriken { to, from, col: aura }, from, fly, s);
                    f.x = from.0;
                    f.y = from.1;
                    self.fx.push(f);
                }
            }
            JutsuK::Kunai => {
                for (i, tt) in [0.25f32, 0.6, 0.95].iter().enumerate() {
                    if crossed(*tt) {
                        let from = (self.x + self.facing * 14.0 * s, self.y - 45.0 * s);
                        let to = (self.x + self.facing * (70.0 + i as f32 * 26.0) * s, self.y + 3.0 * s);
                        self.fx.push(Fx::new(Kind::Kunai { to, from, stick: 0.0 }, from, 4.0, s));
                    }
                }
            }
            JutsuK::Clones => {
                if crossed(0.7) {
                    self.spawn_clones(w);
                    self.quip(&["Shadow clones!", "Multi-bonk!", "There's more of me!", "Everybody do your own thing!"], 0.9);
                }
            }
            JutsuK::Fire => {
                if at > 0.7 && at < 1.9 && self.fx_cd <= 0.0 {
                    self.fx_cd = 0.045;
                    let mouth = (self.x + self.facing * 10.0 * s, self.y - 60.0 * s);
                    let vx = self.facing * self.rng.range(220.0, 300.0) * self.ui;
                    let vy = self.rng.range(-30.0, 50.0) * self.ui;
                    self.fx.push(Fx::new(Kind::Fire { vx }, mouth, 0.55, s).vel(vx, vy).rot(self.rng.range(0.0, 6.0)));
                }
            }
        }
    }

    fn step_fall(&mut self, dt: f32, tr: &Tracker) {
        let g = 2300.0 * self.ui;
        self.vy += g * dt;
        self.vx *= 1.0 - 0.5 * dt;
        let ny = self.y + self.vy * dt;
        let mut nx = self.x + self.vx * dt;
        let (l, r) = (tr.vs.l as f32 + 10.0, tr.vs.r as f32 - 10.0);
        if nx < l {
            nx = l;
            self.vx = self.vx.abs() * 0.6;
        }
        if nx > r {
            nx = r;
            self.vx = -self.vx.abs() * 0.6;
        }
        if self.vy > 0.0 {
            let mut best: Option<(f32, Surf)> = None;
            for seg in &tr.segs {
                if let Some((ns, until)) = self.no_land {
                    if ns == seg.surf && self.t < until {
                        continue;
                    }
                }
                if seg.y >= self.y - 0.5 && seg.y <= ny && nx >= seg.x0 && nx <= seg.x1 {
                    if best.map(|b| seg.y < b.0).unwrap_or(true) {
                        best = Some((seg.y, seg.surf));
                    }
                }
            }
            // safety net: fell below every floor → land on nearest floor
            if best.is_none() {
                let m = tr.mon_at(nx, ny);
                let fy = tr.floor_y(m);
                if ny > fy + 4.0 {
                    let (fl, fr) = tr.floor_range(m);
                    nx = nx.clamp(fl + 10.0, fr - 10.0);
                    best = Some((fy, Surf::Floor(m)));
                }
            }
            if let Some((y, surf)) = best {
                let hard = self.vy > 1400.0 * self.ui;
                if self.set_on(surf, (nx, y)) {
                    for k in 0..5 {
                        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
                        self.fx.push(Fx::new(Kind::Dust, (self.x, self.y - 2.0), 0.5, self.s).vel(dir * self.rng.range(40.0, 120.0), -self.rng.range(10.0, 50.0)));
                    }
                    self.act = Act::Land { dur: if hard { 0.45 } else { 0.22 } };
                    self.act_t = 0.0;
                    if hard {
                        self.quip(&["OOF!", "Nailed it.", "My spine!", "Superhero landing!"], 0.6);
                    }
                    return;
                }
            }
        }
        self.x = nx;
        self.y = ny;
    }

    // ------------------------------------------------------------------
    // decisions
    // ------------------------------------------------------------------

    fn spot_here(&self) -> Option<Spot> {
        match self.on? {
            Surf::Floor(m) => Some(Spot { surf: Surf::Floor(m), x: self.x }),
            s @ Surf::Win(..) => Some(Spot { surf: s, x: self.off }),
        }
    }

    fn random_window_spot(&mut self, tr: &Tracker, exclude: Option<Surf>) -> Option<Spot> {
        let segs: Vec<_> = tr.segs.iter().filter(|s| matches!(s.surf, Surf::Win(..)) && Some(s.surf) != exclude).copied().collect();
        if segs.is_empty() {
            return None;
        }
        let total: f32 = segs.iter().map(|s| s.x1 - s.x0).sum();
        let mut pick = self.rng.f() * total;
        let mut seg = segs[0];
        for s in &segs {
            pick -= s.x1 - s.x0;
            if pick <= 0.0 {
                seg = *s;
                break;
            }
        }
        let Some(h) = seg.surf.hwnd() else { return None };
        let rect_l = tr.win(h)?.rect.l as f32;
        let m = 25.0 * self.ui;
        let x = self.rng.range(seg.x0 + m, (seg.x1 - m).max(seg.x0 + m));
        Some(Spot { surf: seg.surf, x: x - rect_l })
    }

    fn random_floor_spot(&mut self, tr: &Tracker) -> Spot {
        let m = match self.on {
            Some(Surf::Floor(m)) => m,
            _ => tr.mon_at(self.x, self.y),
        };
        let (l, r) = tr.floor_range(m);
        Spot { surf: Surf::Floor(m), x: self.rng.range(l + 60.0, r - 60.0) }
    }

    fn choose_gait(&mut self, d: f32, on_floor: bool, w: &World) -> Gait {
        let ui = self.ui;
        let p = &w.cfg.props;
        if d < 150.0 * ui {
            if self.chaos > 0.5 && self.rng.chance(0.12) {
                return Gait::Moonwalk;
            }
            return if self.rng.chance(0.2) { Gait::Sneak } else { Gait::Walk };
        }
        if on_floor && d > 420.0 * ui {
            let mut opts = vec![];
            if p.horse {
                opts.push(Gait::Horse);
            }
            if p.car {
                opts.push(Gait::Car);
            }
            if !opts.is_empty() && self.rng.chance(0.55) {
                return *self.rng.pick(&opts);
            }
        }
        if p.pogo && self.rng.chance(0.1) {
            return Gait::Pogo;
        }
        if (self.shinobi || w.look.ninja_run) && self.rng.chance(0.75) {
            Gait::Ninja
        } else {
            Gait::Run
        }
    }

    fn plan_travel(&mut self, to: Spot, w: &World) -> Vec<Act> {
        let tr = w.tr;
        let Some(tp) = self.resolve(to, tr) else { return vec![] };
        let cur = self.on;
        if cur == Some(to.surf) {
            let d = (tp.0 - self.x).abs();
            let gait = self.choose_gait(d, matches!(to.surf, Surf::Floor(_)), w);
            return vec![Act::Walk { tx: to.x, gait }];
        }
        if w.cfg.climb && self.rng.chance(0.45) {
            if let Some(v) = self.plan_climb(to, tp, w) {
                return v;
            }
        }
        let mut v = vec![];
        let mut start = (self.x, self.y);
        if let Some(Surf::Floor(m)) = cur {
            let (l, r) = tr.floor_range(m);
            if (tp.0 - self.x).abs() > 450.0 * self.ui && tp.0 > l && tp.0 < r {
                let near = tp.0 - sgn(tp.0 - self.x) * 140.0 * self.ui;
                let gait = self.choose_gait((near - self.x).abs(), true, w);
                v.push(Act::Walk { tx: near, gait });
                start = (near, self.y);
            }
        }
        let d = dist(start, tp);
        let rise = start.1 - tp.1;
        let style = if self.shinobi && w.cfg.jutsu.flicker && self.rng.chance(0.35) {
            LeapStyle::Flicker
        } else if w.cfg.props.jetpack && rise > 280.0 * self.ui && self.rng.chance(0.5) {
            LeapStyle::Jet
        } else if w.cfg.stunts && self.rng.chance(0.55) {
            LeapStyle::Flip
        } else {
            LeapStyle::Hero
        };
        let dur = match style {
            LeapStyle::Flicker => 0.4,
            LeapStyle::Jet => (d / 520.0).clamp(0.9, 2.4),
            _ => (0.45 + d / 1500.0).clamp(0.5, 1.35),
        };
        v.push(Act::Leap { to, dur, style, from: (f32::NAN, f32::NAN) });
        v
    }

    /// Climb a window's side wall instead of leaping: up onto its top edge
    /// from below, or down from its top edge to the floor.
    fn plan_climb(&mut self, to: Spot, tp: P, w: &World) -> Option<Vec<Act>> {
        let tr = w.tr;
        let cur = self.on?;
        let ui = self.ui;
        let s = self.s;
        let (lo, hi) = self.walk_range(tr);
        // surface coordinates of the current surface (offset for windows)
        let to_local = |x: f32, me: &Ninja| match cur {
            Surf::Floor(_) => x,
            Surf::Win(..) => x - me.last_rect.map(|r| r.l as f32).unwrap_or(0.0),
        };
        match to.surf {
            // UP onto a window's top edge
            Surf::Win(h, 0) => {
                let (r, maxed) = Tracker::live_rect(h)?;
                let rise = self.y - r.t as f32;
                let gap = self.y - r.b as f32; // > 0: the wall ends above my feet
                if maxed || rise < 90.0 * ui || rise > 900.0 * ui || gap > 220.0 * ui {
                    return None;
                }
                let mut best: Option<(f32, f32)> = None; // (distance, side)
                for side in [-1.0f32, 1.0] {
                    let wall = if side < 0.0 { r.l as f32 } else { r.r as f32 };
                    let base = wall + side * 7.0 * s;
                    if base < lo || base > hi {
                        continue;
                    }
                    // is the wall's lower part visible-ish (not under another window)?
                    let d = (base - self.x).abs();
                    if best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d, side));
                    }
                }
                let (d, side) = best?;
                let base = (if side < 0.0 { r.l as f32 } else { r.r as f32 }) + side * 7.0 * s;
                let gait = self.choose_gait(d, matches!(cur, Surf::Floor(_)), w);
                let mut v = vec![Act::Walk { tx: to_local(base, self), gait }, Act::Climb { h, side, down: false }];
                if (tp.0 - base).abs() > 60.0 * ui {
                    v.push(Act::Walk { tx: to.x, gait: Gait::Walk });
                }
                Some(v)
            }
            // DOWN from the top edge I'm on to the floor
            Surf::Floor(_) => {
                let Surf::Win(h, 0) = cur else { return None };
                let r = self.last_rect?;
                let seg = tr.seg_at(cur, self.x, 20.0)?;
                let mut sides = vec![];
                if seg.x0 <= r.l as f32 + 14.0 {
                    sides.push(-1.0f32);
                }
                if seg.x1 >= r.r as f32 - 14.0 {
                    sides.push(1.0);
                }
                if sides.is_empty() || tp.1 - self.y < 120.0 * ui {
                    return None;
                }
                let side = *self.rng.pick(&sides);
                let edge = if side < 0.0 { 12.0 * s } else { r.w() as f32 - 12.0 * s };
                Some(vec![
                    Act::Walk { tx: edge, gait: Gait::Walk },
                    Act::Climb { h, side, down: true },
                    Act::Walk { tx: to.x, gait: self.choose_gait(200.0 * ui, true, w) },
                ])
            }
            _ => None,
        }
    }

    fn pick_weapon(&mut self, w: &World) -> Item {
        let p = &w.cfg.props;
        let mut v = vec![];
        for (on, it) in [
            (p.whip, Item::Whip),
            (p.whip, Item::Whip), // the whip is the signature move: double weight
            (p.mallet, Item::Mallet),
            (p.wrench, Item::Wrench),
            (p.crowbar, Item::Crowbar),
            (p.gloves, Item::Gloves),
            (p.chicken, Item::Chicken),
        ] {
            if on {
                v.push(it);
            }
        }
        let fav = match w.look.favorite.as_str() {
            "whip" => Some(Item::Whip),
            "mallet" => Some(Item::Mallet),
            "wrench" => Some(Item::Wrench),
            "crowbar" => Some(Item::Crowbar),
            "gloves" => Some(Item::Gloves),
            "chicken" => Some(Item::Chicken),
            _ => None,
        };
        if let Some(f) = fav {
            if v.contains(&f) && self.rng.chance(0.4) {
                return f;
            }
        }
        if v.is_empty() {
            Item::Gloves
        } else {
            *self.rng.pick(&v)
        }
    }

    fn enabled_jutsu(&self, w: &World) -> Vec<JutsuK> {
        let j = &w.cfg.jutsu;
        let mut v = vec![];
        if !w.cfg.shinobi {
            return v;
        }
        if j.orb {
            v.push(JutsuK::Orb);
        }
        if j.shuriken {
            v.push(JutsuK::Shuriken);
        }
        if j.kunai {
            v.push(JutsuK::Kunai);
        }
        if j.fire {
            v.push(JutsuK::Fire);
        }
        v
    }

    pub fn jutsu_act(k: JutsuK) -> Act {
        let dur = match k {
            JutsuK::Orb => 2.4,
            JutsuK::Shuriken => 2.6,
            JutsuK::Kunai => 1.6,
            JutsuK::Clones => 1.3,
            JutsuK::Fire => 2.2,
        };
        Act::Jutsu { k, dur, n: 0 }
    }

    /// A randomised attack sequence. Eye stares, clones and jutsu are spread
    /// out with random pauses instead of firing back-to-back.
    fn attack(&mut self, w: &World) -> Vec<Act> {
        let mut v = vec![];
        let js = self.enabled_jutsu(w);
        if w.cfg.shinobi && w.cfg.jutsu.eye && self.rng.chance(0.22) {
            v.push(Act::Glare { dur: 1.1 });
            v.push(Act::Idle { dur: self.rng.range(0.8, 2.6), v: 0 });
        }
        if w.cfg.shinobi && w.cfg.jutsu.clones && self.clones.is_empty() && self.rng.chance(0.18) {
            v.push(Self::jutsu_act(JutsuK::Clones));
            v.push(Act::Idle { dur: self.rng.range(0.4, 1.5), v: 0 });
        }
        let fav_jutsu = match w.look.favorite.as_str() {
            "orb" => Some(JutsuK::Orb),
            "shuriken" => Some(JutsuK::Shuriken),
            "kunai" => Some(JutsuK::Kunai),
            _ => None,
        };
        let hits = 1 + self.rng.chance(0.35 * self.chaos) as usize;
        for i in 0..hits {
            if !js.is_empty() && self.rng.chance(0.5) {
                let k = match fav_jutsu {
                    Some(f) if js.contains(&f) && self.rng.chance(0.35) => f,
                    _ => *self.rng.pick(&js),
                };
                v.push(Self::jutsu_act(k));
            } else {
                let item = self.pick_weapon(w);
                v.push(Act::Whop { item, dur: self.rng.range(2.0, 3.4) });
            }
            if i + 1 < hits {
                v.push(Act::Taunt { dur: self.rng.range(0.5, 1.4) });
            }
        }
        v
    }

    /// Where the current target is, right now.
    fn target_point(&self) -> Option<P> {
        match self.target? {
            Target::Rect(r) => Some(((r.l + r.r) as f32 / 2.0, (r.t + r.b) as f32 / 2.0)),
            Target::Win(h) => {
                let (r, _) = Tracker::live_rect(h)?;
                let x = self.x.clamp(r.l as f32 + 40.0, (r.r as f32 - 40.0).max(r.l as f32 + 40.0));
                Some((x, r.t as f32 + 14.0 * self.ui))
            }
        }
    }

    /// Target point if it's somewhere else (not right under my feet).
    fn far_target(&self) -> Option<P> {
        let p = self.target_point()?;
        if let (Some(Target::Win(h)), Some(on)) = (self.target, self.on) {
            if on.hwnd() == Some(h) {
                return None;
            }
        }
        (dist(p, (self.x, self.y - 30.0 * self.s)) > 120.0 * self.ui).then_some(p)
    }

    fn find_hog(&self, w: &World) -> Option<ProcStat> {
        let m = w.mem;
        if !m.ok || self.t - self.last_bonk < w.cfg.bonk_cooldown_s as f32 {
            return None;
        }
        let thr = w.cfg.bonk_mb as u64 * MB;
        let alarm = m.load >= w.cfg.sys_alarm;
        for p in m.procs.iter().take(15) {
            if w.cfg.ignored(&p.name) || p.name.is_empty() {
                continue;
            }
            if let Some(t) = self.bonked.get(&p.name) {
                if self.t - t < w.cfg.bonk_cooldown_s as f32 * 3.0 {
                    continue;
                }
            }
            if p.bytes >= thr || (alarm && w.tr.has_window(&p.name)) {
                return Some(p.clone());
            }
            if !alarm {
                break;
            }
        }
        None
    }

    fn plan_bonk(&mut self, hog: &ProcStat, w: &World) -> Vec<Act> {
        let tr = w.tr;
        let pretty = pretty_name(&hog.name);
        let mut spot = None;
        let mut target = None;
        let mut taunt = format!("{}: {}?!", pretty, fmt_bytes(hog.bytes));
        let wins: Vec<_> = tr.windows_of(&hog.name).into_iter().cloned().collect();
        let width = |a: &&Seg, b: &&Seg| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0));
        // 1. a visible part of one of its open windows (top edge, else an inner ledge)
        for wi in &wins {
            let segs: Vec<Seg> = tr.segs.iter().filter(|s| s.surf.hwnd() == Some(wi.hwnd)).copied().collect();
            let best = segs.iter().filter(|s| !s.surf.is_ledge()).max_by(width).or_else(|| segs.iter().max_by(width)).copied();
            if let Some(seg) = best {
                let m = 30.0 * self.ui;
                let x = self.rng.range(seg.x0 + m, (seg.x1 - m).max(seg.x0 + m));
                spot = Some(Spot { surf: seg.surf, x: x - wi.rect.l as f32 });
                target = Some(Target::Win(wi.hwnd));
                break;
            }
        }
        // 2. minimized / covered: its taskbar or tray button
        if spot.is_none() {
            if let Some(b) = taskbar::find(w.tb, &hog.name, &tr.titles_of(&hog.name)) {
                let cx = (b.l + b.r) as f32 / 2.0;
                let m = tr.mon_at(cx, b.t as f32 - 4.0);
                let (l, r) = tr.floor_range(m);
                let side = if self.x < cx { -1.0 } else { 1.0 };
                let x = (cx + side * 30.0 * self.s).clamp(l + 14.0, r - 14.0);
                spot = Some(Spot { surf: Surf::Floor(m), x });
                target = Some(Target::Rect(b));
                taunt = format!("Hiding in the taskbar, {}? ({})", pretty, fmt_bytes(hog.bytes));
            }
        }
        // 3. a covered window: stand on the floor under it
        if spot.is_none() {
            if let Some(wi) = wins.first() {
                let cx = (wi.rect.l + wi.rect.r) as f32 / 2.0;
                let m = tr.mon_at(cx, (wi.rect.t + wi.rect.b) as f32 / 2.0);
                let (l, r) = tr.floor_range(m);
                spot = Some(Spot { surf: Surf::Floor(m), x: (cx + self.rng.range(-200.0, 200.0)).clamp(l + 60.0, r - 60.0) });
                target = Some(Target::Win(wi.hwnd));
                taunt = format!("Found you, {}! ({})", pretty, fmt_bytes(hog.bytes));
            } else {
                spot = Some(self.random_floor_spot(tr));
                taunt = format!("Invisible hog {}: {}", pretty, fmt_bytes(hog.bytes));
            }
        }
        self.target = target;
        let mut v = vec![];
        let line2 = self.rng.pick(&["Bonk time.", "Prepare yourself.", "Nothing personal.", "Diet starts NOW.", "Whip it. Whip it good."]).to_string();
        // sometimes open with a long-range wind-spiral shuriken from right here
        let ranged = self.shinobi
            && w.cfg.jutsu.shuriken
            && self.far_target().map(|p| dist(p, (self.x, self.y)) < 1050.0 * self.ui && dist(p, (self.x, self.y)) > 220.0 * self.ui).unwrap_or(false)
            && self.rng.chance(0.4);
        if ranged {
            v.push(Act::Say { lines: vec![taunt.clone(), "Rasen-shuriken incoming!".into()], dur: 3.0, think: false });
            v.push(Self::jutsu_act(JutsuK::Shuriken));
            v.push(Act::Idle { dur: self.rng.range(0.5, 1.6), v: 1 });
        }
        v.extend(self.plan_travel(spot.unwrap(), w));
        if !ranged {
            v.push(Act::Say { lines: vec![taunt, line2], dur: 3.2, think: false });
        }
        v.push(Act::Taunt { dur: self.rng.range(0.4, 1.0) });
        v.extend(self.attack(w));
        if self.rng.chance(0.5) {
            v.push(Act::Taunt { dur: 1.0 });
        }
        v
    }

    fn kept_target(&mut self, w: &World) -> Option<String> {
        let mut c: Vec<String> = w.cfg.keep.apps.iter().filter(|a| w.tr.has_window(a)).cloned().collect();
        if c.is_empty() {
            return None;
        }
        Some(c.swap_remove(self.rng.idx(c.len())))
    }

    fn decide(&mut self, w: &World) -> Vec<Act> {
        let tr = w.tr;
        let cfg = w.cfg;
        if self.on.is_none() {
            return vec![Act::Fall];
        }
        self.target = None;
        // clones are out: stay with them and show off on the spot
        if !self.clones.is_empty() {
            return match self.rng.idx(5) {
                0 => vec![Act::Dance { dur: self.rng.range(2.0, 3.5) }],
                1 => vec![Act::Say { lines: vec![self.rng.pick(&["Get 'em, boys!", "Clone party!", "Which one is the real me?"]).to_string()], dur: 2.0, think: false }, Act::Taunt { dur: 1.5 }],
                2 if w.cfg.stunts => vec![Act::Stunt { k: StuntK::Backflip, dur: 0.85 }, Act::Idle { dur: 1.0, v: 0 }],
                3 => {
                    let item = self.pick_weapon(w);
                    vec![Act::Whop { item, dur: self.rng.range(1.5, 2.5) }]
                }
                _ => vec![Act::Idle { dur: self.rng.range(1.0, 2.5), v: self.rng.idx(3) as u8 }],
            };
        }
        // 0. you've been away for a while: go rest on the taskbar
        if cfg.rest_after_min > 0 && w.user_idle > cfg.rest_after_min as f32 * 60.0 && !(w.mem.ok && w.mem.load >= cfg.sys_alarm) {
            let mut v = vec![];
            if !matches!(self.on, Some(Surf::Floor(_))) {
                let s = self.random_floor_spot(tr);
                let s = Spot { x: self.x + self.rng.range(-200.0, 200.0) * self.ui, ..s };
                v.extend(self.plan_travel(s, w));
            }
            v.push(Act::Stunt { k: StuntK::Scratch, dur: 1.0 });
            v.push(Act::Sit { dur: self.rng.range(1.5, 3.0) });
            v.push(Act::Rest);
            return v;
        }
        // 1. periodic memory callout
        if cfg.clouds && self.t - self.last_callout > cfg.callout_s as f32 {
            self.last_callout = self.t;
            return vec![if cfg.props.megaphone { Act::Shout { dur: 3.0 } } else { Act::Shout { dur: 2.5 } }];
        }
        // 2. bonk a memory hog
        if let Some(h) = self.find_hog(w) {
            self.last_bonk = self.t;
            self.bonked.insert(h.name.clone(), self.t);
            let v = self.plan_bonk(&h, w);
            if !v.is_empty() {
                return v;
            }
        }
        // 3. visit a kept-active app
        if cfg.keep.enabled && cfg.keep.visits && self.t - self.last_visit > 150.0 {
            if let Some(app) = self.kept_target(w) {
                self.last_visit = self.t;
                let wins: Vec<_> = tr.windows_of(&app).into_iter().cloned().collect();
                let mut spot = None;
                for wi in &wins {
                    if let Some(seg) = tr.segs_of(Surf::Win(wi.hwnd, 0)).next().copied() {
                        let x = (seg.x0 + seg.x1) / 2.0;
                        spot = Some(Spot { surf: seg.surf, x: x - wi.rect.l as f32 });
                        break;
                    }
                }
                let spot = spot.unwrap_or_else(|| self.random_floor_spot(tr));
                let mut v = self.plan_travel(spot, w);
                let name = pretty_name(&app);
                let line = self.rng.pick(&["Stay awake, {}!", "No napping, {}!", "{}: still green. You're welcome.", "Poke poke, {}."]).replace("{}", &name);
                v.push(Act::Say { lines: vec![line], dur: 2.6, think: false });
                v.push(Act::Poke { dur: 2.2 });
                return v;
            }
        }
        // 4. nap when things are calm
        if cfg.nap && w.mem.ok && w.mem.load < cfg.nap_below && self.rng.chance(0.15) {
            let mut v = vec![];
            if matches!(self.on, Some(Surf::Win(..))) && self.rng.chance(0.5) {
                v.push(Act::Sit { dur: self.rng.range(2.0, 5.0) });
            }
            v.push(Act::Sleep { dur: self.rng.range(8.0, 20.0) });
            v.push(Act::Stunt { k: StuntK::Scratch, dur: 1.2 });
            return v;
        }
        // 5. free roam
        let ch = self.chaos;
        let on_win = matches!(self.on, Some(Surf::Win(..)));
        let on_floor = matches!(self.on, Some(Surf::Floor(_)));
        let mounts = cfg.props.horse || cfg.props.car;
        let t = self.t;
        let stunt_ok = cfg.stunts && t >= self.next_stunt;
        let jutsu_ok = cfg.shinobi && t >= self.next_jutsu;
        let ride_ok = on_floor && mounts && t >= self.next_ride;
        let mut opts: Vec<(f32, u8)> = vec![
            (3.0, 0),                                                    // wander
            (if cfg.perch { 2.6 } else { 0.0 }, 1),                      // perch on another window / ledge
            (if on_win { 0.9 } else { 0.0 }, 2),                         // back to floor
            (if on_win { 1.6 } else { 0.4 }, 3),                         // sit
            (if stunt_ok { 0.4 + 1.4 * ch } else { 0.0 }, 4),            // stunt
            (if stunt_ok { 0.2 + 0.8 * ch } else { 0.0 }, 5),            // dance
            (1.2, 6),                                                    // idle
            (if ride_ok { 0.8 } else { 0.0 }, 7),                        // joyride
            (if jutsu_ok { 0.5 + 0.7 * ch } else { 0.0 }, 8),            // jutsu show-off
            (0.5, 9),                                                    // taunt / quip
            (if cfg.clouds { 0.25 } else { 0.0 }, 10),                   // random memory thought
            // rare stuff: seen "every now and then"
            (if cfg.shinobi && cfg.jutsu.eye { 0.12 } else { 0.0 }, 11), // red-eye stare
            (if cfg.props.jetpack { 0.1 } else { 0.0 }, 12),             // jetpack tour
            (if on_floor { 0.15 + 0.2 * ch } else { 0.0 }, 13),          // full-width sprint
            (if cfg.climb && cfg.perch { 0.35 } else { 0.0 }, 14),       // climb a window
            (0.12, 15),                                                  // knock on a random window
        ];
        // don't repeat what he just did
        for o in opts.iter_mut() {
            let n = self.recent.iter().filter(|r| **r == o.1).count();
            o.0 *= 0.3f32.powi(n as i32);
        }
        let total: f32 = opts.iter().map(|o| o.0).sum();
        let mut r = self.rng.f() * total;
        let mut choice = 0;
        for (wgt, id) in opts {
            r -= wgt;
            if r <= 0.0 {
                choice = id;
                break;
            }
        }
        self.recent.push_back(choice);
        while self.recent.len() > 5 {
            self.recent.pop_front();
        }
        let calm = 1.5 - ch; // more chaos = shorter cooldowns
        match choice {
            4 | 5 => self.next_stunt = t + self.rng.range(6.0, 30.0) * calm,
            7 => self.next_ride = t + self.rng.range(40.0, 160.0) * calm,
            8 => self.next_jutsu = t + self.rng.range(30.0, 120.0) * calm,
            _ => {}
        }
        match choice {
            11 => {
                let line = self.rng.pick(&["I see your RAM.", "...", "Who's using all the memory?", "*stares in tomoe*"]).to_string();
                return vec![Act::Glare { dur: 1.2 }, Act::Say { lines: vec![line], dur: 2.0, think: false }, Act::Taunt { dur: 0.8 }];
            }
            12 => {
                if let Some(s) = self.random_window_spot(tr, self.on) {
                    return vec![Act::Say { lines: vec!["JETPACK TIME!".into()], dur: 1.5, think: false }, Act::Leap { to: s, dur: 2.2, style: LeapStyle::Jet, from: (f32::NAN, f32::NAN) }];
                }
                return self.wander(w);
            }
            13 => {
                let (l, r) = match self.on {
                    Some(Surf::Floor(m)) => tr.floor_range(m),
                    _ => return self.wander(w),
                };
                let target = if self.x - l > r - self.x { l + 30.0 } else { r - 30.0 };
                let gait = if self.shinobi || w.look.ninja_run { Gait::Ninja } else { Gait::Run };
                let shout: &[&str] = if w.look.gait == crate::skins::Gait::Panic {
                    &["Aaha... kilambitaangayaa!", "Odu! Odu!", "Escape! RAM is coming!"]
                } else {
                    &["SPRINT!", "Gotta go fast!", "Taskbar marathon!"]
                };
                return vec![Act::Say { lines: vec![self.rng.pick(shout).to_string()], dur: 1.4, think: false }, Act::Walk { tx: target, gait }, Act::Taunt { dur: 0.8 }];
            }
            14 => {
                // pick a window top and insist on climbing it
                for _ in 0..4 {
                    if let Some(s) = self.random_window_spot(tr, self.on) {
                        if s.surf.is_ledge() {
                            continue;
                        }
                        if let Some(tp) = self.resolve(s, tr) {
                            if let Some(mut v) = self.plan_climb(s, tp, w) {
                                if self.rng.chance(0.5) {
                                    v.push(Act::Sit { dur: self.rng.range(3.0, 7.0) });
                                }
                                return v;
                            }
                        }
                    }
                }
                return self.wander(w);
            }
            15 => {
                if let Some(s) = self.random_window_spot(tr, self.on) {
                    let mut v = self.plan_travel(s, w);
                    v.push(Act::Say { lines: vec![self.rng.pick(&["Knock knock.", "Anybody home?", "Nice pane you've got here.", "Is this a toolbar? I'm standing on it."]).to_string()], dur: 2.2, think: false });
                    v.push(Act::Poke { dur: 1.6 });
                    return v;
                }
                return self.wander(w);
            }
            _ => {}
        }
        match choice {
            1 => {
                if let Some(s) = self.random_window_spot(tr, self.on) {
                    let mut v = self.plan_travel(s, w);
                    if self.rng.chance(0.5) {
                        v.push(Act::Sit { dur: self.rng.range(3.0, 8.0) });
                    }
                    return v;
                }
                self.wander(w)
            }
            2 => {
                let s = self.random_floor_spot(tr);
                let s = Spot { x: (self.x + self.rng.range(-250.0, 250.0) * self.ui), ..s };
                self.plan_travel(s, w)
            }
            3 => vec![Act::Sit { dur: self.rng.range(3.0, 9.0) }],
            4 => {
                let k = *self.rng.pick(&[StuntK::Backflip, StuntK::Cartwheel, StuntK::Handstand, StuntK::Headspin, StuntK::Split, StuntK::Backflip]);
                let dur = match k {
                    StuntK::Backflip => 0.85,
                    StuntK::Cartwheel => 1.0,
                    StuntK::Handstand => 2.5,
                    StuntK::Headspin => 2.0,
                    StuntK::Split => 0.8,
                    StuntK::Scratch => 1.2,
                };
                let mut v = vec![Act::Stunt { k, dur }];
                if self.rng.chance(0.3 * ch) {
                    v.push(Act::Stunt { k: StuntK::Backflip, dur: 0.85 });
                }
                v.push(Act::Taunt { dur: 0.6 });
                v
            }
            5 => vec![Act::Dance { dur: self.rng.range(2.5, 5.0) }],
            7 => {
                let (l, r) = match self.on {
                    Some(Surf::Floor(m)) => tr.floor_range(m),
                    _ => (self.x - 100.0, self.x + 100.0),
                };
                let target = if self.x - l > r - self.x { l + 80.0 } else { r - 80.0 };
                let mut opts = vec![];
                if cfg.props.horse {
                    opts.push(Gait::Horse);
                }
                if cfg.props.car {
                    opts.push(Gait::Car);
                }
                if opts.is_empty() {
                    return self.wander(w);
                }
                let gait = *self.rng.pick(&opts);
                let line = if gait == Gait::Horse { "YEEHAW!" } else { "VROOOM!" };
                vec![Act::Say { lines: vec![line.into()], dur: 1.6, think: false }, Act::Walk { tx: target, gait }]
            }
            8 => {
                let mut js = self.enabled_jutsu(w);
                if cfg.jutsu.clones && self.clones.is_empty() {
                    js.push(JutsuK::Clones);
                }
                if js.is_empty() {
                    return self.wander(w);
                }
                let k = *self.rng.pick(&js);
                vec![Self::jutsu_act(k), Act::Taunt { dur: 0.6 }]
            }
            9 => {
                let mut v = vec![Act::Taunt { dur: 1.4 }];
                let line = if !w.look.phrases.is_empty() && self.rng.chance(0.6) {
                    self.rng.pick(&w.look.phrases).clone()
                } else {
                    self.rng
                        .pick(&["*cracks knuckles*", "Any hogs around?", "I'm watching your RAM.", "Bored. Bonk something?", "Nice wallpaper."])
                        .to_string()
                };
                v.insert(0, Act::Say { lines: vec![line], dur: 2.2, think: false });
                v
            }
            10 => {
                let lines = self.memory_lines(w);
                vec![Act::Say { lines, dur: 4.0, think: true }, Act::Sit { dur: 3.0 }]
            }
            6 => {
                let v = self.rng.idx(3) as u8;
                vec![Act::Idle { dur: self.rng.range(1.5, 4.0), v }]
            }
            _ => self.wander(w),
        }
    }

    fn wander(&mut self, w: &World) -> Vec<Act> {
        let (lo, hi) = self.walk_range(w.tr);
        if hi - lo < 10.0 {
            return vec![Act::Idle { dur: 1.5, v: 0 }];
        }
        let tx_abs = self.rng.range(lo, hi);
        let tx = match self.on {
            Some(Surf::Win(..)) => tx_abs - self.last_rect.map(|r| r.l as f32).unwrap_or(0.0),
            _ => tx_abs,
        };
        let gait = self.choose_gait((tx_abs - self.x).abs(), matches!(self.on, Some(Surf::Floor(_))), w);
        vec![Act::Walk { tx, gait }, Act::Idle { dur: self.rng.range(0.6, 2.0), v: self.rng.idx(3) as u8 }]
    }

    // ------------------------------------------------------------------
    // commands from menu / control center
    // ------------------------------------------------------------------

    fn interrupt_ok(&self) -> bool {
        self.on.is_some() && !self.busy_with_motion()
    }

    pub fn callout_now(&mut self, w: &World) {
        let lines = self.memory_lines(w);
        self.say(lines, 5.0, false);
        if self.interrupt_ok() {
            self.plan.clear();
            self.begin(Act::Taunt { dur: 1.2 }, w);
        }
        self.last_callout = self.t;
    }

    pub fn bonk_now(&mut self, w: &World) {
        if !self.interrupt_ok() {
            return;
        }
        let hog = w.mem.procs.iter().find(|p| !w.cfg.ignored(&p.name)).cloned();
        if let Some(h) = hog {
            self.plan.clear();
            self.last_bonk = self.t;
            let v = self.plan_bonk(&h, w);
            self.plan.extend(v);
            self.next(w);
        }
    }

    /// Whop a specific app (by exe name), wherever it's shown.
    pub fn whop_app(&mut self, exe: &str, w: &World) {
        if !self.interrupt_ok() {
            return;
        }
        let exe = exe.trim().to_lowercase();
        let exe = if exe.ends_with(".exe") { exe } else { format!("{exe}.exe") };
        let hog = w.mem.procs.iter().find(|p| p.name == exe).cloned().unwrap_or(ProcStat { name: exe.clone(), ..Default::default() });
        self.plan.clear();
        let v = self.plan_bonk(&hog, w);
        self.plan.extend(v);
        self.next(w);
    }

    pub fn jutsu_now(&mut self, k: JutsuK, w: &World) {
        if !self.interrupt_ok() {
            return;
        }
        self.plan.clear();
        self.plan.push_back(Act::Taunt { dur: 0.6 });
        self.begin(Self::jutsu_act(k), w);
    }

    /// Hop down to the floor (if needed) and ride across it on a horse or car.
    pub fn ride_now(&mut self, w: &World) {
        if !self.interrupt_ok() {
            return;
        }
        let tr = w.tr;
        let spot = self.random_floor_spot(tr);
        let Surf::Floor(m) = spot.surf else { return };
        self.plan.clear();
        let mut v = if matches!(self.on, Some(Surf::Floor(_))) { vec![] } else { self.plan_travel(spot, w) };
        let start_x = if v.is_empty() { self.x } else { spot.x };
        let (l, r) = tr.floor_range(m);
        let tx = if start_x - l > r - start_x { l + 80.0 } else { r - 80.0 };
        let mut opts = vec![];
        if w.cfg.props.horse {
            opts.push(Gait::Horse);
        }
        if w.cfg.props.car {
            opts.push(Gait::Car);
        }
        let gait = if opts.is_empty() { Gait::Ninja } else { *self.rng.pick(&opts) };
        let line = match gait {
            Gait::Horse => "YEEHAW!",
            Gait::Car => "VROOOM!",
            _ => "Ninja run!",
        };
        v.push(Act::Say { lines: vec![line.into()], dur: 1.6, think: false });
        v.push(Act::Walk { tx, gait });
        self.plan.extend(v);
        self.next(w);
    }

    pub fn come_here(&mut self, w: &World) {
        let (cx, cy) = crate::win::cursor_pos();
        self.plan.clear();
        self.on = None;
        self.x = cx as f32;
        self.y = cy as f32 - 40.0;
        self.fx.push(Fx::new(Kind::Poof, (self.x, self.y), 0.5, self.s));
        self.act = Act::Fall;
        self.vy = 0.0;
        self.vx = 0.0;
        self.act_t = 0.0;
        self.quip(&["You called?", "I'm here!"], 1.0);
        let _ = w;
    }

    pub fn greet(&mut self) {
        self.say(vec!["Yo! I'm RamRonin.".into(), "I bonk memory hogs.".into(), "Right-click me for settings!".into()], 7.0, false);
    }

    // ------------------------------------------------------------------
    // mouse
    // ------------------------------------------------------------------

    pub fn mouse_down(&mut self, p: P) {
        self.press = Some((p, self.t));
        self.dragging = false;
        self.grab_samples.clear();
    }

    pub fn mouse_move(&mut self, p: P, w: &World) {
        let Some((p0, _)) = self.press else { return };
        if !self.dragging {
            if dist(p, p0) < 5.0 * self.ui {
                return;
            }
            // Substitution jutsu: leave a log in your hand and vanish.
            if self.shinobi && w.cfg.jutsu.substitution && self.on.is_some() && self.rng.chance(0.35) {
                self.press = None;
                let c = (self.x, self.y - 35.0 * self.s);
                self.fx.push(Fx::new(Kind::Poof, c, 0.5, self.s * 1.2));
                self.fx.push(Fx::new(Kind::Log { vy: -200.0, ang: 0.3 }, c, 1.4, self.s).vel(self.rng.range(-60.0, 60.0), 0.0));
                let spot = self.random_window_spot(w.tr, self.on).unwrap_or_else(|| self.random_floor_spot(w.tr));
                if let Some(tp) = self.resolve(spot, w.tr) {
                    if self.set_on(spot.surf, tp) {
                        self.fx.push(Fx::new(Kind::Poof, (self.x, self.y - 25.0 * self.s), 0.5, self.s));
                    }
                }
                self.plan.clear();
                self.act = Act::Taunt { dur: 1.5 };
                self.act_t = 0.0;
                self.cloud = None;
                self.quip(&["Substitution! Nice try.", "Ha! A log.", "Too slow!"], 1.0);
                return;
            }
            self.dragging = true;
            self.plan.clear();
            self.on = None;
            self.act = Act::Dragged;
            self.act_t = 0.0;
            self.quip(&["Put me down!", "Hey hey HEY!", "Unhand me!", "Wheee!"], 0.7);
        }
        let hold = (rig::TH + rig::SH + rig::TO + rig::NK + rig::HR) * self.s - 4.0 * self.s;
        self.x = p.0;
        self.y = p.1 + hold;
        self.grab_samples.push_back((p, self.t));
        while self.grab_samples.len() > 6 {
            self.grab_samples.pop_front();
        }
    }

    /// Returns true if it was a plain click (no drag).
    pub fn mouse_up(&mut self, _p: P) -> bool {
        let was_press = self.press.take().is_some();
        if self.dragging {
            self.dragging = false;
            let (mut vx, mut vy) = (0.0, 0.0);
            if let (Some(a), Some(b)) = (self.grab_samples.front(), self.grab_samples.back()) {
                let dt = (b.1 - a.1).max(0.016);
                vx = (b.0 .0 - a.0 .0) / dt;
                vy = (b.0 .1 - a.0 .1) / dt;
            }
            let lim = 2600.0 * self.ui;
            self.vx = vx.clamp(-lim, lim);
            self.vy = vy.clamp(-lim, lim);
            self.act = Act::Fall;
            self.act_t = 0.0;
            if (vx * vx + vy * vy).sqrt() > 1200.0 * self.ui {
                self.quip(&["WHEEEEE!", "YEET!", "I CAN FLY", "Not again!"], 1.0);
            }
            return false;
        }
        was_press
    }

    pub fn click(&mut self, w: &World) {
        if !self.interrupt_ok() {
            return;
        }
        let r = self.rng.idx(4);
        self.plan.clear();
        match r {
            0 => {
                self.quip(&["Hey!", "That tickles!", "Personal space!", "Boop received."], 1.0);
                self.begin(Act::Stunt { k: StuntK::Backflip, dur: 0.85 }, w);
            }
            1 => {
                self.fx.push(Fx::new(Kind::Heart, (self.x, self.y - 75.0 * self.s), 1.2, self.s).vel(0.0, -40.0));
                self.begin(Act::Dance { dur: 2.0 }, w);
            }
            2 => self.callout_now(w),
            _ => {
                self.quip(&["You rang?", "*cracks knuckles*", "Need something bonked?"], 1.0);
                self.begin(Act::Taunt { dur: 1.2 }, w);
            }
        }
    }

    // ------------------------------------------------------------------
    // pose & layout & drawing
    // ------------------------------------------------------------------

    fn pose(&self, w: &World) -> (Pose, Item, Option<Gait>, f32) {
        let t = self.t;
        let at = self.act_t;
        let mut swing = 0.0;
        let (pose, item, gait) = match &self.act {
            Act::Idle { v, .. } => (
                match v {
                    0 => poses::stance(t),
                    2 => poses::scratch(t),
                    _ => poses::casual(t),
                },
                Item::None,
                None,
            ),
            Act::Walk { gait, .. } => {
                let p = match gait {
                    Gait::Walk => poses::walk(self.phase),
                    Gait::Sneak => poses::sneak(self.phase),
                    Gait::Moonwalk => poses::walk(-self.phase),
                    Gait::Run => w.look.sprint(false, self.phase),
                    Gait::Ninja => w.look.sprint(true, self.phase),
                    Gait::Horse => poses::ride(t),
                    Gait::Car => poses::drive(t),
                    Gait::Pogo => poses::pogo(),
                };
                (p, Item::None, Some(*gait))
            }
            Act::Leap { style, dur, .. } => {
                let u = (at / dur).min(1.0);
                let p = match style {
                    LeapStyle::Flip => poses::tuck(360.0 * ease_io(u)),
                    LeapStyle::Hero | LeapStyle::Flicker => poses::hero(t),
                    LeapStyle::Jet => poses::superman(t),
                };
                (p, Item::None, None)
            }
            Act::Sit { .. } => (poses::sit(t), Item::None, None),
            Act::Sleep { .. } => (poses::sleep(t), Item::None, None),
            Act::Meditate => (poses::meditate(t), Item::None, None),
            Act::Whop { item, .. } => {
                let per = if *item == Item::Gloves { 0.42 } else { 0.6 };
                swing = (at % per) / per;
                let p = if *item == Item::Gloves { poses::punches(swing) } else { poses::swing(swing) };
                (p, *item, None)
            }
            Act::Jutsu { k, .. } => {
                let p = match k {
                    JutsuK::Orb => {
                        if at < 0.4 {
                            poses::signs(t)
                        } else if at < 1.35 {
                            poses::charge(t)
                        } else if at < 1.75 {
                            Pose { torso: 45.0, ..poses::charge(t) }
                        } else {
                            poses::stance(t)
                        }
                    }
                    JutsuK::Shuriken => {
                        if at < 0.45 {
                            poses::signs(t)
                        } else if at < 1.6 {
                            poses::hold_up(t)
                        } else if at < 1.9 {
                            poses::throw((at - 1.6) / 0.3)
                        } else {
                            poses::stance(t)
                        }
                    }
                    JutsuK::Kunai => {
                        if at < 1.15 {
                            poses::throw(((at + 0.1) % 0.35) / 0.35)
                        } else {
                            poses::stance(t)
                        }
                    }
                    JutsuK::Clones => poses::signs(t),
                    JutsuK::Fire => {
                        if at < 0.65 {
                            poses::signs(t)
                        } else {
                            let mut p = poses::shout(t);
                            p.torso = 14.0;
                            p.head = -10.0;
                            p.ua_f = 40.0;
                            p.fa_f = 150.0;
                            p
                        }
                    }
                };
                let item = if *k == JutsuK::Kunai && at < 1.15 { Item::Kunai } else { Item::None };
                (p, item, None)
            }
            Act::Stunt { k, dur } => {
                let u = (at / dur).min(1.0);
                let p = match k {
                    StuntK::Backflip => {
                        let mut p = poses::tuck(-360.0 * ease_io(u));
                        p.dy = -(u * PI).sin() * 42.0;
                        p
                    }
                    StuntK::Cartwheel => poses::spread(360.0 * u, (u * PI).sin() * 8.0),
                    StuntK::Handstand => poses::handstand(t),
                    StuntK::Headspin => poses::headspin(t),
                    StuntK::Split => poses::split_jump(u),
                    StuntK::Scratch => poses::scratch(t),
                };
                (p, Item::None, None)
            }
            Act::Dance { .. } => (poses::dance(t, self.chaos), Item::None, None),
            Act::Shout { .. } => (poses::shout(t), if w.cfg.props.megaphone { Item::Megaphone } else { Item::None }, None),
            Act::Poke { .. } => (poses::poke(t), Item::Stick, None),
            Act::Taunt { .. } | Act::Say { .. } => (poses::taunt(t), Item::None, None),
            Act::Fall => (poses::fall(t), Item::None, None),
            Act::Stumble { .. } => (poses::stumble(at), Item::None, None),
            Act::Land { dur } => (poses::land(at / dur), Item::None, None),
            Act::Dragged => (poses::dragged(t), Item::None, None),
            Act::Climb { .. } => (poses::climb(self.phase), Item::None, None),
            Act::Rest => (poses::sleep(t), Item::None, None),
            Act::Glare { .. } => {
                let mut p = poses::stance(t);
                p.head = -6.0;
                p.expr = rig::Expr::Focus;
                (p, Item::None, None)
            }
        };
        (pose, item, gait, swing)
    }

    pub fn layout(&mut self, w: &World) -> Layout {
        let (pose, item, gait, swing) = self.pose(w);
        let s = self.s;
        let ground = (self.x, self.y);
        let root = match gait {
            Some(Gait::Horse) => (ground.0 - 2.0 * s * self.facing, ground.1 - 44.0 * s),
            Some(Gait::Car) => props::car_seat(ground, self.facing, s),
            Some(Gait::Pogo) => (ground.0, ground.1 - (14.0 + 18.0 * (self.phase).sin().abs()) * s),
            _ => ground,
        };
        let skel = rig::solve(&pose, s, self.facing, root);
        let hidden = matches!(self.act, Act::Leap { style: LeapStyle::Flicker, .. }) && {
            if let Act::Leap { dur, .. } = self.act {
                let u = self.act_t / dur;
                u > 0.12 && u < 0.88
            } else {
                false
            }
        };
        let clones = self
            .clones
            .iter()
            .filter_map(|c| {
                let at = c.at?;
                let (p, item, swing, orb) = self.clone_pose(c);
                Some(CloneDraw { skel: rig::solve(&p, s, c.facing, at), item, swing, orb })
            })
            .collect::<Vec<_>>();
        let fast = (self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt() > 260.0 * self.ui;
        let tomoe = self.shinobi
            && w.cfg.jutsu.eye
            && (matches!(self.act, Act::Whop { .. } | Act::Jutsu { .. } | Act::Glare { .. }) || matches!(gait, Some(Gait::Ninja)));
        // trails
        if w.cfg.speed_fx && fast && !hidden && gait != Some(Gait::Car) {
            self.trail.push_front((skel, 0.0));
            while self.trail.len() > 4 {
                self.trail.pop_back();
            }
        }
        if tomoe && fast && !hidden {
            let eye = (skel.head.0 + skel.fwd.0 * skel.head_r * 0.48, skel.head.1 + skel.fwd.1 * skel.head_r * 0.48 + skel.up.1 * skel.head_r * 0.12);
            self.eye_trail.push_front((eye, 0.0));
        }
        // cloud box
        let cloud = self.cloud.as_ref().map(|c| {
            let ui = self.ui;
            let fs = 13.5 * ui;
            let lh = fs * 1.32;
            let wmax = c.lines.iter().map(|l| gfx::text_width(fs, l)).fold(0.0, f32::max);
            let cw = wmax + 30.0 * ui;
            let ch = c.lines.len() as f32 * lh + 18.0 * ui;
            let top_body = skel.bbox().1;
            let bottom = top_body - 16.0 * ui;
            let m = &w.tr.mons[w.tr.mon_at(self.x, self.y - 10.0)];
            let cx = skel.head.0.clamp(m.work.l as f32 + cw / 2.0 + 14.0 * ui, m.work.r as f32 - cw / 2.0 - 14.0 * ui);
            let top = (bottom - ch).max(m.rect.t as f32 + 12.0 * ui);
            (cx - cw / 2.0, top, cx + cw / 2.0, top + ch)
        });
        Layout { skel, pose, item, gait, ground, hidden, cloud, clones, tomoe, swing }
    }

    pub fn bounds(&self, lay: &Layout) -> (f32, f32, f32, f32) {
        let s = self.s;
        let pad = match lay.gait {
            Some(Gait::Horse) | Some(Gait::Car) => 75.0,
            _ => 70.0,
        } * s;
        let b = lay.skel.bbox();
        let mut r = (b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad.max(10.0 * s));
        let mut grow = |q: (f32, f32, f32, f32)| {
            r.0 = r.0.min(q.0);
            r.1 = r.1.min(q.1);
            r.2 = r.2.max(q.2);
            r.3 = r.3.max(q.3);
        };
        for c in &lay.clones {
            let b = c.skel.bbox();
            if (b.0 - self.x).abs() < 1200.0 && (b.1 - self.y).abs() < 900.0 {
                grow((b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad));
            }
        }
        for f in &self.fx {
            let fb = f.bounds();
            // ignore anything absurdly far away (keeps the overlay small)
            if (fb.0 - self.x).abs() < 1200.0 && (fb.1 - self.y).abs() < 900.0 {
                grow(fb);
            }
        }
        if let Some(c) = lay.cloud {
            grow((c.0 - 8.0, c.1 - 20.0 * self.ui, c.2 + 8.0, c.3 + 8.0));
        }
        if self.eye_trail.len() > 1 {
            for (p, _) in &self.eye_trail {
                grow((p.0 - 4.0, p.1 - 4.0, p.0 + 4.0, p.1 + 4.0));
            }
        }
        r
    }

    pub fn draw(&mut self, g: &mut Gfx, lay: &Layout, w: &World) {
        let look = w.look;
        let s = self.s;
        let t = self.t;
        let figfx = FigFx { t, vel: self.vel, tomoe: lay.tomoe };

        // afterimages
        if !lay.hidden {
            for (i, (sk, age)) in self.trail.iter().enumerate().skip(1) {
                g.alpha = (0.22 * (1.0 - age / 0.14) * (1.0 - i as f32 * 0.15)).max(0.0);
                rig::draw_figure(g, sk, look, &figfx);
            }
            g.alpha = 1.0;
        }
        // red eye streak
        if self.eye_trail.len() > 1 {
            let pts: Vec<P> = self.eye_trail.iter().map(|e| e.0).collect();
            g.taper(&pts, 2.6 * s, 0.6 * s, Col::rgb(235, 25, 25).alpha(0.85));
        }
        // mount behind rider
        let moving = matches!(self.act, Act::Walk { .. });
        if lay.gait == Some(Gait::Horse) {
            props::horse(g, lay.ground, self.facing, s, t, moving);
        }
        // clones
        let clone_fx = FigFx { vel: (0.0, 0.0), ..figfx };
        for c in &lay.clones {
            g.alpha = 0.9;
            rig::draw_figure(g, &c.skel, look, &clone_fx);
            props::draw_item(g, c.item, &c.skel, t, c.swing);
            if c.orb {
                let r = 9.0 * s;
                let h = c.skel.hand_f;
                props::chakra_orb(g, (h.0 + c.skel.fwd.0 * r * 0.6, h.1 + r * 0.2), r, t, look.aura);
            }
            g.alpha = 1.0;
        }
        if !lay.hidden {
            if matches!(self.act, Act::Leap { style: LeapStyle::Jet, .. }) {
                props::jetpack(g, &lay.skel, t);
            }
            rig::draw_figure(g, &lay.skel, look, &figfx);
            props::draw_item(g, lay.item, &lay.skel, t, lay.swing);
            if lay.gait == Some(Gait::Pogo) {
                props::pogo(g, &lay.skel, lay.ground);
            }
            if lay.gait == Some(Gait::Car) {
                let sp = if moving { 500.0 } else { 0.0 };
                props::car(g, lay.ground, self.facing, s, t, sp, self.car_col);
            }
            // jutsu in hand
            if let Act::Jutsu { k, .. } = self.act {
                let at = self.act_t;
                match k {
                    JutsuK::Orb if at >= 0.4 && at < 1.7 => {
                        let r = (11.0 * ((at - 0.4) / 0.8).min(1.0)).max(2.0) * s;
                        let h = lay.skel.hand_f;
                        let c = (h.0 + self.facing * r * 0.6, h.1 + r * 0.2);
                        props::chakra_orb(g, c, r, t, look.aura);
                    }
                    JutsuK::Shuriken if at >= 0.45 && at < 1.75 => {
                        let r = (26.0 * ((at - 0.45) / 0.9).min(1.0)).max(3.0) * s;
                        let h = lay.skel.hand_f;
                        props::wind_shuriken(g, (h.0, h.1 - r * 0.9), r, t, look.aura);
                    }
                    _ => {}
                }
                if at < 0.6 && matches!(k, JutsuK::Orb | JutsuK::Shuriken | JutsuK::Clones | JutsuK::Fire) {
                    // chakra aura flicker while forming hand seals
                    let c = lerp_p(lay.skel.hip, lay.skel.neck, 0.5);
                    g.ring(c, (30.0 + 4.0 * (t * 30.0).sin()) * s, 2.0 * s, look.aura.alpha(0.5));
                }
            }
        }
        // effects
        for f in &self.fx {
            f.draw(g, t);
        }
        // cloud
        if let (Some(c), Some(b)) = (&self.cloud, lay.cloud) {
            let a = (c.t / 0.2).min(1.0).min(((c.dur - c.t) / 0.35).max(0.0));
            draw_cloud(g, b, &c.lines, c.think, (lay.skel.head.0, lay.skel.bbox().1 - 4.0), self.ui, a);
        }
        let hb = rig::hit_box(&lay.skel, look);
        self.hit = if lay.hidden { (0.0, 0.0, 0.0, 0.0) } else { hb };
    }
}

pub fn draw_cloud(g: &mut Gfx, b: (f32, f32, f32, f32), lines: &[String], think: bool, anchor: P, ui: f32, alpha: f32) {
    let (l, t, r, bt) = b;
    let h = bt - t;
    let w = r - l;
    let br = (9.0 * ui).min(h * 0.4);
    let outline = Col::rgb(40, 40, 50);
    let fill = Col::rgb(255, 255, 255);
    let cy = (t + bt) / 2.0;
    let mut circles: Vec<(P, f32)> = vec![((l + h / 2.0, cy), h / 2.0), ((r - h / 2.0, cy), h / 2.0)];
    let n = ((w - h) / (br * 1.5)).max(1.0) as i32;
    for i in 0..=n {
        let x = l + h / 2.0 + (w - h) * i as f32 / n as f32;
        circles.push(((x, t + br * 0.35), br));
        circles.push(((x + br * 0.6, bt - br * 0.35), br));
    }
    let body = (l + h / 2.0, t, r - h / 2.0, bt);
    // tail
    let tail_base = (anchor.0.clamp(l + h / 2.0, r - h / 2.0), bt);
    let pass = |g: &mut Gfx, e: f32, col: Col| {
        for (c, rad) in &circles {
            g.circle(*c, rad + e, col);
        }
        g.rect(body.0, body.1 - e, body.2 - body.0, body.3 - body.1 + 2.0 * e, col);
        if think {
            let d = (anchor.0 - tail_base.0, anchor.1 - tail_base.1);
            g.circle((tail_base.0 + d.0 * 0.35, tail_base.1 + d.1 * 0.35 + 4.0 * ui), 4.0 * ui + e, col);
            g.circle((tail_base.0 + d.0 * 0.7, tail_base.1 + d.1 * 0.7 + 2.0 * ui), 2.5 * ui + e, col);
        } else {
            let tip = (anchor.0, anchor.1.max(bt + 6.0 * ui));
            g.poly(&[(tail_base.0 - 8.0 * ui - e, bt - 4.0), (tip.0, tip.1 + e), (tail_base.0 + 8.0 * ui + e, bt - 4.0)], col);
        }
    };
    let fs = 13.5 * ui;
    let lh = fs * 1.32;
    let start = t + 9.0 * ui + lh / 2.0;
    let pad = 16.0 * ui;
    let area = (l - pad, t - pad, r + pad, anchor.1.max(bt) + pad);
    // draw the whole cloud in one layer so fading has no seams
    g.layer(area, alpha, |g| {
        pass(g, 2.0 * ui, outline);
        pass(g, 0.0, fill);
        for (i, line) in lines.iter().enumerate() {
            let col = if i == 0 { Col::rgb(20, 20, 30) } else { Col::rgb(70, 70, 85) };
            g.text((l + r) / 2.0, start + i as f32 * lh, fs, col, line, 0);
        }
    });
}
