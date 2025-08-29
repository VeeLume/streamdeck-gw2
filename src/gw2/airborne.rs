use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// A single motion sample: (position (x,z,y) in meters, front (x,z,y), ui_tick)
pub type MotionSample = ([f32; 3], [f32; 3], u32);

/// Something that can provide motion samples (position + front + ui_tick).
/// We implement this for your MumbleLink below (in gw2::mumble).
pub trait MotionSource {
    fn read_motion(&self) -> Option<MotionSample>;
}

// ---------- Units & numerics ----------
const UNITS_PER_METER: f32 = 39.37;
const EPS: f32 = 1e-5;

// ---------- Public speed snapshot ----------
#[derive(Debug, Clone, Copy, Default)]
pub struct Speed {
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub horizontal: f32,
    pub magnitude: f32,
    pub dt_s: f32,
}

// ---------- Speed calculator (ported) ----------
pub struct SpeedCalculator {
    prev_pos_m: Option<[f32; 3]>, // (x,y,z)
    prev_t: Option<Instant>,
    smoothing_alpha: f32,
    v_smooth: [f32; 3],
    max_reasonable_dt: f32,
    max_reasonable_step_m: f32,
}
impl SpeedCalculator {
    pub fn new() -> Self {
        Self {
            prev_pos_m: None,
            prev_t: None,
            smoothing_alpha: 0.35,
            v_smooth: [0.0; 3],
            max_reasonable_dt: 0.5,
            max_reasonable_step_m: 30.0,
        }
    }

    /// Feed one sample (meters in **(x,z,y)** order).
    pub fn step(&mut self, pos_xzy_m: [f32; 3], now: Instant) -> Option<Speed> {
        let pos_m = [pos_xzy_m[0], pos_xzy_m[2], pos_xzy_m[1]]; // -> (x,y,z)

        let (prev_pos, prev_t) = match (self.prev_pos_m, self.prev_t) {
            (Some(p), Some(t)) => (p, t),
            _ => {
                self.prev_pos_m = Some(pos_m);
                self.prev_t = Some(now);
                return None;
            }
        };

        let dt = (now - prev_t).as_secs_f32();
        if dt < 1e-3 || dt > self.max_reasonable_dt {
            self.prev_pos_m = Some(pos_m);
            self.prev_t = Some(now);
            return None;
        }

        let dx = pos_m[0] - prev_pos[0];
        let dy = pos_m[1] - prev_pos[1];
        let dz = pos_m[2] - prev_pos[2];

        let step_len_m = (dx * dx + dy * dy + dz * dz).sqrt();
        if step_len_m > self.max_reasonable_step_m {
            self.prev_pos_m = Some(pos_m);
            self.prev_t = Some(now);
            return None;
        }

        let vx_m = dx / dt;
        let vy_m = dy / dt;
        let vz_m = dz / dt;

        let a = self.smoothing_alpha.clamp(0.0, 1.0);
        self.v_smooth[0] = a * self.v_smooth[0] + (1.0 - a) * vx_m;
        self.v_smooth[1] = a * self.v_smooth[1] + (1.0 - a) * vy_m;
        self.v_smooth[2] = a * self.v_smooth[2] + (1.0 - a) * vz_m;

        let vx = self.v_smooth[0] * UNITS_PER_METER;
        let vy = self.v_smooth[1] * UNITS_PER_METER;
        let vz = self.v_smooth[2] * UNITS_PER_METER;

        let horizontal = (vx * vx + vy * vy).sqrt();
        let magnitude = (horizontal * horizontal + vz * vz).sqrt();

        self.prev_pos_m = Some(pos_m);
        self.prev_t = Some(now);

        Some(Speed {
            vx: if vx.abs() < EPS { 0.0 } else { vx },
            vy: if vy.abs() < EPS { 0.0 } else { vy },
            vz: if vz.abs() < EPS { 0.0 } else { vz },
            horizontal,
            magnitude,
            dt_s: dt,
        })
    }
}

// ---------- Instant classifier (ported + trimmed) ----------
pub mod classify {
    use super::Speed;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Movement {
        Idle,
        Walk,
        RunForward,
        Strafe,
        Backpedal,
        GlideForward,
        GlideNeutral,
        GlideBack,
        Falling,
        FallingTerminal,
        Other,
    }

    pub const GLIDE_VZ_MIN: f32 = 80.0;
    const GLIDE_VZ_MAX: f32 = 150.0;
    const GLIDE_BAND_TOL: f32 = 18.0;

    const GLIDE_BACK_H: f32 = 80.0;
    const GLIDE_NEUTRAL_H: f32 = 294.0;
    const GLIDE_FWD_H: f32 = 390.0;

    const FALL_MIN_VZ: f32 = 220.0;
    const FALL_VERTICAL_DOM_RATIO: f32 = 1.35;
    const TERMINAL_VZ: f32 = 900.0;
    const BEYOND_GLIDE_VZ_MARGIN: f32 = 20.0;

    const IDLE_H_MAX: f32 = 10.0;

    const WALK_H: f32 = 80.0;
    const WALK_TOL_H: f32 = 20.0;

    const RUN_FWD_OOC_H: f32 = 294.0;
    const RUN_FWD_IC_H: f32 = 210.0;
    const RUN_STRAFE_H: f32 = 180.0;
    const RUN_BACK_H: f32 = 105.0;
    const RUN_FWD_TOL_H: f32 = 50.0;
    const RUN_STRAFE_TOL_H: f32 = 28.0;
    const RUN_BACK_TOL_H: f32 = 24.0;
    const GROUND_MAX_VZ_FOR_RUN: f32 = 180.0;
    const GROUND_MAX_GLIDE_RATIO: f32 = 0.9;

    #[inline]
    pub fn facing_xy_from_front(front_xzy: [f32; 3]) -> [f32; 2] {
        let fx = front_xzy[0];
        let fy = front_xzy[2];
        let len = (fx * fx + fy * fy).sqrt();
        if len > 1e-3 {
            [fx / len, fy / len]
        } else {
            [0.0, 1.0]
        }
    }

    #[inline]
    pub fn snap_glide(h: f32) -> Movement {
        let diffs = [
            (Movement::GlideBack, (h - GLIDE_BACK_H).abs()),
            (Movement::GlideNeutral, (h - GLIDE_NEUTRAL_H).abs()),
            (Movement::GlideForward, (h - GLIDE_FWD_H).abs()),
        ];
        diffs
            .into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap()
            .0
    }

    pub fn classify(s: &Speed, facing_xy: Option<[f32; 2]>) -> Movement {
        let h = s.horizontal;
        let vz = s.vz;
        let abs_vz = vz.abs();
        let speed3d = s.magnitude;

        if h < IDLE_H_MAX && abs_vz < 5.0 {
            return Movement::Idle;
        }

        if vz < -(GLIDE_VZ_MAX + BEYOND_GLIDE_VZ_MARGIN)
            && abs_vz >= FALL_VERTICAL_DOM_RATIO * (h + 1.0)
        {
            return if abs_vz >= TERMINAL_VZ {
                Movement::FallingTerminal
            } else {
                Movement::Falling
            };
        }

        if vz <= -FALL_MIN_VZ && abs_vz >= FALL_VERTICAL_DOM_RATIO * (h + 1.0) {
            return if abs_vz >= TERMINAL_VZ {
                Movement::FallingTerminal
            } else {
                Movement::Falling
            };
        }

        let mut forward_comp = h;
        let mut fwd_like = false;
        let mut back_like = false;
        let mut strafe_like = false;

        if let Some([fx, fy]) = facing_xy {
            if h > 30.0 {
                let nhx = s.vx / h;
                let nhy = s.vy / h;
                let dot = (nhx * fx + nhy * fy).clamp(-1.0, 1.0);
                forward_comp = h * dot;
                fwd_like = dot >= 0.35;
                back_like = dot <= -0.35;
                strafe_like = !fwd_like && !back_like;
            }
        }

        if vz < 0.0 && abs_vz >= GLIDE_VZ_MIN && abs_vz <= GLIDE_VZ_MAX {
            let m = snap_glide(h);
            let ok = match m {
                Movement::GlideBack => (h - GLIDE_BACK_H).abs() <= GLIDE_BAND_TOL,
                Movement::GlideNeutral => (h - GLIDE_NEUTRAL_H).abs() <= GLIDE_BAND_TOL,
                Movement::GlideForward => (h - GLIDE_FWD_H).abs() <= GLIDE_BAND_TOL,
                _ => false,
            };
            if ok {
                return m;
            }
        }

        let ground_ok_by_vz =
            abs_vz <= GROUND_MAX_VZ_FOR_RUN || abs_vz / (h + 1.0) < GROUND_MAX_GLIDE_RATIO;

        if ground_ok_by_vz {
            if (speed3d - WALK_H).abs() <= WALK_TOL_H {
                return Movement::Walk;
            }
            if back_like || (h - RUN_BACK_H).abs() <= RUN_BACK_TOL_H {
                return Movement::Backpedal;
            }
            if strafe_like || (h - RUN_STRAFE_H).abs() <= RUN_STRAFE_TOL_H {
                return Movement::Strafe;
            }
            for (t, tol) in [
                (RUN_FWD_OOC_H, RUN_FWD_TOL_H),
                (RUN_FWD_IC_H, RUN_FWD_TOL_H),
            ] {
                if (forward_comp - t).abs() <= tol {
                    return Movement::RunForward;
                }
            }
            for (t, tol) in [
                (RUN_FWD_OOC_H, RUN_FWD_TOL_H),
                (RUN_FWD_IC_H, RUN_FWD_TOL_H),
            ] {
                if (h - t).abs() <= tol {
                    return Movement::RunForward;
                }
            }
            if fwd_like && h > 150.0 {
                return Movement::RunForward;
            }
        }

        Movement::Other
    }
}

// ---------- Temporal layer (ported + trimmed) ----------
use classify::Movement;

fn same_airborne_family(a: Movement, b: Movement) -> bool {
    use Movement::*;
    matches!(
        (a, b),
        (GlideBack, _) | (GlideNeutral, _) | (GlideForward, _)
      | (Falling, _) | (FallingTerminal, _)
        if matches!(b, GlideBack|GlideNeutral|GlideForward|Falling|FallingTerminal)
    )
}

fn vz_trend(hist: &VecDeque<(Instant, Speed)>) -> f32 {
    if hist.len() < 2 {
        return 0.0;
    }
    let (t0, s0) = hist.front().unwrap();
    let (t1, s1) = hist.back().unwrap();
    let dt = (*t1 - *t0).as_secs_f32().max(1e-3);
    (s1.vz - s0.vz) / dt
}

const GLIDE_LOCK_VZ_MIN: f32 = 60.0;
const GLIDE_LOCK_VZ_MAX: f32 = 170.0;
const GLIDE_LOCK_DWELL_MS: u64 = 180;
const BEYOND_GLIDE_VZ_MARGIN: f32 = 20.0;

pub struct TemporalClassifier {
    history: VecDeque<(Instant, Speed)>,
    window: Duration,
    state: Movement,
    last_switch: Instant,
    min_dwell_in: Duration,
    min_dwell_out: Duration,
    glide_locked_until: Instant,
}
impl TemporalClassifier {
    pub fn new(now: Instant) -> Self {
        Self {
            history: VecDeque::with_capacity(64),
            window: Duration::from_millis(300),
            state: Movement::Idle,
            last_switch: now,
            min_dwell_in: Duration::from_millis(120),
            min_dwell_out: Duration::from_millis(160),
            glide_locked_until: now,
        }
    }

    pub fn update(&mut self, now: Instant, s: Speed, facing_xy: Option<[f32; 2]>) -> Movement {
        self.history.push_back((now, s));
        while let Some((t0, _)) = self.history.front() {
            if now.duration_since(*t0) > self.window {
                self.history.pop_front();
            } else {
                break;
            }
        }

        let mut sum = Speed::default();
        for (_, sp) in &self.history {
            sum.vx += sp.vx;
            sum.vy += sp.vy;
            sum.vz += sp.vz;
            sum.horizontal += sp.horizontal;
            sum.magnitude += sp.magnitude;
        }
        let n = self.history.len().max(1) as f32;
        let avg = Speed {
            vx: sum.vx / n,
            vy: sum.vy / n,
            vz: sum.vz / n,
            horizontal: sum.horizontal / n,
            magnitude: sum.magnitude / n,
            dt_s: s.dt_s,
        };

        // majority vote over tail
        let k = 5usize.min(self.history.len());
        let mut counts = std::collections::HashMap::<Movement, usize>::new();
        for (_, sp) in self.history.iter().rev().take(k) {
            *counts.entry(classify::classify(sp, facing_xy)).or_default() += 1;
        }
        let (vote, _) = counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap_or((Movement::Other, 0));
        let avg_label = classify::classify(&avg, facing_xy);

        const FALL_ACCEL_GATE: f32 = -350.0;
        let accel_suggests_fall =
            vz_trend(&self.history) <= FALL_ACCEL_GATE && avg.vz < -classify::GLIDE_VZ_MIN;

        let abs_vz = avg.vz.abs();
        let looks_glidey_vz =
            avg.vz < 0.0 && abs_vz >= GLIDE_LOCK_VZ_MIN && abs_vz <= GLIDE_LOCK_VZ_MAX;
        if looks_glidey_vz {
            self.glide_locked_until = now + Duration::from_millis(GLIDE_LOCK_DWELL_MS);
        }
        let beyond_glide_vz = avg.vz < -(150.0 + BEYOND_GLIDE_VZ_MARGIN);
        if accel_suggests_fall || beyond_glide_vz {
            self.glide_locked_until = now;
        }

        let mut proposed = if accel_suggests_fall {
            Movement::Falling
        } else if vote == avg_label {
            avg_label
        } else {
            avg_label
        };

        if now <= self.glide_locked_until {
            proposed = classify::snap_glide(avg.horizontal);
        } else {
            if matches!(
                self.state,
                Movement::GlideBack | Movement::GlideNeutral | Movement::GlideForward
            ) && matches!(proposed, Movement::Other)
            {
                proposed = classify::snap_glide(avg.horizontal);
            }
        }

        let cur = self.state;
        if proposed != cur {
            if same_airborne_family(cur, proposed) && !matches!(proposed, Movement::Other) {
                self.state = proposed;
                self.last_switch = now;
            } else {
                let since = now.duration_since(self.last_switch);
                let mut proposed_time = Duration::ZERO;
                let mut last_t = now;
                for (t, sp) in self.history.iter().rev() {
                    if classify::classify(sp, facing_xy) == proposed {
                        proposed_time += last_t.saturating_duration_since(*t);
                        last_t = *t;
                    } else {
                        break;
                    }
                }
                if proposed_time >= self.min_dwell_in && since >= self.min_dwell_out {
                    self.state = proposed;
                    self.last_switch = now;
                }
            }
        }

        self.state
    }
}

// ---------- Thin runtime wrapper used by the adapter ----------
pub struct AirClassifier {
    calc: SpeedCalculator,
    temporal: TemporalClassifier,
    last_state: Movement,
    last_change: Instant,

    // expose this to the adapter
    pub landing_grace_ms: u64,
}

impl AirClassifier {
    pub fn new(now: Instant) -> Self {
        Self {
            calc: SpeedCalculator::new(),
            temporal: TemporalClassifier::new(now),
            last_state: Movement::Idle,
            last_change: now,
            landing_grace_ms: 250,
        }
    }

    /// Call once per loop with a motion source.
    pub fn update_with<S: MotionSource>(&mut self, source: &S) -> Movement {
        let now = Instant::now();

        let (pos_xzy, front_xzy, tick) = match source.read_motion() {
            Some(v) => v,
            None => return self.last_state,
        };

        if let Some(spd) = self.calc.step(pos_xzy, now) {
            let facing_xy = classify::facing_xy_from_front(front_xzy);
            let state = self.temporal.update(now, spd, Some(facing_xy));
            if state != self.last_state {
                self.last_state = state;
                self.last_change = now;
            }
        }

        self.last_state
    }

    #[inline]
    pub fn is_airborne(&self) -> bool {
        matches!(
            self.last_state,
            Movement::GlideBack
                | Movement::GlideNeutral
                | Movement::GlideForward
                | Movement::Falling
                | Movement::FallingTerminal
        )
    }

    #[inline]
    pub fn landed_recently(&self) -> bool {
        !self.is_airborne()
            && self.last_change.elapsed().as_millis() as u64 <= self.landing_grace_ms
    }

    #[inline]
    pub fn state(&self) -> Movement {
        self.last_state
    }
}
