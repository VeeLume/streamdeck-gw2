// src/bin/glide_logger.rs
#![cfg(windows)]

/*!
A tiny GW2 movement/speed probe using Mumble Link.

- Reads Mumble shared memory (`f_avatar_position`, `f_avatar_front`)
- Computes player velocity (GW2 “units” = inches/s)
- Classifies movement (walk/run/strafe/back, glide [back/neutral/forward], falling)
- Adds a 0.3s temporal layer (rolling average + hysteresis) to reduce “Other” flicker

Notes:
- Mumble positions are **meters** and in *(x, z, y)* order. We remap to *(x, y, z)* and convert to inches/s.
- Position updates ~25 Hz; `f_avatar_front` updates every frame.
- Positive **vz** = up; negative = down.
*/

use std::{
    collections::VecDeque,
    mem, slice, thread,
    time::{Duration, Instant},
};

use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;
use windows_core::w;

use crate::movement_speed::{
    Speed, SpeedCalculator,
    classify::{self, Movement},
};

// ---------- Tiny bytemuck shim (no external dep) ----------
mod bytemuck {
    #[inline]
    pub fn from_bytes<T: Copy>(b: &[u8]) -> &T {
        assert!(b.len() >= std::mem::size_of::<T>());
        // Safety: caller guarantees `b` points at a properly aligned `T` in MappedView
        unsafe { &*(b.as_ptr() as *const T) }
    }
}

// ---------- Mumble Link bindings ----------
#[repr(C)]
#[derive(Clone, Copy)]
struct LinkedMem {
    ui_version: u32,
    ui_tick: u32,
    /// Player position in meters, stored as (x, z, y); updates ~25 Hz
    f_avatar_position: [f32; 3],
    /// Player facing vector, stored as (x, z, y); updates every frame
    f_avatar_front: [f32; 3],
    /// Player top vector, stored as (x, z, y)
    f_avatar_top: [f32; 3],
    name: [u16; 256],
    // Camera fields unused in this tool
    f_camera_position: [u16; 0],
    f_camera_front: [u16; 0],
    f_camera_top: [u16; 0],
    identity: [u16; 256],
    context_len: u32,
    context: [u8; 256],
    description: [u16; 2048],
}
const SHARED_MEM_SIZE: usize = mem::size_of::<LinkedMem>();

struct MumbleLink {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}
impl MumbleLink {
    fn new() -> Result<Self, String> {
        let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, w!("MumbleLink")) }
            .map_err(|_| "OpenFileMappingW(MumbleLink) failed".to_string())?;
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, SHARED_MEM_SIZE) };
        if view.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("MapViewOfFile failed".into());
        }
        Ok(Self { handle, view })
    }

    #[inline]
    fn read(&self) -> Option<LinkedMem> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view.Value as *const u8, SHARED_MEM_SIZE);
            Some(*bytemuck::from_bytes::<LinkedMem>(bytes))
        }
    }
}
impl Drop for MumbleLink {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.handle);
        }
    }
}

// ---------- Movement & speed ----------
mod movement_speed {
    use std::time::Instant;

    // --- Units & numerics ---
    const UNITS_PER_METER: f32 = 39.37; // inches per meter
    const EPS: f32 = 1e-5;

    // Reference speeds (units/s)
    pub mod ref_speeds {
        pub const OUT_OF_COMBAT_FWD: f32 = 294.0;
        pub const OUT_OF_COMBAT_STRAFE: f32 = 180.0;
        pub const OUT_OF_COMBAT_BACK: f32 = 105.0;

        pub const IN_COMBAT_FWD: f32 = 210.0;
        pub const IN_COMBAT_STRAFE: f32 = 180.0;
        pub const IN_COMBAT_BACK: f32 = 105.0;

        pub const WALKING: f32 = 80.0;

        // Glide horizontals; vertical ~113 u/s downward across leans
        pub const GLIDE_FWD: f32 = 390.0;
        pub const GLIDE_NEUTRAL: f32 = 294.0;
        pub const GLIDE_BACK: f32 = 220.0;
        pub const GLIDE_DESCEND: f32 = 113.0;
    }

    /// Velocity snapshot (GW2 units = inches/s).
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Speed {
        /// Components in units/s after remap to (x, y, z), with z vertical
        pub vx: f32,
        pub vy: f32,
        pub vz: f32,
        /// Horizontal magnitude (xy-plane), units/s
        pub horizontal: f32,
        /// Full 3D magnitude, units/s
        pub magnitude: f32,
        /// Sample-to-sample dt in seconds
        pub dt_s: f32,
    }

    /// Computes smoothed velocity from consecutive Mumble position samples.
    pub struct SpeedCalculator {
        prev_pos_m: Option<[f32; 3]>, // previous position (x,y,z) in meters
        prev_t: Option<Instant>,
        smoothing_alpha: f32, // exp smoothing factor on velocity
        v_smooth: [f32; 3],   // last smoothed velocity (m/s)
        // Guards
        max_reasonable_dt: f32,     // drop samples if too old
        max_reasonable_step_m: f32, // drop teleports/waypoints
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
        /// Returns None on first call or if the sample is discarded by guards.
        pub fn step(&mut self, mumble_pos_m_xzy: [f32; 3], now: Instant) -> Option<Speed> {
            // Remap (x, z, y) -> (x, y, z) with z vertical
            let pos_m = [
                mumble_pos_m_xzy[0],
                mumble_pos_m_xzy[2],
                mumble_pos_m_xzy[1],
            ];

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
                // skip absurd intervals (loading, stalls)
                self.prev_pos_m = Some(pos_m);
                self.prev_t = Some(now);
                return None;
            }

            // Displacement in meters
            let dx = pos_m[0] - prev_pos[0];
            let dy = pos_m[1] - prev_pos[1];
            let dz = pos_m[2] - prev_pos[2];

            // Guard teleports
            let step_len_m = (dx * dx + dy * dy + dz * dz).sqrt();
            if step_len_m > self.max_reasonable_step_m {
                self.prev_pos_m = Some(pos_m);
                self.prev_t = Some(now);
                return None;
            }

            // Raw velocity in m/s
            let vx_m = dx / dt;
            let vy_m = dy / dt;
            let vz_m = dz / dt;

            // Exponential smoothing (helps with 25 Hz jitter)
            let a = self.smoothing_alpha.clamp(0.0, 1.0);
            self.v_smooth[0] = a * self.v_smooth[0] + (1.0 - a) * vx_m;
            self.v_smooth[1] = a * self.v_smooth[1] + (1.0 - a) * vy_m;
            self.v_smooth[2] = a * self.v_smooth[2] + (1.0 - a) * vz_m;

            // Convert to units/s (inches/s)
            let vx_u = self.v_smooth[0] * UNITS_PER_METER;
            let vy_u = self.v_smooth[1] * UNITS_PER_METER;
            let vz_u = self.v_smooth[2] * UNITS_PER_METER;

            let horizontal = (vx_u * vx_u + vy_u * vy_u).sqrt();
            let magnitude = (horizontal * horizontal + vz_u * vz_u).sqrt();

            // Update state
            self.prev_pos_m = Some(pos_m);
            self.prev_t = Some(now);

            Some(Speed {
                vx: if vx_u.abs() < EPS { 0.0 } else { vx_u },
                vy: if vy_u.abs() < EPS { 0.0 } else { vy_u },
                vz: if vz_u.abs() < EPS { 0.0 } else { vz_u },
                horizontal,
                magnitude,
                dt_s: dt,
            })
        }
    }

    // ---------- Instant classifier (per-sample) ----------
    pub mod classify {
        use super::{Speed, ref_speeds};

        /// Movement buckets. “Other” is a safety net; temporal layer reduces its usage.
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

        // --- Tunables ---
        const IDLE_H_MAX: f32 = 10.0;

        // Glide signature
        pub const GLIDE_VZ_MIN: f32 = 80.0;
        const GLIDE_VZ_MAX: f32 = 150.0;
        const GLIDE_BAND_TOL: f32 = 18.0;

        const GLIDE_BACK_H: f32 = 80.0;
        const GLIDE_NEUTRAL_H: f32 = 294.0;
        const GLIDE_FWD_H: f32 = 390.0;

        // Falling
        const FALL_MIN_VZ: f32 = 220.0;
        const FALL_VERTICAL_DOM_RATIO: f32 = 1.35;
        const TERMINAL_VZ: f32 = 900.0;
        const BEYOND_GLIDE_VZ_MARGIN: f32 = 20.0;

        // Ground
        const WALK_H: f32 = ref_speeds::WALKING; // 80
        const WALK_TOL_H: f32 = 20.0;

        const RUN_FWD_OOC_H: f32 = ref_speeds::OUT_OF_COMBAT_FWD; // 294
        const RUN_FWD_IC_H: f32 = ref_speeds::IN_COMBAT_FWD; // 210
        const RUN_STRAFE_H: f32 = ref_speeds::OUT_OF_COMBAT_STRAFE; // 180
        const RUN_BACK_H: f32 = ref_speeds::OUT_OF_COMBAT_BACK; // 105

        // Wider forward bands; slopes/latency/buffs
        const RUN_FWD_TOL_H: f32 = 50.0;
        const RUN_STRAFE_TOL_H: f32 = 28.0;
        const RUN_BACK_TOL_H: f32 = 24.0;

        // Prefer ground over glide if vertical isn’t dominating
        const GROUND_MAX_VZ_FOR_RUN: f32 = 180.0;
        const GROUND_MAX_GLIDE_RATIO: f32 = 0.9;

        /// Build normalized facing XY from `f_avatar_front` (order **(x,z,y)**).
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

        /// Snap ambiguous glide to the nearest known horizontal band.
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

        /// Classify a single `Speed` sample. Slope-safe (uses 3D for walk/run) and
        /// glide-safe (requires both |vz| and an in-band horizontal).
        pub fn classify(s: &Speed, facing_xy: Option<[f32; 2]>) -> Movement {
            let h = s.horizontal;
            let vz = s.vz;
            let abs_vz = vz.abs();
            let speed3d = s.magnitude;

            // Idle
            if h < IDLE_H_MAX && abs_vz < 5.0 {
                return Movement::Idle;
            }

            // Early-fall: beyond glide’s vertical envelope ⇒ Falling
            if vz < -(GLIDE_VZ_MAX + BEYOND_GLIDE_VZ_MARGIN)
                && abs_vz >= FALL_VERTICAL_DOM_RATIO * (h + 1.0)
            {
                return if abs_vz >= TERMINAL_VZ {
                    Movement::FallingTerminal
                } else {
                    Movement::Falling
                };
            }

            // Falling
            if vz <= -FALL_MIN_VZ && abs_vz >= FALL_VERTICAL_DOM_RATIO * (h + 1.0) {
                return if abs_vz >= TERMINAL_VZ {
                    Movement::FallingTerminal
                } else {
                    Movement::Falling
                };
            }

            // Forward projection (helps RunForward on slopes)
            let mut forward_comp = h; // no facing ⇒ treat as unsigned
            let mut fwd_like = false;
            let mut back_like = false;
            let mut strafe_like = false;

            if let Some([fx, fy]) = facing_xy {
                if h > 30.0 {
                    let nhx = s.vx / h;
                    let nhy = s.vy / h;
                    let dot = nhx * fx + nhy * fy; // cos(theta)
                    forward_comp = h * dot.clamp(-1.0, 1.0);
                    fwd_like = dot >= 0.35;
                    back_like = dot <= -0.35;
                    strafe_like = !fwd_like && !back_like;
                }
            }

            // Glide: require |vz| in [80,150] AND horizontal near 80/294/390
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

            // Ground preferred if vertical is small or not dominating
            let ground_ok_by_vz =
                abs_vz <= GROUND_MAX_VZ_FOR_RUN || abs_vz / (h + 1.0) < GROUND_MAX_GLIDE_RATIO;
            if ground_ok_by_vz {
                // Walk (use 3D magnitude so slopes don’t misclassify)
                if (speed3d - WALK_H).abs() <= WALK_TOL_H {
                    return Movement::Walk;
                }

                // Backpedal / Strafe
                if back_like || (h - RUN_BACK_H).abs() <= RUN_BACK_TOL_H {
                    return Movement::Backpedal;
                }
                if strafe_like || (h - RUN_STRAFE_H).abs() <= RUN_STRAFE_TOL_H {
                    return Movement::Strafe;
                }

                // Forward run by forward component; fallback to horizontal
                for (t, tol) in [(RUN_FWD_OOC_H, 50.0), (RUN_FWD_IC_H, 50.0)] {
                    if (forward_comp - t).abs() <= tol {
                        return Movement::RunForward;
                    }
                }
                for (t, tol) in [(RUN_FWD_OOC_H, 50.0), (RUN_FWD_IC_H, 50.0)] {
                    if (h - t).abs() <= tol {
                        return Movement::RunForward;
                    }
                }

                // Assertive forward fallback
                if fwd_like && h > 150.0 {
                    return Movement::RunForward;
                }
            }

            Movement::Other
        }
    }
}

// ---------- Temporal layer (rolling average + hysteresis) ----------
fn same_airborne_family(a: Movement, b: Movement) -> bool {
    use Movement::*;
    matches!(
        (a, b),
        (GlideBack,    _) | (GlideNeutral, _) | (GlideForward, _)
      | (Falling,      _) | (FallingTerminal, _)
            if matches!(b, GlideBack|GlideNeutral|GlideForward|Falling|FallingTerminal)
    )
}

/// dvz/dt over the current window (units/s²). Negative ⇒ accelerating downward.
fn vz_trend(hist: &VecDeque<(Instant, Speed)>) -> f32 {
    if hist.len() < 2 {
        return 0.0;
    }
    let (t0, s0) = hist.front().unwrap();
    let (t1, s1) = hist.back().unwrap();
    let dt = (*t1 - *t0).as_secs_f32().max(1e-3);
    (s1.vz - s0.vz) / dt
}

// --- Glide lock tunables ---
const GLIDE_LOCK_VZ_MIN: f32 = 60.0; // widen around the 80..150 glide window
const GLIDE_LOCK_VZ_MAX: f32 = 170.0; // tolerate jitter while turning
const GLIDE_LOCK_DWELL_MS: u64 = 180; // how long we “stick” to glide after seeing it
const BEYOND_GLIDE_VZ_MARGIN: f32 = 20.0; // match your classifier’s early-fall margin

/// Smooth, sticky classifier over a ~0.3s window.
/// - Rolling mean of speed
/// - Majority vote over last K samples
/// - Hysteresis (min dwell) with cheap switches within the airborne family
pub struct TemporalClassifier {
    history: VecDeque<(Instant, Speed)>,
    window: Duration, // e.g. 0.3 s

    state: Movement,
    last_switch: Instant,

    // Dwell thresholds
    min_dwell_in: Duration, // need this much consistent evidence to adopt a new state
    min_dwell_out: Duration, // need this much time since last switch to abandon current

    // short-lived “we are gliding” lock to ignore facing-induced flaps
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

    /// Push latest `Speed` + optional facing and get the temporally-smoothed label.
    pub fn update(&mut self, now: Instant, s: Speed, facing_xy: Option<[f32; 2]>) -> Movement {
        // 1) push + trim window
        self.history.push_back((now, s));
        while let Some((t0, _)) = self.history.front() {
            if now.duration_since(*t0) > self.window {
                self.history.pop_front();
            } else {
                break;
            }
        }

        // 2) rolling averages
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

        // 3) majority vote across last K samples (helps reduce “Other” blips)
        let k = 5usize.min(self.history.len());
        let mut counts = std::collections::HashMap::<Movement, usize>::new();
        for (_, sp) in self.history.iter().rev().take(k) {
            let m = classify::classify(sp, facing_xy);
            *counts.entry(m).or_default() += 1;
        }
        let (vote, _) = counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap_or((Movement::Other, 0));

        // 4) averaged-sample label
        let avg_label = classify::classify(&avg, facing_xy);

        // 5) Falling bias from vertical acceleration
        const FALL_ACCEL_GATE: f32 = -350.0; // accelerating downward fast enough
        let accel_suggests_fall = vz_trend(&self.history) <= FALL_ACCEL_GATE
            && avg.vz < -(movement_speed::classify::GLIDE_VZ_MIN);

        // If avg vertical looks glide-ish, extend the lock.
        let abs_vz = avg.vz.abs();
        let looks_glidey_vz =
            avg.vz < 0.0 && abs_vz >= GLIDE_LOCK_VZ_MIN && abs_vz <= GLIDE_LOCK_VZ_MAX;

        if looks_glidey_vz {
            self.glide_locked_until = now + Duration::from_millis(GLIDE_LOCK_DWELL_MS);
        }

        // Break the lock immediately if we’re clearly beyond the glide envelope (falling fast).
        let beyond_glide_vz = avg.vz < -(150.0 + BEYOND_GLIDE_VZ_MARGIN); // mirrors your classifier
        if accel_suggests_fall || beyond_glide_vz {
            self.glide_locked_until = now; // cancel lock
        }

        // 6) Proposed label (combine heuristics). Keep airborne family sticky and avoid “Other” when airborne.
        let mut proposed = if accel_suggests_fall {
            Movement::Falling
        } else if vote == avg_label {
            avg_label
        } else {
            avg_label
        };

        // While locked, stay within glide subtypes.
        if now <= self.glide_locked_until {
            // While locked, never drop to ground/Other; snap within glide subtypes.
            proposed = classify::snap_glide(avg.horizontal);
        } else {
            // If we’re gliding and the proposal is Other, snap within glide instead of dropping to Other.
            if matches!(
                self.state,
                Movement::GlideBack | Movement::GlideNeutral | Movement::GlideForward
            ) && matches!(proposed, Movement::Other)
            {
                proposed = classify::snap_glide(avg.horizontal);
            }
        }

        // 7) Hysteresis / family rules
        let now_state = self.state;
        if proposed != now_state {
            // In-family (airborne) switches are cheap and immediate.
            if same_airborne_family(now_state, proposed) {
                if !matches!(proposed, Movement::Other) {
                    self.state = proposed;
                    self.last_switch = now;
                }
            } else {
                // Normal dwell: need some time with consistent proposed evidence.
                let since = now.duration_since(self.last_switch);
                // Accumulate how long the tail of the window has matched `proposed`
                let mut proposed_time = Duration::ZERO;
                let mut last_t = now;
                for (t, sp) in self.history.iter().rev() {
                    let lbl = classify::classify(sp, facing_xy);
                    if lbl == proposed {
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

        // Sticky safety: if airborne and proposal was Other, keep current.
        // if same_airborne_family(self.state, Movement::Other) {
        //     // no-op
        // }

        self.state
    }
}

// ---------- Demo loop ----------
fn main() -> Result<(), String> {
    let link = MumbleLink::new()?;
    let mut calc = SpeedCalculator::new();
    let mut temporal = TemporalClassifier::new(Instant::now());

    loop {
        if let Some(mem) = link.read() {
            let now = Instant::now();
            if let Some(s) = calc.step(mem.f_avatar_position, now) {
                let facing_xy = movement_speed::classify::facing_xy_from_front(mem.f_avatar_front); // (x,z,y) → (x,y)
                let mv = temporal.update(now, s, Some(facing_xy));
                println!(
                    "h={:>6.1}  vz={:>6.1}  3d={:>6.1}  ⇒ {:?}",
                    s.horizontal, s.vz, s.magnitude, mv
                );
            }
        }
        // ~25 Hz; Mumble updates ~25 Hz. If you want snappier feel, drop to ~20ms.
        thread::sleep(Duration::from_millis(40));
    }
}
