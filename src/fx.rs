//! Short-lived visual effects in screen coordinates.

use crate::gfx::Gfx;
use crate::props;
use crate::util::*;

#[derive(Clone, Debug)]
pub enum Kind {
    Burst { word: String, col: Col },
    Poof,
    Dust,
    Zzz,
    Line { len: f32 },
    Star,
    Sweat,
    Exhaust,
    /// flies from (x,y) to target then sticks there
    Kunai { to: P, from: P, stick: f32 },
    /// thrown wind shuriken that detonates at target
    Shuriken { to: P, from: P, col: Col },
    /// expanding chakra sphere
    Boom { col: Col },
    Fire { vx: f32 },
    Log { vy: f32, ang: f32 },
    EyeFlash,
    Heart,
}

#[derive(Clone, Debug)]
pub struct Fx {
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub t: f32,
    pub dur: f32,
    pub s: f32,
    pub rot: f32,
    pub done: bool,
}

impl Fx {
    pub fn new(kind: Kind, p: P, dur: f32, s: f32) -> Fx {
        Fx { kind, x: p.0, y: p.1, vx: 0.0, vy: 0.0, t: 0.0, dur, s, rot: 0.0, done: false }
    }
    pub fn vel(mut self, vx: f32, vy: f32) -> Fx {
        self.vx = vx;
        self.vy = vy;
        self
    }
    pub fn rot(mut self, r: f32) -> Fx {
        self.rot = r;
        self
    }
    fn u(&self) -> f32 {
        (self.t / self.dur).clamp(0.0, 1.0)
    }

    /// Returns follow-up effects (e.g. a shuriken spawning its explosion).
    pub fn update(&mut self, dt: f32) -> Vec<Fx> {
        let mut spawn = vec![];
        self.t += dt;
        match &mut self.kind {
            Kind::Kunai { to, from, stick } => {
                let fly = 0.22;
                if self.t < fly {
                    let k = self.t / fly;
                    self.x = lerp(from.0, to.0, k);
                    self.y = lerp(from.1, to.1, k);
                } else {
                    if *stick == 0.0 {
                        spawn.push(Fx::new(Kind::Star, *to, 0.4, self.s));
                    }
                    *stick += dt;
                    self.x = to.0;
                    self.y = to.1;
                }
            }
            Kind::Shuriken { to, from, col } => {
                let fly = self.dur;
                let k = (self.t / fly).min(1.0);
                self.x = lerp(from.0, to.0, ease_io(k));
                self.y = lerp(from.1, to.1, ease_io(k)) - (k * std::f32::consts::PI).sin() * 25.0 * self.s;
                if self.t >= fly {
                    spawn.push(Fx::new(Kind::Boom { col: *col }, *to, 0.9, self.s));
                    spawn.push(Fx::new(Kind::Burst { word: "RASEN-BONK!".into(), col: Col::rgb(255, 230, 70) }, (to.0, to.1 - 45.0 * self.s), 1.1, self.s));
                }
            }
            Kind::Log { vy, .. } => {
                *vy += 1800.0 * dt;
                self.y += *vy * dt;
                self.x += self.vx * dt;
            }
            Kind::Fire { .. } | Kind::Exhaust | Kind::Dust | Kind::Sweat | Kind::Heart => {
                self.x += self.vx * dt;
                self.y += self.vy * dt;
                if matches!(self.kind, Kind::Sweat) {
                    self.vy += 600.0 * dt;
                }
                if matches!(self.kind, Kind::Heart | Kind::Exhaust) {
                    self.vy -= 20.0 * dt;
                }
            }
            Kind::Zzz => {
                self.x += (self.t * 3.0).sin() * 12.0 * dt * self.s;
                self.y -= 18.0 * dt * self.s;
            }
            Kind::Star => {
                self.rot += dt * 8.0;
                self.x += self.vx * dt;
                self.y += self.vy * dt;
            }
            _ => {}
        }
        if self.t >= self.dur {
            self.done = true;
        }
        spawn
    }

    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let r = match &self.kind {
            Kind::Burst { .. } => 55.0,
            Kind::Boom { .. } => 80.0,
            Kind::Shuriken { .. } => 40.0,
            Kind::EyeFlash => 50.0,
            Kind::Log { .. } => 25.0,
            Kind::Poof => 32.0,
            Kind::Line { len } => *len / self.s + 5.0,
            Kind::Fire { .. } => 22.0,
            _ => 16.0,
        } * self.s;
        (self.x - r, self.y - r, self.x + r, self.y + r)
    }

    pub fn draw(&self, g: &mut Gfx, t: f32) {
        let s = self.s;
        let u = self.u();
        let fade = (1.0 - u).clamp(0.0, 1.0);
        let p = (self.x, self.y);
        match &self.kind {
            Kind::Burst { word, col } => {
                let pop = if u < 0.15 { ease_out(u / 0.15) * 1.15 } else { 1.0 + 0.15 * (1.0 - ((u - 0.15) / 0.2).min(1.0)) };
                g.alpha = if u > 0.75 { (1.0 - u) / 0.25 } else { 1.0 };
                props::burst(g, (p.0, p.1 - u * 10.0 * s), 30.0 * s * pop, word, *col, self.rot);
                g.alpha = 1.0;
            }
            Kind::Poof => props::smoke_puff(g, p, 22.0 * s, u),
            Kind::Dust => g.circle(p, (3.0 + 6.0 * u) * s, Col::rgb(200, 195, 185).alpha(fade * 0.8)),
            Kind::Zzz => {
                let sz = (7.0 + 6.0 * u) * s;
                props::zzz(g, p, sz, Col::rgb(70, 90, 160).alpha(fade));
            }
            Kind::Line { len } => {
                let a = fade * 0.55;
                g.line(p, (p.0 + len, p.1), 1.6 * s, Col::rgb(120, 120, 130).alpha(a));
            }
            Kind::Star => props::star(g, p, 5.0 * s, self.rot, Col::rgb(255, 220, 40).alpha(fade)),
            Kind::Sweat => g.circle(p, 2.2 * s, Col::rgb(110, 190, 255).alpha(fade)),
            Kind::Exhaust => g.circle(p, (3.0 + 7.0 * u) * s, Col::rgb(150, 150, 150).alpha(fade * 0.6)),
            Kind::Kunai { to, from, stick } => {
                let ang = (to.1 - from.1).atan2(to.0 - from.0);
                g.alpha = if self.dur - self.t < 0.4 { ((self.dur - self.t) / 0.4).max(0.0) } else { 1.0 };
                let wob = if *stick > 0.0 { (stick * 40.0).sin() * 0.15 * (1.0 - stick * 3.0).max(0.0) } else { 0.0 };
                // tip at the point: draw kunai backwards from p
                let d = ((ang + wob).cos(), (ang + wob).sin());
                props::draw_kunai(g, (p.0 - d.0 * 16.0 * s, p.1 - d.1 * 16.0 * s), ang + wob, s);
                g.alpha = 1.0;
            }
            Kind::Shuriken { col, .. } => props::wind_shuriken(g, p, 26.0 * s, t, *col),
            Kind::Boom { col } => {
                let r = (14.0 + 60.0 * ease_out(u)) * s;
                g.circle(p, r, col.mix(WHITE, 0.5).alpha(fade * 0.55));
                g.circle(p, r * 0.7, WHITE.alpha(fade * 0.8));
                g.ring(p, r, 2.5 * s, col.alpha(fade));
                for k in 0..10 {
                    let a = k as f32 * 0.628 + u * 3.0;
                    g.line(
                        (p.0 + a.cos() * r * 0.8, p.1 + a.sin() * r * 0.8),
                        (p.0 + a.cos() * r * 1.15, p.1 + a.sin() * r * 1.15),
                        1.8 * s,
                        WHITE.alpha(fade),
                    );
                }
            }
            Kind::Fire { .. } => {
                g.alpha = fade;
                props::fireball(g, p, (5.0 + 12.0 * u) * s, t + self.rot);
                g.alpha = 1.0;
            }
            Kind::Log { ang, .. } => props::log(g, p, s, *ang + u * 2.0),
            Kind::EyeFlash => {
                let a = if u < 0.2 { u / 0.2 } else if u > 0.7 { (1.0 - u) / 0.3 } else { 1.0 };
                let r = 15.0 * s * (0.8 + 0.25 * ease_out((u * 3.0).min(1.0)));
                props::tomoe_eye(g, p, r, t, a);
            }
            Kind::Heart => {
                let r = 4.0 * s;
                let c = Col::rgb(255, 70, 110).alpha(fade);
                g.circle((p.0 - r * 0.5, p.1), r * 0.6, c);
                g.circle((p.0 + r * 0.5, p.1), r * 0.6, c);
                g.poly(&[(p.0 - r * 1.05, p.1 + 0.2 * r), (p.0 + r * 1.05, p.1 + 0.2 * r), (p.0, p.1 + r * 1.3)], c);
            }
        }
    }
}
