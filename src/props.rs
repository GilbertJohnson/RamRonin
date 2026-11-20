//! Props, mounts and jutsu visuals. Pure drawing functions.

use crate::gfx::Gfx;
use crate::rig::Skel;
use crate::util::*;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Item {
    None,
    Whip,
    Mallet,
    Wrench,
    Crowbar,
    Gloves,
    Chicken,
    Megaphone,
    Stick,
    Kunai,
}

impl Item {
    pub fn hit_word(self) -> &'static [&'static str] {
        match self {
            Item::Whip => &["CRACK!", "WHAP!", "YEEHAW!"],
            Item::Mallet => &["BONK!", "THWOMP!", "BONK!!"],
            Item::Wrench => &["CLANK!", "TUNE-UP!", "KLONK!"],
            Item::Crowbar => &["CLANG!", "PRY!", "KRAK!"],
            Item::Gloves => &["POW!", "BAM!", "ORA!"],
            Item::Chicken => &["SQUEAK!", "BAWK!", "SQUONK!"],
            Item::Kunai => &["THUNK!", "SHINK!"],
            _ => &["BONK!"],
        }
    }
}

const WOOD: Col = Col::rgb(132, 86, 46);
const STEEL: Col = Col::rgb(170, 178, 190);
const DARK_STEEL: Col = Col::rgb(70, 76, 88);

/// Draw the item held in the front hand. `swing` = swing phase [0,1) (for whip shape).
pub fn draw_item(g: &mut Gfx, item: Item, sk: &Skel, t: f32, swing: f32) {
    let s = sk.s;
    let h = sk.hand_f;
    let d = sk.fore_dir();
    let ang = d.1.atan2(d.0);
    let along = |l: f32| (h.0 + d.0 * l * s, h.1 + d.1 * l * s);
    let perp = (-d.1, d.0);
    match item {
        Item::None => {}
        Item::Whip => {
            let handle_end = along(9.0);
            g.line(h, handle_end, 3.6 * s, Col::rgb(90, 50, 25));
            // lash: curls back during windup, snaps straight on the strike
            let strike = if swing < 0.45 { 0.0 } else if swing < 0.62 { (swing - 0.45) / 0.17 } else { 1.0 };
            let curl = lerp(2.6, -0.15, ease_out(strike)) * sk.facing;
            let n = 14;
            let len = 62.0 * s;
            let mut pts = vec![handle_end];
            let mut p = handle_end;
            for i in 1..=n {
                let k = i as f32 / n as f32;
                let wave = ((t * 24.0) - k * 7.0).sin() * 0.35 * (1.0 - strike * 0.7) * k;
                let a = ang + curl * k + wave;
                p = (p.0 + a.cos() * len / n as f32, p.1 + a.sin() * len / n as f32);
                pts.push(p);
            }
            g.taper(&pts, 2.6 * s, 0.9 * s, Col::rgb(110, 62, 30));
        }
        Item::Mallet => {
            let end = along(24.0);
            g.line(h, end, 3.2 * s, WOOD);
            g.rrect(end, 13.0 * s, 20.0 * s, ang + PI / 2.0, Col::rgb(180, 40, 40));
            g.rrect(end, 13.5 * s, 4.0 * s, ang + PI / 2.0, Col::rgb(240, 210, 80));
        }
        Item::Wrench => {
            let end = along(20.0);
            g.line(along(-2.0), end, 4.0 * s, STEEL);
            let c = along(23.0);
            // open-ended jaw: two prongs
            g.line(c, (c.0 + d.0 * 5.0 * s + perp.0 * 4.0 * s, c.1 + d.1 * 5.0 * s + perp.1 * 4.0 * s), 3.2 * s, STEEL);
            g.line(c, (c.0 + d.0 * 5.0 * s - perp.0 * 4.0 * s, c.1 + d.1 * 5.0 * s - perp.1 * 4.0 * s), 3.2 * s, STEEL);
        }
        Item::Crowbar => {
            let end = along(26.0);
            g.line(along(-3.0), end, 3.2 * s, Col::rgb(200, 30, 30));
            let hook = (end.0 + perp.0 * 6.0 * s - d.0 * 3.0 * s, end.1 + perp.1 * 6.0 * s - d.1 * 3.0 * s);
            g.polyline(&[end, (end.0 + d.0 * 3.0 * s, end.1 + d.1 * 3.0 * s), hook], 3.2 * s, Col::rgb(200, 30, 30));
        }
        Item::Gloves => {
            g.circle(sk.hand_b, 4.8 * s, Col::rgb(150, 20, 20));
            g.circle(sk.hand_f, 5.2 * s, Col::rgb(215, 30, 30));
            g.circle((sk.hand_f.0 - 1.5 * s, sk.hand_f.1 - 1.5 * s), 1.6 * s, Col::rgba(255, 255, 255, 150));
        }
        Item::Chicken => {
            let wob = (t * 18.0).sin() * 0.35;
            let a2 = ang + wob;
            let mid = (h.0 + a2.cos() * 14.0 * s, h.1 + a2.sin() * 14.0 * s);
            g.ellipse(mid, 13.0 * s, 4.6 * s, a2, Col::rgb(250, 220, 60));
            let headp = (h.0 + a2.cos() * 27.0 * s, h.1 + a2.sin() * 27.0 * s);
            g.circle(headp, 4.0 * s, Col::rgb(250, 220, 60));
            g.circle((headp.0 - (a2 + 1.4).cos() * 3.5 * s, headp.1 - (a2 + 1.4).sin() * 3.5 * s), 2.0 * s, RED);
            g.poly(
                &[
                    (headp.0 + a2.cos() * 3.0 * s, headp.1 + a2.sin() * 3.0 * s),
                    (headp.0 + a2.cos() * 8.0 * s, headp.1 + a2.sin() * 8.0 * s),
                    (headp.0 + (a2 + 0.6).cos() * 3.5 * s, headp.1 + (a2 + 0.6).sin() * 3.5 * s),
                ],
                Col::rgb(255, 140, 20),
            );
            g.circle((headp.0 + (a2 - 0.8).cos() * 1.5 * s, headp.1 + (a2 - 0.8).sin() * 1.5 * s), 0.9 * s, BLACK);
        }
        Item::Megaphone => {
            let fwd = (sk.facing, 0.0);
            let base = h;
            let tip = (base.0 + fwd.0 * 16.0 * s, base.1);
            g.poly(
                &[
                    (base.0, base.1 - 2.5 * s),
                    (tip.0, tip.1 - 7.5 * s),
                    (tip.0, tip.1 + 7.5 * s),
                    (base.0, base.1 + 2.5 * s),
                ],
                Col::rgb(240, 240, 240),
            );
            g.line((tip.0 - fwd.0 * 4.0 * s, tip.1 - 6.0 * s), (tip.0 - fwd.0 * 4.0 * s, tip.1 + 6.0 * s), 2.0 * s, RED);
            g.line(tip, (tip.0, tip.1), 1.0, RED);
            g.poly_stroke(
                &[
                    (base.0, base.1 - 2.5 * s),
                    (tip.0, tip.1 - 7.5 * s),
                    (tip.0, tip.1 + 7.5 * s),
                    (base.0, base.1 + 2.5 * s),
                ],
                1.2 * s,
                DARK_STEEL,
            );
        }
        Item::Stick => {
            g.line(along(-4.0), along(30.0), 2.4 * s, WOOD);
            g.circle(along(30.0), 2.2 * s, Col::rgb(90, 200, 90));
        }
        Item::Kunai => draw_kunai(g, h, ang, s),
    }
}

pub fn draw_kunai(g: &mut Gfx, at: P, ang: f32, s: f32) {
    let d = (ang.cos(), ang.sin());
    let p = (-d.1, d.0);
    let ring = (at.0 - d.0 * 3.0 * s, at.1 - d.1 * 3.0 * s);
    g.ring(ring, 2.0 * s, 1.0 * s, DARK_STEEL);
    g.line(at, (at.0 + d.0 * 5.0 * s, at.1 + d.1 * 5.0 * s), 2.0 * s, Col::rgb(60, 50, 50));
    let b0 = (at.0 + d.0 * 5.0 * s, at.1 + d.1 * 5.0 * s);
    g.poly(
        &[
            (b0.0 + p.0 * 2.6 * s, b0.1 + p.1 * 2.6 * s),
            (b0.0 + d.0 * 12.0 * s, b0.1 + d.1 * 12.0 * s),
            (b0.0 - p.0 * 2.6 * s, b0.1 - p.1 * 2.6 * s),
        ],
        DARK_STEEL,
    );
}

// ---------------------------------------------------------------------------
// Mounts. Each returns the point where the rider's hip goes.
// ---------------------------------------------------------------------------

pub fn horse(g: &mut Gfx, ground: P, facing: f32, s: f32, t: f32, moving: bool) -> P {
    let f = facing;
    let x = |v: f32| ground.0 + v * s * f;
    let y = |v: f32| ground.1 - v * s;
    let brown = Col::rgb(140, 88, 50);
    let dark = Col::rgb(92, 56, 30);
    let gal = if moving { t * 16.0 } else { 0.0 };
    // legs (back pair darker)
    let leg = |g: &mut Gfx, hipx: f32, ph: f32, col: Col| {
        let a = if moving { (gal + ph).sin() * 0.6 } else { 0.0 };
        let knee = (x(hipx) + a.sin() * 10.0 * s * f, y(26.0) + a.cos() * 10.0 * s);
        let b = a + if moving { ((gal + ph).cos() * 0.6).max(-0.2) } else { 0.0 };
        let foot = (knee.0 + b.sin() * 12.0 * s * f - 2.0 * s * f, knee.1 + b.cos() * 12.0 * s);
        g.polyline(&[(x(hipx), y(28.0)), knee, foot], 3.6 * s, col);
        g.line(foot, (foot.0 + 2.0 * s * f, foot.1), 3.6 * s, BLACK);
    };
    leg(g, -15.0, PI, dark);
    leg(g, 15.0, 0.0, dark);
    // tail
    let tw = (t * 10.0).sin() * 0.3;
    let tail: Vec<P> = (0..6)
        .map(|i| {
            let k = i as f32 / 5.0;
            (x(-22.0 - k * 14.0), y(36.0 - k * 10.0 + (tw + k * 2.0).sin() * 4.0 * k))
        })
        .collect();
    g.taper(&tail, 4.0 * s, 1.5 * s, BLACK);
    // body
    g.ellipse((x(0.0), y(34.0)), 25.0 * s, 10.0 * s, 0.0, brown);
    // neck & head
    let bob = if moving { (gal * 2.0).sin() * 2.0 } else { (t * 2.0).sin() };
    let neck = [(x(14.0), y(40.0)), (x(24.0), y(58.0 + bob)), (x(30.0), y(55.0 + bob)), (x(24.0), y(33.0))];
    g.poly(&neck, brown);
    g.ellipse((x(33.0), y(54.0 + bob)), 9.0 * s, 4.5 * s, if f > 0.0 { 0.5 } else { -0.5 }, brown);
    g.circle((x(31.0), y(57.5 + bob)), 1.3 * s, BLACK);
    g.poly(&[(x(25.0), y(60.0 + bob)), (x(27.0), y(66.0 + bob)), (x(28.5), y(59.0 + bob))], dark);
    // mane
    let mane: Vec<P> = (0..5).map(|i| (x(15.0 + i as f32 * 2.6), y(43.0 + i as f32 * 3.6 + bob * 0.5))).collect();
    g.taper(&mane, 4.5 * s, 2.0 * s, BLACK);
    // front legs (in front of body)
    leg(g, -18.0, 0.0, brown);
    leg(g, 18.0, PI, brown);
    // saddle
    g.ellipse((x(-2.0), y(43.0)), 9.0 * s, 3.0 * s, 0.0, Col::rgb(150, 30, 30));
    (x(-2.0), y(44.0))
}

/// Draws the car body *after* the rider so it hides the legs. Call
/// `car_seat` first to place the rider.
pub fn car_seat(ground: P, facing: f32, s: f32) -> P {
    (ground.0 - 6.0 * s * facing, ground.1 - 15.0 * s)
}

/// Car paint jobs (one is picked per ride).
pub const CAR_COLORS: &[Col] = &[
    Col::rgb(20, 120, 230),  // electric blue
    Col::rgb(255, 196, 0),   // taxi yellow
    Col::rgb(40, 190, 110),  // racing green
    Col::rgb(150, 70, 230),  // purple
    Col::rgb(255, 120, 20),  // orange
    Col::rgb(0, 190, 200),   // teal
];

pub fn car(g: &mut Gfx, ground: P, facing: f32, s: f32, t: f32, speed: f32, paint: Col) {
    let f = facing;
    let x = |v: f32| ground.0 + v * s * f;
    let y = |v: f32| ground.1 - v * s;
    let bounce = (t * 25.0).sin() * 0.8 * (speed > 1.0) as u8 as f32;
    let yb = |v: f32| y(v + bounce);
    let red = paint;
    // body
    g.poly(
        &[(x(-32.0), yb(6.0)), (x(34.0), yb(6.0)), (x(36.0), yb(14.0)), (x(28.0), yb(20.0)), (x(-30.0), yb(20.0)), (x(-34.0), yb(14.0))],
        red,
    );
    // windshield
    g.poly(&[(x(8.0), yb(20.0)), (x(16.0), yb(32.0)), (x(19.0), yb(32.0)), (x(14.0), yb(20.0))], Col::rgba(160, 220, 255, 220));
    g.line((x(16.0), yb(32.0)), (x(19.0), yb(32.0)), 1.6 * s, DARK_STEEL);
    // stripe & light
    g.line((x(-28.0), yb(13.0)), (x(30.0), yb(13.0)), 2.0 * s, WHITE);
    g.circle((x(34.0), yb(11.0)), 2.4 * s, Col::rgb(255, 230, 100));
    g.circle((x(-33.0), yb(11.0)), 2.0 * s, Col::rgb(255, 80, 80));
    // wheels
    for wx in [-20.0, 22.0] {
        let c = (x(wx), y(7.0));
        g.circle(c, 7.5 * s, BLACK);
        g.circle(c, 3.5 * s, STEEL);
        let a = -t * speed / (7.5 * s).max(1.0) * f;
        for k in 0..3 {
            let aa = a + k as f32 * TAU / 3.0;
            g.line(c, (c.0 + aa.cos() * 3.4 * s, c.1 + aa.sin() * 3.4 * s), 1.2 * s, DARK_STEEL);
        }
    }
}

pub fn pogo(g: &mut Gfx, sk: &Skel, ground: P) {
    let s = sk.s;
    let x = (sk.hand_f.0 + sk.hand_b.0) / 2.0;
    let top = (x, sk.hand_f.1.min(sk.hand_b.1) - 2.0 * s);
    let pedal_y = sk.foot_f.1.max(sk.foot_b.1);
    g.line((x - 6.0 * s, top.1), (x + 6.0 * s, top.1), 2.6 * s, BLACK);
    g.line(top, (x, pedal_y), 2.6 * s, Col::rgb(40, 120, 220));
    g.line((x - 7.0 * s, pedal_y), (x + 7.0 * s, pedal_y), 2.6 * s, BLACK);
    // spring
    let bottom = ground.1;
    let n = 7;
    let mut pts = vec![(x, pedal_y)];
    for i in 1..n {
        let yy = lerp(pedal_y, bottom, i as f32 / n as f32);
        pts.push((x + if i % 2 == 0 { -3.5 } else { 3.5 } * s, yy));
    }
    pts.push((x, bottom));
    g.polyline(&pts, 1.6 * s, STEEL);
}

pub fn jetpack(g: &mut Gfx, sk: &Skel, t: f32) {
    let s = sk.s;
    let back = (-sk.fwd.0, -sk.fwd.1);
    let c = (
        lerp(sk.hip.0, sk.neck.0, 0.6) + back.0 * 5.0 * s,
        lerp(sk.hip.1, sk.neck.1, 0.6) + back.1 * 5.0 * s,
    );
    let body_ang = (sk.neck.1 - sk.hip.1).atan2(sk.neck.0 - sk.hip.0);
    g.rrect(c, 15.0 * s, 8.0 * s, body_ang, DARK_STEEL);
    let nozzle = (
        c.0 - (sk.neck.0 - sk.hip.0) * 0.4,
        c.1 - (sk.neck.1 - sk.hip.1) * 0.4,
    );
    let dir = {
        let (dx, dy) = (sk.hip.0 - sk.neck.0, sk.hip.1 - sk.neck.1);
        let l = (dx * dx + dy * dy).sqrt().max(0.01);
        (dx / l, dy / l)
    };
    let flick = 1.0 + 0.35 * (t * 40.0).sin();
    let fl = 16.0 * s * flick;
    let p = (-dir.1, dir.0);
    g.poly(
        &[
            (nozzle.0 + p.0 * 3.5 * s, nozzle.1 + p.1 * 3.5 * s),
            (nozzle.0 + dir.0 * fl, nozzle.1 + dir.1 * fl),
            (nozzle.0 - p.0 * 3.5 * s, nozzle.1 - p.1 * 3.5 * s),
        ],
        Col::rgb(255, 140, 20),
    );
    g.poly(
        &[
            (nozzle.0 + p.0 * 2.0 * s, nozzle.1 + p.1 * 2.0 * s),
            (nozzle.0 + dir.0 * fl * 0.6, nozzle.1 + dir.1 * fl * 0.6),
            (nozzle.0 - p.0 * 2.0 * s, nozzle.1 - p.1 * 2.0 * s),
        ],
        Col::rgb(255, 235, 120),
    );
}

// ---------------------------------------------------------------------------
// Jutsu visuals (Shinobi Edition). Original drawings inspired by classic
// ninja-anime techniques: a spiralling chakra orb, a wind-blade shuriken,
// a three-tomoe eye, kunai, fire and smoke.
// ---------------------------------------------------------------------------

pub fn chakra_orb(g: &mut Gfx, c: P, r: f32, t: f32, col: Col) {
    if r < 1.0 {
        return;
    }
    for i in 0..4 {
        let k = 1.0 + i as f32 * 0.22;
        g.circle(c, r * k, col.alpha(0.18 - i as f32 * 0.035));
    }
    g.circle(c, r, col.mix(WHITE, 0.35).alpha(0.9));
    g.circle(c, r * 0.62, col.mix(WHITE, 0.7));
    // spiral streaks
    for arm in 0..4 {
        let base = t * 14.0 + arm as f32 * TAU / 4.0;
        let pts: Vec<P> = (0..10)
            .map(|i| {
                let k = i as f32 / 9.0;
                let a = base + k * 3.2;
                let rr = r * (0.15 + 0.85 * k);
                (c.0 + a.cos() * rr, c.1 + a.sin() * rr * 0.9)
            })
            .collect();
        g.taper(&pts, r * 0.08, r * 0.03, WHITE.alpha(0.85));
    }
    g.circle(c, r * 0.22, WHITE);
}

pub fn wind_shuriken(g: &mut Gfx, c: P, r: f32, t: f32, col: Col) {
    // faint outer ring
    g.ring(c, r * 1.05, r * 0.06, WHITE.alpha(0.35));
    g.ring(c, r * 0.85, r * 0.03, col.alpha(0.4));
    // blades
    let spin = t * 18.0;
    for k in 0..4 {
        let a = spin + k as f32 * TAU / 4.0;
        let tip = (c.0 + a.cos() * r, c.1 + a.sin() * r);
        let b1 = (c.0 + (a + 0.55).cos() * r * 0.35, c.1 + (a + 0.55).sin() * r * 0.35);
        let b2 = (c.0 + (a - 0.25).cos() * r * 0.28, c.1 + (a - 0.25).sin() * r * 0.28);
        let mid = (c.0 + (a + 0.28).cos() * r * 0.75, c.1 + (a + 0.28).sin() * r * 0.75);
        g.poly(&[b2, tip, mid, b1], WHITE.alpha(0.85));
        g.poly_stroke(&[b2, tip, mid, b1], r * 0.025, col.mix(WHITE, 0.3));
    }
    chakra_orb(g, c, r * 0.32, t, col);
}

/// Big flash of the three-tomoe eye (appears briefly above the ninja).
pub fn tomoe_eye(g: &mut Gfx, c: P, r: f32, t: f32, alpha: f32) {
    g.alpha = alpha;
    // almond eye shape
    let w = r * 1.9;
    let pts: Vec<P> = (0..=24)
        .map(|i| {
            let k = i as f32 / 24.0 * TAU;
            (c.0 + k.cos() * w, c.1 + k.sin() * r * 0.95 * (1.0 - 0.35 * k.cos().abs()))
        })
        .collect();
    g.poly(&pts, WHITE);
    g.circle(c, r * 0.88, Col::rgb(205, 15, 20));
    g.ring(c, r * 0.88, r * 0.07, BLACK);
    g.ring(c, r * 0.55, r * 0.03, Col::rgb(90, 0, 0));
    g.circle(c, r * 0.2, BLACK);
    for i in 0..3 {
        let a = t * 7.0 + i as f32 * TAU / 3.0;
        let tc = (c.0 + a.cos() * r * 0.55, c.1 + a.sin() * r * 0.55);
        g.circle(tc, r * 0.13, BLACK);
        // comma tail
        let tail: Vec<P> = (0..6)
            .map(|j| {
                let k = j as f32 / 5.0;
                let aa = a - k * 0.75;
                let rr = r * (0.55 + 0.04 * k);
                (c.0 + aa.cos() * rr, c.1 + aa.sin() * rr)
            })
            .collect();
        g.taper(&tail, r * 0.16, r * 0.03, BLACK);
    }
    g.poly_stroke(&pts, r * 0.1, BLACK);
    g.alpha = 1.0;
}

pub fn smoke_puff(g: &mut Gfx, c: P, r: f32, u: f32) {
    let a = (1.0 - u).clamp(0.0, 1.0);
    let rr = r * (0.5 + u * 0.9);
    for i in 0..7 {
        let ang = i as f32 / 7.0 * TAU + u * 1.5;
        let p = (c.0 + ang.cos() * rr * 0.7, c.1 + ang.sin() * rr * 0.55);
        g.circle(p, rr * 0.55, Col::rgb(235, 235, 240).alpha(a * 0.9));
    }
    g.circle(c, rr * 0.7, WHITE.alpha(a));
}

pub fn log(g: &mut Gfx, c: P, s: f32, ang: f32) {
    g.rrect(c, 34.0 * s, 13.0 * s, ang, Col::rgb(150, 98, 52));
    let d = (ang.cos(), ang.sin());
    let end = (c.0 + d.0 * 17.0 * s, c.1 + d.1 * 17.0 * s);
    g.ellipse(end, 3.5 * s, 6.5 * s, ang, Col::rgb(214, 170, 110));
    g.ring(end, 3.0 * s, 1.0 * s, Col::rgb(150, 98, 52));
    g.line(
        (c.0 - d.0 * 10.0 * s, c.1 - d.1 * 10.0 * s - 2.0 * s),
        (c.0 + d.0 * 6.0 * s, c.1 + d.1 * 6.0 * s - 2.0 * s),
        1.0 * s,
        Col::rgb(110, 70, 35),
    );
}

pub fn fireball(g: &mut Gfx, c: P, r: f32, t: f32) {
    let f = 1.0 + 0.12 * (t * 30.0).sin();
    g.circle(c, r * 1.25 * f, Col::rgba(255, 90, 20, 90));
    g.circle(c, r * f, Col::rgb(255, 110, 20));
    g.circle(c, r * 0.68 * f, Col::rgb(255, 190, 40));
    g.circle(c, r * 0.35, Col::rgb(255, 245, 190));
}

/// Comic starburst with a word in it.
pub fn burst(g: &mut Gfx, c: P, r: f32, word: &str, col: Col, rot: f32) {
    let n = 12;
    let pts: Vec<P> = (0..n * 2)
        .map(|i| {
            let a = rot + i as f32 / (n * 2) as f32 * TAU;
            let rr = if i % 2 == 0 { r } else { r * 0.62 };
            (c.0 + a.cos() * rr * 1.25, c.1 + a.sin() * rr * 0.85)
        })
        .collect();
    g.poly(&pts, col);
    g.poly_stroke(&pts, (r * 0.07).max(1.5), Col::rgb(190, 20, 20));
    g.text_outlined(c.0, c.1, r * 0.55, WHITE, Col::rgb(170, 20, 20), word);
}

pub fn zzz(g: &mut Gfx, c: P, size: f32, col: Col) {
    let h = size;
    g.polyline(&[(c.0 - h / 2.0, c.1 - h / 2.0), (c.0 + h / 2.0, c.1 - h / 2.0), (c.0 - h / 2.0, c.1 + h / 2.0), (c.0 + h / 2.0, c.1 + h / 2.0)], h * 0.18, col);
}

pub fn star(g: &mut Gfx, c: P, r: f32, rot: f32, col: Col) {
    let pts: Vec<P> = (0..10)
        .map(|i| {
            let a = rot + i as f32 * PI / 5.0 - PI / 2.0;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            (c.0 + a.cos() * rr, c.1 + a.sin() * rr)
        })
        .collect();
    g.poly(&pts, col);
}
