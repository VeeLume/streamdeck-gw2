// src/bin/glide_logger.rs
#![cfg(windows)]

use std::{
    collections::VecDeque,
    env,
    fs::File,
    io::Write,
    mem, slice, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;
use windows_core::w;

use crate::movement_speed::{
    Speed, SpeedCalculator,
    classify::{self, Movement, classify},
};

// tiny bytemuck shim (no external dep)
mod bytemuck {
    pub fn from_bytes<T: Copy>(b: &[u8]) -> &T {
        assert!(b.len() >= std::mem::size_of::<T>());
        unsafe { &*(b.as_ptr() as *const T) }
    }
}

/// Mumble LinkedMem — meters; stored order is (x, z, y).
/// We remap to (x, y, z) with z = vertical (up = +z)
#[repr(C)]
#[derive(Clone, Copy)]
struct LinkedMem {
    ui_version: u32,
    ui_tick: u32,
    f_avatar_position: [f32; 3], // (x, z, y) — position updates ~25 Hz, in meters
    f_avatar_front: [f32; 3],    // (x, z, y) — updated every frame
    f_avatar_top: [f32; 3],      // (x, z, y)
    name: [u16; 256],
    f_camera_position: [u16; 0], // unused
    f_camera_front: [u16; 0],    // unused
    f_camera_top: [u16; 0],      // unused
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
            };
            return Err("MapViewOfFile failed".into());
        }
        Ok(Self { handle, view })
    }
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

mod movement_speed {
    use std::time::{Duration, Instant};

    // --- Units ---
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

        pub const GLIDE_FWD: f32 = 390.0;
        pub const GLIDE_NEUTRAL: f32 = 294.0;
        pub const GLIDE_BACK: f32 = 220.0;
        pub const GLIDE_DESCEND: f32 = 113.0;
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct Speed {
        /// Velocity components in GW2 units/s after remap to (x,y,z) with z vertical
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

    pub struct SpeedCalculator {
        // previous position in meters, remapped to (x, y, z) where z is vertical
        prev_pos_m: Option<[f32; 3]>,
        prev_t: Option<Instant>,
        // simple exponential smoothing on velocity
        smoothing_alpha: f32,
        v_smooth: [f32; 3],
        // guards
        max_reasonable_dt: f32,
        max_reasonable_step_m: f32,
    }

    impl SpeedCalculator {
        pub fn new() -> Self {
            Self {
                prev_pos_m: None,
                prev_t: None,
                smoothing_alpha: 0.35, // tweak to taste (0=no smooth, 1=frozen)
                v_smooth: [0.0; 3],
                max_reasonable_dt: 0.5, // ignore long gaps (loading, alt-tab)
                max_reasonable_step_m: 30.0, // ignore teleports/waypoints etc.
            }
        }

        /// Feed one sample from Mumble (meters, order **(x,z,y)**) and current time.
        /// Returns None until a previous sample exists or if the sample was discarded.
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
            // Guard weird dt
            if dt < 1e-3 || dt > self.max_reasonable_dt {
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

            // Raw velocity in meters/s
            let vx_m = dx / dt;
            let vy_m = dy / dt;
            let vz_m = dz / dt;

            // Smooth
            let a = self.smoothing_alpha.clamp(0.0, 1.0);
            self.v_smooth[0] = a * self.v_smooth[0] + (1.0 - a) * vx_m;
            self.v_smooth[1] = a * self.v_smooth[1] + (1.0 - a) * vy_m;
            self.v_smooth[2] = a * self.v_smooth[2] + (1.0 - a) * vz_m;

            // Convert to GW2 units/s (inches/s)
            let vx_u = self.v_smooth[0] * UNITS_PER_METER;
            let vy_u = self.v_smooth[1] * UNITS_PER_METER;
            let vz_u = self.v_smooth[2] * UNITS_PER_METER;

            let horizontal = (vx_u * vx_u + vy_u * vy_u).sqrt();
            let magnitude = (vx_u * vx_u + vy_u * vy_u + vz_u * vz_u).sqrt();

            // update state
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

    pub mod classify {
        use super::Speed;
        use super::ref_speeds;

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
        const GLIDE_VZ_MIN: f32 = 80.0;
        const GLIDE_VZ_MAX: f32 = 150.0;
        const GLIDE_BAND_TOL: f32 = 18.0; // slightly wider to reduce "Other"

        const GLIDE_BACK_H: f32 = 80.0;
        const GLIDE_NEUTRAL_H: f32 = 294.0;
        const GLIDE_FWD_H: f32 = 390.0;

        // Falling
        const FALL_MIN_VZ: f32 = 300.0;
        const FALL_VERTICAL_DOM_RATIO: f32 = 1.4;
        const TERMINAL_VZ: f32 = 900.0;

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
        fn snap_glide(h: f32) -> Movement {
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

            // Idle
            if h < IDLE_H_MAX && abs_vz < 5.0 {
                return Movement::Idle;
            }

            // Falling
            if vz <= -FALL_MIN_VZ && abs_vz >= FALL_VERTICAL_DOM_RATIO * (h + 1.0) {
                return if abs_vz >= TERMINAL_VZ {
                    Movement::FallingTerminal
                } else {
                    Movement::Falling
                };
            }

            // --- Forward projection (helps “RunForward”) ---
            let mut forward_comp = h; // default: no facing, just use h
            let mut fwd_like = false;
            let mut back_like = false;
            let mut strafe_like = false;

            if let Some([fx, fy]) = facing_xy {
                if h > 30.0 {
                    let nhx = s.vx / h;
                    let nhy = s.vy / h;
                    let dot = nhx * fx + nhy * fy; // cos(theta)
                    forward_comp = h * dot.max(-1.0).min(1.0); // signed forward speed
                    fwd_like = dot >= 0.35; // looser than before
                    back_like = dot <= -0.35;
                    strafe_like = !fwd_like && !back_like;
                }
            }

            // Glide: require vertical in [80,150] **and** snap horizontal to nearest band
            if vz < 0.0 && abs_vz >= GLIDE_VZ_MIN && abs_vz <= GLIDE_VZ_MAX {
                let m = snap_glide(h);
                // Only accept if we’re close enough to a band; otherwise let ground logic handle it
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
                // Walk (use 3D to be slope-safe)
                if (speed3d - WALK_H).abs() <= WALK_TOL_H {
                    return Movement::Walk;
                }

                // Backpedal / Strafe by horizontal bands or facing
                if back_like || (h - RUN_BACK_H).abs() <= RUN_BACK_TOL_H {
                    return Movement::Backpedal;
                }
                if strafe_like || (h - RUN_STRAFE_H).abs() <= RUN_STRAFE_TOL_H {
                    return Movement::Strafe;
                }

                // Forward run by **forward component** first; fall back to horizontal bands
                let fwd_targets = [
                    (RUN_FWD_OOC_H, RUN_FWD_TOL_H),
                    (RUN_FWD_IC_H, RUN_FWD_TOL_H),
                ];
                for (t, tol) in fwd_targets {
                    if (forward_comp - t).abs() <= tol {
                        return Movement::RunForward;
                    }
                }
                // No facing or weak dot? Use plain horizontal
                for (t, tol) in fwd_targets {
                    if (h - t).abs() <= tol {
                        return Movement::RunForward;
                    }
                }

                // Last-chance snap to forward if moving decisively forward
                if fwd_like && h > 150.0 {
                    return Movement::RunForward;
                }
            }

            Movement::Other
        }
    }
}

pub struct TemporalClassifier {
    // rolling buffer of (time, Speed)
    hist: VecDeque<(Instant, Speed)>,
    window: Duration, // e.g. 0.3 s
    // sticky state
    state: Movement,
    last_switch: Instant,
    // tunables
    min_dwell_in: Duration,  // must stay in a new state this long to confirm
    min_dwell_out: Duration, // must contradict current state this long to switch
}

impl TemporalClassifier {
    pub fn new(now: Instant) -> Self {
        Self {
            hist: VecDeque::with_capacity(64),
            window: Duration::from_millis(300),
            state: Movement::Idle,
            last_switch: now,
            min_dwell_in: Duration::from_millis(120),
            min_dwell_out: Duration::from_millis(160),
        }
    }

    /// Push latest Speed sample + facing, return **temporal** movement.
    /// Pass facing_xy from your f_avatar_front (remapped) or None.
    pub fn update(&mut self, now: Instant, s: Speed, facing_xy: Option<[f32; 2]>) -> Movement {
        // Helper: same family?
        fn same_family(a: Movement, b: Movement) -> bool {
            use Movement::*;
            match (a, b) {
                (GlideBack, GlideBack)
                | (GlideBack, GlideNeutral)
                | (GlideBack, GlideForward)
                | (GlideNeutral, GlideBack)
                | (GlideNeutral, GlideNeutral)
                | (GlideNeutral, GlideForward)
                | (GlideForward, GlideBack)
                | (GlideForward, GlideNeutral)
                | (GlideForward, GlideForward) => true,
                _ => false,
            }
        }

        // 1) push + trim window
        self.hist.push_back((now, s));
        while let Some((t0, _)) = self.hist.front() {
            if now.duration_since(*t0) > self.window {
                self.hist.pop_front();
            } else {
                break;
            }
        }

        // 2) compute rolling averages over the window
        let mut sum_h = 0.0;
        let mut sum_vz = 0.0;
        let mut sum_3d = 0.0;
        let mut sum_vx = 0.0;
        let mut sum_vy = 0.0;
        let n = self.hist.len().max(1) as f32;
        for (_, sp) in &self.hist {
            sum_h += sp.horizontal;
            sum_vz += sp.vz;
            sum_3d += sp.magnitude;
            sum_vx += sp.vx;
            sum_vy += sp.vy;
        }
        let avg = Speed {
            vx: sum_vx / n,
            vy: sum_vy / n,
            vz: sum_vz / n,
            horizontal: sum_h / n,
            magnitude: sum_3d / n,
            dt_s: s.dt_s, // not used here
        };

        // 3) majority vote across last K samples (helps cut “Other” noise)
        let k = 5usize.min(self.hist.len());
        let mut counts = std::collections::HashMap::<Movement, usize>::new();
        for (_, sp) in self.hist.iter().rev().take(k) {
            let m = classify::classify(sp, facing_xy);
            *counts.entry(m).or_default() += 1;
        }
        let mut vote = Movement::Other;
        let mut best = 0usize;
        for (m, c) in counts {
            if c > best {
                best = c;
                vote = m;
            }
        }

        // 4) classify on the **averaged** sample as well
        let avg_label = classify::classify(&avg, facing_xy);

        // 5) combine: if vote and avg agree, that’s our “proposed” state.
        // otherwise, prefer avg_label but mark as weak.
        let proposed = if vote == avg_label {
            avg_label
        } else {
            avg_label
        };

        // 6) hysteresis — only switch if contradiction persists
        let now_state = self.state;
        let proposed = {
            // If avg looks glide-ish but not perfectly in-band, snap to nearest glide subtype
            use Movement::*;
            match avg_label {
                GlideBack | GlideNeutral | GlideForward => avg_label,
                _ => {
                    // If we're currently gliding and vote says glide-ish, don’t drop to Other
                    if same_family(now_state, vote) {
                        vote
                    } else {
                        avg_label
                    }
                }
            }
        };

        // Hysteresis
        if proposed != now_state {
            // If switching within Glide family: very cheap switch (no dwell), and never via Other
            if same_family(now_state, proposed) {
                if proposed != Movement::Other {
                    self.state = proposed;
                    self.last_switch = now;
                }
            } else {
                // normal dwell logic
                let since = now.duration_since(self.last_switch);
                let mut proposed_time = std::time::Duration::ZERO;
                let mut last_t = now;
                for (t, sp) in self.hist.iter().rev() {
                    let lbl = crate::movement_speed::classify::classify(sp, facing_xy);
                    if lbl == proposed {
                        proposed_time += last_t.saturating_duration_since(*t);
                        last_t = *t;
                    } else {
                        break;
                    }
                }

                if proposed_time >= self.min_dwell_in
                    && since >= std::time::Duration::from_millis(80)
                {
                    self.state = proposed;
                    self.last_switch = now;
                }
            }
        }

        // Extra: if we are in a Glide state and proposed == Other, keep current
        if matches!(
            self.state,
            Movement::GlideBack | Movement::GlideNeutral | Movement::GlideForward
        ) {
            // do nothing; sticky within glide to avoid "Other" blips
        }

        self.state
    }
}

fn main() -> Result<(), String> {
    let link = MumbleLink::new()?;
    let mut calc = SpeedCalculator::new();
    let mut temporal = TemporalClassifier::new(Instant::now());

    loop {
        if let Some(mem) = link.read() {
            // mem.f_avatar_position is (x, z, y) in meters
            let now = Instant::now();
            if let Some(s) = calc.step(mem.f_avatar_position, now) {
                let facing_xy = classify::facing_xy_from_front(mem.f_avatar_front); // xzy→xyz
                let mv = temporal.update(now, s, Some(facing_xy));
                println!("avg h={:.1} vz={:.1} ⇒ {:?}", s.horizontal, s.vz, mv);
            }
        }
        std::thread::sleep(Duration::from_millis(40)); // ~25 Hz
    }
}
