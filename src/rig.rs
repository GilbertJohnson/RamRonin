//! Stick-figure skeleton: a pose is a set of joint angles; `solve` turns it
//! into screen points (forward kinematics) and `draw_figure` paints it with a
//! skin. Every animation in the app is just a function that returns a Pose.
//!
//! Angle conventions (degrees, "facing right" space):
//!  * torso/head: 0 = upright, + leans forward
//!  * limbs: 0 = pointing straight down, + swings forward (90 = horizontal
//!    forward, 180 = straight up). Angles are absolute, not relative.
//!  * rot: whole-body rotation, + = forward (front-flip direction).

use crate::gfx::Gfx;
use crate::skins::{EyeStyle, Gait, HeadStyle, Look};
use crate::util::*;

pub const TH: f32 = 15.0;
pub const SH: f32 = 15.0;
pub const TO: f32 = 24.0;
pub const NK: f32 = 3.0;
pub const HR: f32 = 8.5;
pub const UA: f32 = 13.0;
pub const FA: f32 = 13.0;
pub const HEIGHT: f32 = TH + SH + TO + NK + HR * 2.0;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Anchor {
    /// Lowest foot touches the ground.
    #[default]
    Feet,
    /// Hip sits this many units above the root (sitting, riding).
    Hip(f32),
    /// Hip at standing height; no ground contact (air, flips).
    Free,
    /// Lowest point of the whole body touches the ground (lying, handstand).
    Lowest,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Expr {
    #[default]
    Normal,
    Grin,
    Crazy,
    Sleep,
    Ko,
    Shock,
    Angry,
    Focus,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    pub torso: f32,
    pub head: f32,
    pub ua_f: f32,
    pub fa_f: f32,
    pub ua_b: f32,
    pub fa_b: f32,
    pub th_f: f32,
    pub sh_f: f32,
    pub th_b: f32,
    pub sh_b: f32,
    pub rot: f32,
    pub dx: f32,
    pub dy: f32,
    pub anchor: Anchor,
    pub expr: Expr,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Skel {
    pub hip: P,
    pub knee_f: P,
    pub foot_f: P,
    pub knee_b: P,
    pub foot_b: P,
    pub neck: P,
    pub shoulder: P,
    pub elbow_f: P,
    pub hand_f: P,
    pub elbow_b: P,
    pub hand_b: P,
    pub head: P,
    pub head_r: f32,
    /// unit vector pointing "up" out of the top of the head
    pub up: P,
    /// unit vector pointing where the face looks
    pub fwd: P,
    pub facing: f32,
    pub s: f32,
    pub expr: Expr,
}

impl Skel {
    pub fn points(&self) -> [P; 12] {
        [
            self.hip, self.knee_f, self.foot_f, self.knee_b, self.foot_b, self.neck, self.shoulder, self.elbow_f,
            self.hand_f, self.elbow_b, self.hand_b, self.head,
        ]
    }
    pub fn bbox(&self) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in self.points() {
            b.0 = b.0.min(p.0);
            b.1 = b.1.min(p.1);
            b.2 = b.2.max(p.0);
            b.3 = b.3.max(p.1);
        }
        let r = self.head_r;
        (b.0 - r, b.1 - r, b.2 + r, b.3 + r)
    }
    /// Unit direction of the front forearm (for holding props).
    pub fn fore_dir(&self) -> P {
        let (dx, dy) = (self.hand_f.0 - self.elbow_f.0, self.hand_f.1 - self.elbow_f.1);
        let l = (dx * dx + dy * dy).sqrt().max(0.001);
        (dx / l, dy / l)
    }
    pub fn translate(&mut self, dx: f32, dy: f32) {
        for p in [
            &mut self.hip, &mut self.knee_f, &mut self.foot_f, &mut self.knee_b, &mut self.foot_b, &mut self.neck,
            &mut self.shoulder, &mut self.elbow_f, &mut self.hand_f, &mut self.elbow_b, &mut self.hand_b, &mut self.head,
        ] {
            p.0 += dx;
            p.1 += dy;
        }
    }
}

fn down(a: f32, l: f32) -> P {
    let r = rad(a);
    (l * r.sin(), l * r.cos())
}
fn up(a: f32, l: f32) -> P {
    let r = rad(a);
    (l * r.sin(), -l * r.cos())
}
fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

/// Forward kinematics. `s` = pixels per unit, `root` = ground point in screen px.
pub fn solve(p: &Pose, s: f32, facing: f32, root: P) -> Skel {
    let hip = (0.0, 0.0);
    let knee_f = down(p.th_f, TH);
    let foot_f = add(knee_f, down(p.sh_f, SH));
    let knee_b = down(p.th_b, TH);
    let foot_b = add(knee_b, down(p.sh_b, SH));
    let neck = up(p.torso, TO);
    let shoulder = lerp_p(hip, neck, 0.92);
    let head = add(neck, up(p.torso + p.head, NK + HR));
    let elbow_f = add(shoulder, down(p.ua_f, UA));
    let hand_f = add(elbow_f, down(p.fa_f, FA));
    let elbow_b = add(shoulder, down(p.ua_b, UA));
    let hand_b = add(elbow_b, down(p.fa_b, FA));
    let mut pts = [hip, knee_f, foot_f, knee_b, foot_b, neck, shoulder, elbow_f, hand_f, elbow_b, hand_b, head];

    let head_ang = p.torso + p.head + p.rot;
    if p.rot != 0.0 {
        let c = lerp_p(hip, neck, 0.4);
        let (sn, cs) = rad(p.rot).sin_cos();
        for q in pts.iter_mut() {
            let (x, y) = (q.0 - c.0, q.1 - c.1);
            *q = (c.0 + x * cs - y * sn, c.1 + x * sn + y * cs);
        }
    }
    // anchor
    let shift_y = match p.anchor {
        Anchor::Feet => -pts[2].1.max(pts[4].1),
        Anchor::Hip(h) => -h - pts[0].1,
        Anchor::Free => -(TH + SH) - pts[0].1 + 0.0,
        Anchor::Lowest => {
            let mut m = pts.iter().map(|q| q.1).fold(f32::MIN, f32::max);
            m = m.max(pts[11].1 + HR);
            -m
        }
    };
    let (dx, dy) = (p.dx, p.dy + shift_y);
    for q in pts.iter_mut() {
        *q = (root.0 + (q.0 + dx) * s * facing, root.1 + (q.1 + dy) * s);
    }
    let r = rad(head_ang);
    // local (facing right): up = (sin a, -cos a), forward = (cos a, sin a); mirror x.
    let upv = (r.sin() * facing, -r.cos());
    let fwd = (r.cos() * facing, r.sin());
    Skel {
        hip: pts[0],
        knee_f: pts[1],
        foot_f: pts[2],
        knee_b: pts[3],
        foot_b: pts[4],
        neck: pts[5],
        shoulder: pts[6],
        elbow_f: pts[7],
        hand_f: pts[8],
        elbow_b: pts[9],
        hand_b: pts[10],
        head: pts[11],
        head_r: HR * s,
        up: upv,
        fwd,
        facing,
        s,
        expr: p.expr,
    }
}

// ---------------------------------------------------------------------------
// Pose library
// ---------------------------------------------------------------------------

pub mod poses {
    use super::*;
    use std::f32::consts::{PI, TAU};

    fn sn(x: f32) -> f32 {
        x.sin()
    }

    /// Bouncy fighter stance, fists up.
    pub fn stance(t: f32) -> Pose {
        Pose {
            torso: 8.0,
            head: -6.0,
            ua_f: 35.0,
            fa_f: 150.0,
            ua_b: 22.0,
            fa_b: 138.0,
            th_f: 22.0,
            sh_f: 6.0,
            th_b: -18.0,
            sh_b: -26.0,
            dy: -1.6 * (t * 6.0).sin().abs(),
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn casual(t: f32) -> Pose {
        Pose {
            torso: 2.0 + 1.5 * sn(t * 2.0),
            head: 2.0 * sn(t * 0.7),
            ua_f: 8.0,
            fa_f: 14.0,
            ua_b: -7.0,
            fa_b: -2.0,
            th_f: 6.0,
            sh_f: 4.0,
            th_b: -6.0,
            sh_b: -6.0,
            ..Default::default()
        }
    }

    pub fn scratch(t: f32) -> Pose {
        Pose { ua_f: 150.0, fa_f: 250.0 + 12.0 * sn(t * 28.0), head: 8.0, expr: Expr::Normal, ..casual(t) }
    }

    pub fn walk(p: f32) -> Pose {
        let s = sn(p);
        let c = p.cos();
        Pose {
            torso: 6.0,
            head: -2.0,
            th_f: 26.0 * s,
            sh_f: 26.0 * s - 12.0 - 26.0 * c.max(0.0),
            th_b: -26.0 * s,
            sh_b: -26.0 * s - 12.0 - 26.0 * (-c).max(0.0),
            ua_f: -22.0 * s,
            fa_f: -22.0 * s + 22.0,
            ua_b: 22.0 * s,
            fa_b: 22.0 * s + 22.0,
            ..Default::default()
        }
    }

    pub fn run(p: f32) -> Pose {
        let s = sn(p);
        let c = p.cos();
        Pose {
            torso: 18.0,
            head: -10.0,
            th_f: 48.0 * s + 10.0,
            sh_f: 48.0 * s - 30.0 - 50.0 * c.max(0.0),
            th_b: -48.0 * s + 10.0,
            sh_b: -48.0 * s - 30.0 - 50.0 * (-c).max(0.0),
            ua_f: -55.0 * s + 10.0,
            fa_f: -55.0 * s + 95.0,
            ua_b: 55.0 * s + 10.0,
            fa_b: 55.0 * s + 95.0,
            dy: -2.5 * (2.0 * p).sin().abs(),
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    /// The arms-back ninja sprint.
    pub fn ninja_run(p: f32) -> Pose {
        let s = sn(p);
        let c = p.cos();
        Pose {
            torso: 42.0,
            head: -30.0,
            th_f: 55.0 * s + 15.0,
            sh_f: 55.0 * s - 35.0 - 55.0 * c.max(0.0),
            th_b: -55.0 * s + 15.0,
            sh_b: -55.0 * s - 35.0 - 55.0 * (-c).max(0.0),
            ua_f: -62.0 + 4.0 * s,
            fa_f: -52.0,
            ua_b: -72.0 - 4.0 * s,
            fa_b: -60.0,
            dy: -3.0 * (2.0 * p).sin().abs(),
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    /// The comedy escape run: upright and leaning back, head thrown back,
    /// mouth open, knees pumping high, both arms flailing wildly.
    pub fn panic_run(p: f32) -> Pose {
        let s = sn(p);
        let c = p.cos();
        let w = (2.0 * p).sin();
        Pose {
            torso: -8.0 + 4.0 * w,
            head: -14.0 + 7.0 * (2.0 * p).cos(),
            th_f: 52.0 * s + 16.0,
            sh_f: 52.0 * s - 28.0 - 60.0 * c.max(0.0),
            th_b: -52.0 * s + 16.0,
            sh_b: -52.0 * s - 28.0 - 60.0 * (-c).max(0.0),
            // arms whirl: big swings with loose, flapping forearms
            ua_f: 75.0 * s + 45.0,
            fa_f: 75.0 * s + 45.0 + 70.0 + 25.0 * (2.0 * p + 1.0).sin(),
            ua_b: -75.0 * s + 45.0,
            fa_b: -75.0 * s + 45.0 + 70.0 + 25.0 * (2.0 * p + 2.5).sin(),
            dy: -4.0 * w.abs(),
            expr: Expr::Shock,
            ..Default::default()
        }
    }

    pub fn sneak(p: f32) -> Pose {
        let mut w = walk(p);
        w.torso = 25.0;
        w.head = -15.0;
        w.ua_f = 60.0;
        w.fa_f = 140.0;
        w.ua_b = 45.0;
        w.fa_b = 130.0;
        w.th_f += 15.0;
        w.th_b += 15.0;
        w.expr = Expr::Grin;
        w
    }

    /// Legs dangling over a window's top edge.
    pub fn sit(t: f32) -> Pose {
        Pose {
            torso: 4.0 + 2.0 * sn(t * 1.3),
            head: 4.0 * sn(t * 0.6),
            th_f: 86.0,
            sh_f: 8.0 + 22.0 * sn(t * 3.0),
            th_b: 82.0,
            sh_b: 8.0 - 22.0 * sn(t * 3.0 + 0.6),
            ua_f: 20.0,
            fa_f: 8.0,
            ua_b: -18.0,
            fa_b: -10.0,
            anchor: Anchor::Hip(1.0),
            expr: Expr::Normal,
            ..Default::default()
        }
    }

    pub fn sleep(t: f32) -> Pose {
        Pose {
            rot: -90.0,
            torso: 2.0 * sn(t * 1.6),
            head: 12.0,
            ua_f: 165.0,
            fa_f: 250.0,
            ua_b: 172.0,
            fa_b: 255.0,
            th_f: 38.0,
            sh_f: -32.0,
            th_b: 3.0,
            sh_b: 3.0,
            anchor: Anchor::Lowest,
            expr: Expr::Sleep,
            ..Default::default()
        }
    }

    pub fn meditate(t: f32) -> Pose {
        Pose {
            torso: 0.0,
            th_f: 78.0,
            sh_f: -82.0,
            th_b: 72.0,
            sh_b: -88.0,
            ua_f: 22.0,
            fa_f: 72.0,
            ua_b: 12.0,
            fa_b: 62.0,
            anchor: Anchor::Hip(5.0 + 2.5 * sn(t * 2.0)),
            expr: Expr::Sleep,
            ..Default::default()
        }
    }

    pub fn tuck(rot: f32) -> Pose {
        Pose {
            torso: 15.0,
            th_f: 112.0,
            sh_f: -18.0,
            th_b: 102.0,
            sh_b: -28.0,
            ua_f: 62.0,
            fa_f: 122.0,
            ua_b: 52.0,
            fa_b: 112.0,
            rot,
            anchor: Anchor::Free,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn hero(t: f32) -> Pose {
        Pose {
            torso: 20.0,
            ua_f: 168.0,
            fa_f: 175.0,
            ua_b: -50.0,
            fa_b: -40.0,
            th_f: 22.0,
            sh_f: -22.0 + 6.0 * sn(t * 20.0),
            th_b: -25.0,
            sh_b: -62.0,
            anchor: Anchor::Free,
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn superman(t: f32) -> Pose {
        Pose {
            torso: 72.0,
            head: -40.0,
            ua_f: 162.0,
            fa_f: 166.0,
            ua_b: 150.0,
            fa_b: 158.0,
            th_f: -62.0 + 5.0 * sn(t * 14.0),
            sh_f: -72.0,
            th_b: -56.0 - 5.0 * sn(t * 14.0),
            sh_b: -78.0,
            anchor: Anchor::Free,
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn fall(t: f32) -> Pose {
        Pose {
            torso: -8.0 + 6.0 * sn(t * 9.0),
            ua_f: 150.0 + 35.0 * sn(t * 15.0),
            fa_f: 170.0 + 40.0 * sn(t * 15.0 + 1.0),
            ua_b: 160.0 + 35.0 * sn(t * 15.0 + 2.0),
            fa_b: 180.0 + 40.0 * sn(t * 15.0 + 3.0),
            th_f: 40.0 * sn(t * 12.0) + 20.0,
            sh_f: 40.0 * sn(t * 12.0) - 30.0,
            th_b: -40.0 * sn(t * 12.0) + 10.0,
            sh_b: -40.0 * sn(t * 12.0) - 40.0,
            anchor: Anchor::Free,
            expr: Expr::Shock,
            ..Default::default()
        }
    }

    pub fn dragged(t: f32) -> Pose {
        Pose {
            torso: 4.0 * sn(t * 3.0),
            head: 10.0,
            ua_f: 30.0 + 40.0 * sn(t * 11.0),
            fa_f: 50.0 + 50.0 * sn(t * 11.0 + 0.7),
            ua_b: -20.0 + 40.0 * sn(t * 11.0 + 2.0),
            fa_b: 10.0 + 50.0 * sn(t * 11.0 + 2.5),
            th_f: 45.0 * sn(t * 14.0) + 10.0,
            sh_f: 45.0 * sn(t * 14.0) - 35.0,
            th_b: -45.0 * sn(t * 14.0) + 10.0,
            sh_b: -45.0 * sn(t * 14.0) - 35.0,
            anchor: Anchor::Free,
            expr: Expr::Angry,
            ..Default::default()
        }
    }

    pub fn land(u: f32) -> Pose {
        let k = 1.0 - u.clamp(0.0, 1.0);
        Pose {
            torso: 32.0 * k,
            head: -20.0 * k,
            th_f: 72.0 * k,
            sh_f: -42.0 * k,
            th_b: 52.0 * k - 8.0,
            sh_b: -62.0 * k - 8.0,
            ua_f: 70.0 * k + 10.0,
            fa_f: 90.0 * k + 10.0,
            ua_b: -40.0 * k,
            fa_b: -10.0 * k,
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn stumble(t: f32) -> Pose {
        let a = t * 760.0;
        Pose {
            torso: -14.0 + 22.0 * sn(t * 8.0),
            head: 10.0 * sn(t * 11.0),
            ua_f: a,
            fa_f: a + 20.0,
            ua_b: a + 180.0,
            fa_b: a + 200.0,
            th_f: 18.0 + 14.0 * sn(t * 9.0),
            sh_f: 6.0,
            th_b: -18.0 + 14.0 * sn(t * 9.0 + 2.0),
            sh_b: -10.0,
            expr: Expr::Shock,
            ..Default::default()
        }
    }

    /// Weapon swing; `p` is the phase of one swing cycle in [0,1).
    pub fn swing(p: f32) -> Pose {
        let (ua, fa, torso) = if p < 0.45 {
            let e = ease_io(p / 0.45);
            (lerp(60.0, 205.0, e), lerp(80.0, 245.0, e), lerp(18.0, -12.0, e))
        } else if p < 0.6 {
            let e = ease_out((p - 0.45) / 0.15);
            (lerp(205.0, 62.0, e), lerp(245.0, 62.0, e), lerp(-12.0, 34.0, e))
        } else {
            let e = ease_io((p - 0.6) / 0.4);
            (62.0, lerp(62.0, 80.0, e), lerp(34.0, 18.0, e))
        };
        Pose {
            torso,
            head: -torso * 0.4,
            ua_f: ua,
            fa_f: fa,
            ua_b: -30.0,
            fa_b: -8.0,
            th_f: 36.0,
            sh_f: 12.0,
            th_b: -30.0,
            sh_b: -42.0,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn punches(p: f32) -> Pose {
        let k1 = (p * TAU).sin().max(0.0);
        let k2 = (-(p * TAU).sin()).max(0.0);
        Pose {
            torso: 14.0 + 8.0 * (k1 + k2),
            head: -8.0,
            ua_f: lerp(38.0, 92.0, k1),
            fa_f: lerp(155.0, 90.0, k1),
            ua_b: lerp(30.0, 90.0, k2),
            fa_b: lerp(145.0, 90.0, k2),
            th_f: 30.0,
            sh_f: 10.0,
            th_b: -26.0,
            sh_b: -36.0,
            dy: -1.5 * (p * TAU * 2.0).sin().abs(),
            expr: Expr::Angry,
            ..Default::default()
        }
    }

    pub fn dance(t: f32, chaos: f32) -> Pose {
        let a = (t * 4.0).sin();
        let wild = |f: f32, ph: f32| (t * f + ph).sin() * chaos;
        Pose {
            torso: 10.0 * a + 15.0 * wild(5.3, 0.0),
            head: 15.0 * (t * 8.0).sin(),
            ua_f: if a > 0.0 { 160.0 } else { 40.0 } + 50.0 * wild(7.1, 1.0),
            fa_f: if a > 0.0 { 185.0 } else { 120.0 } + 60.0 * wild(6.3, 2.0),
            ua_b: 70.0 + 60.0 * (t * 4.0 + PI).sin() + 50.0 * wild(4.7, 3.0),
            fa_b: 120.0 + 50.0 * wild(9.1, 0.5),
            th_f: 22.0 * (t * 8.0).sin() + 10.0,
            sh_f: 22.0 * (t * 8.0).sin() - 18.0,
            th_b: -22.0 * (t * 8.0).sin() - 5.0,
            sh_b: -22.0 * (t * 8.0).sin() - 25.0,
            rot: 6.0 * a,
            dy: -4.0 * (t * 8.0).sin().abs(),
            expr: if chaos > 0.6 { Expr::Crazy } else { Expr::Grin },
            ..Default::default()
        }
    }

    pub fn handstand(t: f32) -> Pose {
        Pose {
            rot: 180.0,
            ua_f: 178.0,
            fa_f: 178.0,
            ua_b: 184.0,
            fa_b: 184.0,
            th_f: 22.0 * (t * 5.0).sin() + 14.0,
            sh_f: 22.0 * (t * 5.0).sin() + 4.0,
            th_b: -22.0 * (t * 5.0).sin() - 14.0,
            sh_b: -22.0 * (t * 5.0).sin() - 30.0,
            anchor: Anchor::Lowest,
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn headspin(t: f32) -> Pose {
        Pose {
            rot: 180.0,
            ua_f: 90.0 + 20.0 * (t * 20.0).sin(),
            fa_f: 90.0,
            ua_b: -90.0,
            fa_b: -90.0,
            th_f: 55.0,
            sh_f: 55.0,
            th_b: -55.0,
            sh_b: -55.0,
            anchor: Anchor::Lowest,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn spread(rot: f32, lift: f32) -> Pose {
        Pose {
            rot,
            ua_f: 165.0,
            fa_f: 175.0,
            ua_b: -165.0,
            fa_b: -175.0,
            th_f: 42.0,
            sh_f: 42.0,
            th_b: -42.0,
            sh_b: -42.0,
            dy: -lift,
            anchor: Anchor::Free,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn split_jump(u: f32) -> Pose {
        Pose {
            th_f: 88.0,
            sh_f: 88.0,
            th_b: -88.0,
            sh_b: -88.0,
            ua_f: 150.0,
            fa_f: 160.0,
            ua_b: -150.0,
            fa_b: -160.0,
            dy: -(u * PI).sin() * 36.0,
            anchor: Anchor::Free,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn shout(t: f32) -> Pose {
        Pose {
            torso: -6.0,
            head: -6.0,
            ua_f: 78.0,
            fa_f: 100.0,
            ua_b: -25.0,
            fa_b: 70.0,
            th_f: 14.0,
            sh_f: 6.0,
            th_b: -12.0,
            sh_b: -12.0,
            dy: -1.2 * (t * 14.0).sin().abs(),
            expr: Expr::Shock,
            ..Default::default()
        }
    }

    pub fn poke(t: f32) -> Pose {
        let j = (t * 12.0).sin();
        Pose {
            torso: 16.0 + 6.0 * j,
            head: -8.0,
            ua_f: 72.0 + 14.0 * j,
            fa_f: 86.0 + 20.0 * j,
            ua_b: -20.0,
            fa_b: 20.0,
            th_f: 28.0,
            sh_f: 10.0,
            th_b: -24.0,
            sh_b: -34.0,
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn taunt(t: f32) -> Pose {
        Pose { ua_f: 70.0, fa_f: 160.0 + 30.0 * (t * 10.0).sin(), expr: Expr::Grin, ..stance(t) }
    }

    /// Crouched, palm forward: charging a chakra orb.
    pub fn charge(t: f32) -> Pose {
        Pose {
            torso: 30.0,
            head: -18.0,
            ua_f: 55.0,
            fa_f: 72.0,
            ua_b: 72.0 + 10.0 * (t * 20.0).sin(),
            fa_b: 125.0 + 10.0 * (t * 20.0).cos(),
            th_f: 46.0,
            sh_f: 0.0,
            th_b: -40.0,
            sh_b: -52.0,
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    /// Arm raised: holding a giant spinning shuriken overhead.
    /// Climbing a wall that is in front of him (alternating reach / push).
    pub fn climb(p: f32) -> Pose {
        let s = p.sin();
        Pose {
            torso: 10.0,
            head: -14.0,
            ua_f: 150.0 + 26.0 * s,
            fa_f: 172.0 + 12.0 * s,
            ua_b: 150.0 - 26.0 * s,
            fa_b: 172.0 - 12.0 * s,
            th_f: 62.0 + 26.0 * s,
            sh_f: -8.0 + 18.0 * s,
            th_b: 62.0 - 26.0 * s,
            sh_b: -8.0 - 18.0 * s,
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    pub fn hold_up(t: f32) -> Pose {
        Pose {
            torso: -6.0,
            head: -8.0,
            ua_f: 176.0,
            fa_f: 182.0,
            ua_b: 92.0,
            fa_b: 92.0 + 6.0 * (t * 3.0).sin(),
            th_f: 26.0,
            sh_f: 10.0,
            th_b: -22.0,
            sh_b: -30.0,
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    pub fn throw(u: f32) -> Pose {
        let e = ease_out(u);
        Pose {
            torso: lerp(-10.0, 30.0, e),
            head: -12.0,
            ua_f: lerp(190.0, 70.0, e),
            fa_f: lerp(200.0, 70.0, e),
            ua_b: lerp(80.0, -40.0, e),
            fa_b: lerp(80.0, -20.0, e),
            th_f: 38.0,
            sh_f: 14.0,
            th_b: -30.0,
            sh_b: -44.0,
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    /// Rapid hand seals in front of the chest.
    pub fn signs(t: f32) -> Pose {
        let j = (t * 26.0).sin();
        Pose {
            torso: 2.0,
            head: -4.0,
            ua_f: 42.0 + 6.0 * j,
            fa_f: 152.0 + 14.0 * j,
            ua_b: 46.0 - 6.0 * j,
            fa_b: 146.0 - 14.0 * j,
            th_f: 16.0,
            sh_f: 6.0,
            th_b: -14.0,
            sh_b: -18.0,
            expr: Expr::Focus,
            ..Default::default()
        }
    }

    pub fn ride(t: f32) -> Pose {
        Pose {
            torso: 10.0 + 4.0 * (t * 12.0).sin(),
            head: -6.0,
            th_f: 52.0,
            sh_f: -8.0,
            th_b: 46.0,
            sh_b: -14.0,
            ua_f: 55.0,
            fa_f: 92.0,
            ua_b: 170.0,
            fa_b: 170.0 + 45.0 * (t * 10.0).sin(),
            anchor: Anchor::Hip(0.0),
            expr: Expr::Crazy,
            ..Default::default()
        }
    }

    pub fn drive(t: f32) -> Pose {
        Pose {
            torso: -8.0,
            head: -4.0 + 4.0 * (t * 9.0).sin(),
            th_f: 86.0,
            sh_f: -8.0,
            th_b: 84.0,
            sh_b: -10.0,
            ua_f: 70.0,
            fa_f: 96.0 + 6.0 * (t * 15.0).sin(),
            ua_b: 62.0,
            fa_b: 102.0 - 6.0 * (t * 15.0).sin(),
            anchor: Anchor::Hip(0.0),
            expr: Expr::Grin,
            ..Default::default()
        }
    }

    pub fn pogo() -> Pose {
        Pose {
            torso: 6.0,
            th_f: 26.0,
            sh_f: -6.0,
            th_b: 20.0,
            sh_b: -12.0,
            ua_f: 30.0,
            fa_f: 82.0,
            ua_b: 26.0,
            fa_b: 76.0,
            expr: Expr::Crazy,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Figure painter
// ---------------------------------------------------------------------------

pub struct FigFx {
    pub t: f32,
    /// velocity (px/s) – drives headband tails & scarf
    pub vel: P,
    /// force the red tomoe eye (Shinobi edition moments)
    pub tomoe: bool,
}

fn ribbon(sk: &Skel, from: P, len: f32, width: f32, col: Col, fx: &FigFx, phase: f32, g: &mut Gfx) {
    let speed = (fx.vel.0 * fx.vel.0 + fx.vel.1 * fx.vel.1).sqrt();
    let wind = (speed / 320.0).clamp(0.0, 1.0);
    // blow opposite to motion; when still, hang behind the head
    let back = if fx.vel.0.abs() > 20.0 { -fx.vel.0.signum() } else { -sk.facing };
    let n = 7;
    let mut pts = vec![from];
    let mut p = from;
    let seg = len / n as f32;
    for i in 1..=n {
        let k = i as f32 / n as f32;
        // angle measured from horizontal "back" direction; + = downward
        let droop = lerp(1.25, -0.12, wind) - (fx.vel.1 / 900.0).clamp(-0.8, 0.8);
        let wave = ((fx.t * (9.0 + 9.0 * wind)) - k * 4.0 + phase).sin() * (0.25 + 0.45 * wind) * k;
        let a = droop + wave;
        p = (p.0 + back * a.cos() * seg, p.1 + a.sin() * seg);
        pts.push(p);
    }
    g.taper(&pts, width, width * 0.55, col);
}

/// Convex hull (for the lungi, which wraps both thighs).
fn hull(mut pts: Vec<P>) -> Vec<P> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let cross = |o: P, a: P, b: P| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<P> = vec![];
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P> = vec![];
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Torso frame: unit vector hip->neck, and the "chest" normal (forward).
fn torso_frame(sk: &Skel) -> (P, P, f32) {
    let (dx, dy) = (sk.neck.0 - sk.hip.0, sk.neck.1 - sk.hip.1);
    let l = (dx * dx + dy * dy).sqrt().max(0.001);
    let t = (dx / l, dy / l);
    let n = (-t.1 * sk.facing, t.0 * sk.facing);
    (t, n, l)
}

fn off(p: P, v: P, k: f32) -> P {
    (p.0 + v.0 * k, p.1 + v.1 * k)
}

/// Shirt on the torso (+ vest at the collar) and short sleeves on one arm.
fn draw_shirt(g: &mut Gfx, sk: &Skel, look: &Look) {
    let Some(sc) = look.shirt else { return };
    let s = sk.s;
    let (t, n, _) = torso_frame(sk);
    let top = off(sk.neck, t, -1.5 * s);
    let bot = off(sk.hip, t, 1.5 * s);
    let pts = [off(top, n, -4.6 * s), off(top, n, 4.4 * s), off(bot, n, 4.0 * s), off(bot, n, -4.0 * s)];
    g.poly(&pts, sc);
    g.poly_stroke(&pts, 0.9 * s, sc.mix(BLACK, 0.35));
    if let Some(vc) = look.vest {
        // open collar: vest V on the chest
        let v = [off(top, n, -0.6 * s), off(top, n, 4.4 * s), off(lerp_p(top, bot, 0.55), n, 3.4 * s), off(lerp_p(top, bot, 0.2), n, 0.6 * s)];
        g.poly(&v, vc);
    }
    // buttons
    for k in [0.55, 0.75] {
        g.circle(off(lerp_p(top, bot, k), n, 3.3 * s), 0.55 * s, sc.mix(WHITE, 0.6));
    }
}

fn draw_sleeve(g: &mut Gfx, sk: &Skel, look: &Look, front: bool) {
    let Some(sc) = look.shirt else { return };
    let s = sk.s;
    let elbow = if front { sk.elbow_f } else { sk.elbow_b };
    let end = lerp_p(sk.shoulder, elbow, 0.62);
    let c = if front { sc } else { sc.mix(BLACK, 0.18) };
    g.line(sk.shoulder, end, 5.4 * s, c.mix(BLACK, 0.3));
    g.line(sk.shoulder, end, 4.2 * s, c);
}

/// Lungi folded up above the knees, wrapping both thighs.
fn draw_lungi(g: &mut Gfx, sk: &Skel, look: &Look) {
    let Some(lc) = look.lungi else { return };
    let s = sk.s;
    let (t, n, _) = torso_frame(sk);
    let waist = off(sk.hip, t, 3.0 * s);
    let mut pts = vec![off(waist, n, -4.2 * s), off(waist, n, 4.2 * s), off(sk.hip, n, -4.4 * s), off(sk.hip, n, 4.4 * s)];
    let mut hem = vec![];
    for knee in [sk.knee_f, sk.knee_b] {
        let k = lerp_p(sk.hip, knee, 0.82);
        let (dx, dy) = (knee.0 - sk.hip.0, knee.1 - sk.hip.1);
        let l = (dx * dx + dy * dy).sqrt().max(0.001);
        let side = (-dy / l * 3.6 * s, dx / l * 3.6 * s);
        pts.push(off(k, side, 1.0));
        pts.push(off(k, side, -1.0));
        hem.push(k);
    }
    let h = hull(pts);
    g.poly(&h, lc);
    g.poly_stroke(&h, 0.9 * s, lc.mix(BLACK, 0.4));
    if let Some(tc) = look.lungi_trim {
        // border stripe along the hem and a fold line at the waist
        g.line(hem[0], hem[1], 1.6 * s, tc);
        g.line(off(waist, n, -4.0 * s), off(waist, n, 4.0 * s), 1.4 * s, tc);
    }
}

fn draw_chain(g: &mut Gfx, sk: &Skel, look: &Look) {
    let Some(cc) = look.chain else { return };
    let s = sk.s;
    let (t, n, _) = torso_frame(sk);
    let low = off(lerp_p(sk.neck, sk.hip, 0.3), n, 3.2 * s);
    g.polyline(&[off(sk.neck, n, 0.2 * s), low, off(off(sk.neck, n, 4.0 * s), t, -s)], 1.1 * s, cc);
    g.circle(low, 1.3 * s, cc);
}

/// Long curly hair: the part behind the head and down the neck.
fn draw_curls_back(g: &mut Gfx, look: &Look, at: &dyn Fn(f32, f32) -> P, r: f32) {
    let Some(hc) = look.curls else { return };
    for &(a, b, k) in &[(-0.55, 0.75, 0.5), (-0.95, 0.3, 0.5), (-1.05, -0.25, 0.48), (-0.95, -0.8, 0.45), (-0.7, -1.2, 0.4), (-0.25, 0.95, 0.48)] {
        g.circle(at(a, b), r * k, hc);
    }
}

/// ... and the curly fringe on top of the head.
fn draw_curls_top(g: &mut Gfx, look: &Look, at: &dyn Fn(f32, f32) -> P, r: f32) {
    let Some(hc) = look.curls else { return };
    let rim = hc.mix(WHITE, 0.12);
    for (i, &(a, b)) in [(-0.85, 0.55), (-0.55, 0.85), (-0.15, 1.0), (0.25, 0.97), (0.6, 0.8), (0.8, 0.55)].iter().enumerate() {
        g.circle(at(a, b), r * 0.36, if i % 2 == 0 { hc } else { rim });
    }
}

fn draw_mustache(g: &mut Gfx, look: &Look, at: &dyn Fn(f32, f32) -> P, r: f32) {
    let Some(mc) = look.mustache else { return };
    if look.twirl {
        // grand twirled moustache
        g.polyline(&[at(0.62, -0.2), at(0.38, -0.34), at(0.16, -0.32), at(0.06, -0.18), at(0.16, -0.1)], r * 0.16, mc);
        g.polyline(&[at(0.62, -0.2), at(0.92, -0.34), at(1.18, -0.28), at(1.26, -0.12), at(1.14, -0.05)], r * 0.16, mc);
    } else {
        // thick, slightly drooping
        g.polyline(&[at(0.3, -0.3), at(0.45, -0.12), at(0.75, -0.08), at(1.02, -0.18), at(1.08, -0.36)], r * 0.22, mc);
    }
}

fn draw_crown(g: &mut Gfx, look: &Look, at: &dyn Fn(f32, f32) -> P, r: f32) {
    let Some(cc) = look.crown else { return };
    let pts = [at(-0.75, 0.62), at(-0.85, 1.55), at(-0.42, 1.12), at(0.0, 1.7), at(0.42, 1.12), at(0.85, 1.55), at(0.75, 0.62)];
    g.poly(&pts, cc);
    g.poly_stroke(&pts, r * 0.08, cc.mix(BLACK, 0.4));
    g.line(at(-0.78, 0.7), at(0.78, 0.7), r * 0.2, cc.mix(BLACK, 0.15));
    g.circle(at(0.0, 1.0), r * 0.17, Col::rgb(210, 20, 40));
    for x in [-0.85, 0.0, 0.85] {
        g.circle(at(x, if x == 0.0 { 1.72 } else { 1.57 }), r * 0.1, WHITE);
    }
}

/// Where a GIF character is drawn: (bottom-centre, height, rotation degrees).
fn sprite_place(sk: &Skel, look: &Look) -> (P, f32, f32) {
    let foot = (sk.hip.0, sk.foot_f.1.max(sk.foot_b.1));
    let height = HEIGHT * sk.s * look.sprite_scale;
    // follow the body only when it's really tipped over (flips, handstands)
    let ang = (sk.neck.0 - sk.hip.0).atan2(sk.hip.1 - sk.neck.1).to_degrees();
    let deg = if ang.abs() > 55.0 { ang } else { 0.0 };
    (foot, height, deg)
}

fn draw_sprite(g: &mut Gfx, sk: &Skel, look: &Look, fx: &FigFx, sp: &crate::sprite::Sprite) {
    let (foot, height, deg) = sprite_place(sk, look);
    let moving = (fx.vel.0 * fx.vel.0 + fx.vel.1 * fx.vel.1).sqrt() > 15.0;
    let (sp, frame) = match (&look.sprite_idle, moving) {
        (Some(idle), false) => (idle.as_ref(), idle.frame_at(fx.t)),
        (_, true) => (sp, sp.frame_at(fx.t)),
        (None, false) => (sp, &sp.frames[0].0),
    };
    let flip = (sk.facing < 0.0) != look.sprite_left;
    crate::sprite::draw(g, sp, frame, foot, height, flip, deg);
}

pub fn draw_figure(g: &mut Gfx, sk: &Skel, look: &Look, fx: &FigFx) {
    if let Some(sp) = &look.sprite {
        draw_sprite(g, sk, look, fx, sp);
        return;
    }
    let s = sk.s;
    let lw = look.lw * s / 1.0 * 0.9;
    let body = look.body;
    let back = body.mix(Col::rgb(110, 110, 120), 0.18);
    let r = sk.head_r;
    let (u, f) = (sk.up, sk.fwd);
    let at = |a: f32, b: f32| (sk.head.0 + f.0 * a * r + u.0 * b * r, sk.head.1 + f.1 * a * r + u.1 * b * r);

    // scarf (behind everything)
    if let Some(sc) = look.scarf {
        let knot = lerp_p(sk.neck, sk.head, 0.15);
        ribbon(sk, knot, 30.0 * s, 4.2 * s, sc, fx, 1.3, g);
        g.line((knot.0 - f.0 * 4.0 * s, knot.1), (knot.0 + f.0 * 4.0 * s, knot.1), 5.0 * s, sc);
    }
    // afro behind head
    if let Some(ac) = look.afro {
        let c = at(-0.15, 0.35);
        for i in 0..9 {
            let a = i as f32 / 9.0 * std::f32::consts::TAU;
            g.circle((c.0 + a.cos() * r * 0.9, c.1 + a.sin() * r * 0.8), r * 0.75, ac);
        }
        g.circle(c, r * 1.25, ac);
    }
    draw_curls_back(g, look, &at, r);

    // back limbs
    g.polyline(&[sk.shoulder, sk.elbow_b, sk.hand_b], lw, back);
    draw_sleeve(g, sk, look, false);
    g.polyline(&[sk.hip, sk.knee_b, sk.foot_b], lw, back);
    // torso
    g.line(sk.hip, sk.neck, lw * 1.05, body);
    draw_shirt(g, sk, look);
    // front leg
    g.polyline(&[sk.hip, sk.knee_f, sk.foot_f], lw, body);
    draw_lungi(g, sk, look);
    draw_chain(g, sk, look);

    // headband tails (behind head)
    if let (Some(bc), true) = (look.band, look.tails) {
        let knot = at(-0.95, 0.25);
        ribbon(sk, knot, 30.0 * s, 3.4 * s, bc, fx, 0.0, g);
        ribbon(sk, knot, 25.0 * s, 3.0 * s, bc.mix(BLACK, 0.12), fx, 1.7, g);
    }

    // head
    match look.head {
        HeadStyle::Square => {
            let c = sk.head;
            let pts = [at(-0.95, 0.95), at(0.95, 0.95), at(0.95, -0.95), at(-0.95, -0.95)];
            g.poly(&pts, look.head_fill);
            g.poly_stroke(&pts, lw * 0.6, body);
            let _ = c;
        }
        _ => {
            g.circle(sk.head, r, look.head_fill);
            if look.head_fill != body {
                g.ring(sk.head, r, lw * 0.55, body);
            }
        }
    }
    draw_curls_top(g, look, &at, r);
    draw_crown(g, look, &at, r);
    if look.head == HeadStyle::Masked {
        // eye slit band
        g.line(at(-0.1, 0.12), at(0.95, 0.12), r * 0.42, Col::rgb(235, 225, 210));
    }
    if let Some(bc) = look.band {
        g.line(at(-0.98, 0.3), at(0.98, 0.42), r * 0.42, bc);
        // metal plate
        g.line(at(0.35, 0.42), at(0.85, 0.46), r * 0.28, Col::rgb(170, 178, 190));
    }
    if let Some(cc) = look.cap {
        let pts = [at(-1.0, 0.25), at(-0.8, 0.85), at(0.0, 1.12), at(0.8, 0.85), at(1.0, 0.25)];
        g.poly(&pts, cc);
        g.line(at(0.6, 0.28), at(1.55, 0.2), r * 0.25, cc);
    }
    if look.antenna {
        let base = at(0.0, 0.95);
        let tip = at(-0.2, 1.9);
        g.line(base, tip, lw * 0.45, body);
        let blink = ((fx.t * 3.0).sin() > 0.0) as u8 as f32;
        g.circle(tip, r * 0.28, Col::rgb(255, 60, 60).mix(Col::rgb(255, 220, 60), blink));
    }
    if let Some(bc) = look.beard {
        let pts = [at(-0.1, -0.2), at(1.0, -0.15), at(0.85, -1.2), at(0.3, -1.9), at(-0.2, -0.9)];
        g.poly(&pts, bc);
    }

    // face
    let expr = sk.expr;
    let eye_style = if fx.tomoe && look.eye != EyeStyle::Visor && look.gait != Gait::Panic { EyeStyle::Tomoe } else { look.eye };
    let eye = at(0.48, 0.12);
    let light_face = look.head_fill.luma() > 0.55;
    let ink = if light_face { BLACK } else { WHITE };
    match expr {
        Expr::Sleep => {
            g.line(at(0.25, 0.08), at(0.72, 0.1), r * 0.16, ink);
        }
        Expr::Ko => {
            g.line(at(0.3, 0.3), at(0.7, -0.1), r * 0.14, ink);
            g.line(at(0.3, -0.1), at(0.7, 0.3), r * 0.14, ink);
        }
        _ => match eye_style {
            EyeStyle::Visor => {
                g.line(at(0.0, 0.15), at(0.98, 0.15), r * 0.38, look.eye_col);
                let scan = (fx.t * 4.0).sin() * 0.35 + 0.5;
                g.circle(at(scan, 0.15), r * 0.16, WHITE);
            }
            EyeStyle::Shades => {
                g.line(at(-0.4, 0.2), at(0.98, 0.2), r * 0.16, BLACK);
                g.ellipse(at(0.55, 0.08), r * 0.42, r * 0.26, 0.0, BLACK);
            }
            EyeStyle::Tomoe => {
                let er = r * 0.36;
                g.circle(eye, er, Col::rgb(225, 20, 20));
                g.ring(eye, er, r * 0.06, BLACK);
                g.circle(eye, er * 0.3, BLACK);
                for i in 0..3 {
                    let a = fx.t * 6.0 + i as f32 * 2.094;
                    g.circle((eye.0 + a.cos() * er * 0.62, eye.1 + a.sin() * er * 0.62), er * 0.17, BLACK);
                }
            }
            EyeStyle::Slit => {
                g.line(at(0.35, 0.12), at(0.8, 0.12), r * 0.14, BLACK);
                if matches!(expr, Expr::Angry | Expr::Focus) {
                    g.circle(at(0.62, 0.12), r * 0.1, RED);
                }
            }
            EyeStyle::Dot => {
                let (er, pr) = match expr {
                    Expr::Shock => (r * 0.32, r * 0.08),
                    Expr::Crazy => (r * 0.3, r * 0.1),
                    _ => (r * 0.24, r * 0.12),
                };
                let ec = if light_face { WHITE } else { look.eye_col };
                g.circle(eye, er, ec);
                let jitter = if expr == Expr::Crazy { ((fx.t * 37.0).sin() * 0.25, (fx.t * 29.0).cos() * 0.2) } else { (0.0, 0.0) };
                let pp = (eye.0 + f.0 * er * (0.35 + jitter.0), eye.1 + f.1 * er * 0.35 + jitter.1 * er);
                g.circle(pp, pr, BLACK);
                if expr == Expr::Crazy {
                    // the unhinged second, smaller eye
                    let e2 = at(0.05, 0.22);
                    g.circle(e2, er * 0.62, ec);
                    g.circle(e2, pr * 0.8, BLACK);
                }
                if light_face {
                    g.ring(eye, er, r * 0.06, BLACK);
                }
            }
        },
    }
    if matches!(expr, Expr::Angry | Expr::Focus) && eye_style != EyeStyle::Visor && look.head != HeadStyle::Masked {
        g.line(at(0.15, 0.62), at(0.85, 0.42), r * 0.13, ink);
    }
    match expr {
        Expr::Grin | Expr::Crazy => {
            if look.beard.is_none() {
                g.polyline(&[at(0.25, -0.38), at(0.55, -0.52), at(0.85, -0.3)], r * 0.13, ink);
            }
        }
        Expr::Shock => {
            g.ring(at(0.62, -0.45), r * 0.16, r * 0.1, ink);
        }
        _ => {}
    }
    draw_mustache(g, look, &at, r);

    // front arm last (in front of body)
    g.polyline(&[sk.shoulder, sk.elbow_f, sk.hand_f], lw, body);
    draw_sleeve(g, sk, look, true);
}

/// Points used to approximate the clickable hit box of a figure.
pub fn hit_box(sk: &Skel, look: &Look) -> (f32, f32, f32, f32) {
    if let Some(sp) = &look.sprite {
        let (foot, height, _) = sprite_place(sk, look);
        let (w, h) = sp.size_for(height);
        return (foot.0 - w / 2.0, foot.1 - h, foot.0 + w / 2.0, foot.1);
    }
    let b = sk.bbox();
    let pad = 4.0 * sk.s;
    (b.0 - pad, b.1 - pad, b.2 + pad, b.3 + pad)
}

pub const _ANCHOR_FREE_HEIGHT: f32 = TH + SH;
