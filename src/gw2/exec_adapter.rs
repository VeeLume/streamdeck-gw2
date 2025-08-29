#![cfg(windows)]

use crossbeam_channel::{Receiver as CbReceiver, bounded, select};
use std::{collections::VecDeque, sync::Arc, thread, time::Duration};

use streamdeck_lib::prelude::*;

use crate::gw2::mumble::MumbleLink;
use crate::gw2::shared::SharedBindings;
use crate::topics::{ExecState, GW2_EXEC_PROGRESS, GW2_EXEC_QUEUE, Gw2ExecQueue};

// Use the Windows synth (or swap behind a feature if you want)
use streamdeck_lib::input::WinSynth;

struct Job {
    req: Gw2ExecQueue,
    /// Pre-expanded steps; built when the job is enqueued so we can log errors early.
    steps: Vec<streamdeck_lib::prelude::InputStep>,
}

struct CombatSensor {
    link: Option<MumbleLink>,
    last_tick: u32,
    last_state: bool,
    last_read_at: std::time::Instant,
}
impl CombatSensor {
    fn new() -> Self {
        Self {
            link: None,
            last_tick: 0,
            last_state: false,
            last_read_at: std::time::Instant::now() - Duration::from_secs(10),
        }
    }
    fn ensure(&mut self, log: &Arc<dyn ActionLog>) {
        if self.link.is_none() {
            match MumbleLink::new() {
                Ok(l) => {
                    info!(log, "exec: MumbleLink mapped");
                    self.link = Some(l);
                }
                Err(e) => {
                    warn!(log, "exec: MumbleLink map failed: {e}");
                }
            }
        }
    }
    fn in_combat_now(&mut self, log: &Arc<dyn ActionLog>) -> bool {
        self.ensure(log);
        let read_once = |this: &mut Self| -> Option<(bool, u32)> {
            let (ui, tick) = this.link.as_ref()?.read_ui()?;
            Some((ui.is_in_combat(), tick))
        };

        if let Some((state, tick)) = read_once(self) {
            // if ui_tick hasn't advanced and our last read is old, retry once to avoid stale frame
            let stale =
                self.last_tick == tick && self.last_read_at.elapsed() > Duration::from_millis(32);
            if stale {
                std::thread::sleep(Duration::from_millis(20));
                if let Some((s2, t2)) = read_once(self) {
                    self.last_state = s2;
                    self.last_tick = t2;
                    self.last_read_at = std::time::Instant::now();
                    return s2;
                }
            }
            self.last_state = state;
            self.last_tick = tick;
            self.last_read_at = std::time::Instant::now();
            return state;
        }
        // fall back to last seen (default false). Flip to `true` if you prefer “fail closed”.
        self.last_state
    }
}

struct AirSensor {
    link: Option<MumbleLink>,
    // (t, z) samples within a sliding window
    samples: VecDeque<(std::time::Instant, f32)>,
    last_tick: u32,

    airborne: bool,
    last_change: std::time::Instant,

    // tuning
    sample_every: std::time::Duration, // min time between kept samples
    window: std::time::Duration,       // averaging window
    min_glide_window: std::time::Duration,

    hard_v: f32,           // |v_inst| > hard_v => airborne (jump/fall)
    glide_speed_mag: f32,  // v_avg < glide_v   => airborne (glide)
    settle_ms: u64,        // must be stable this long before clearing
    landing_grace_ms: u64, // extra time to stay airborne after landing (not implemented)

    // logging
    log_every: std::time::Duration,
    last_log: std::time::Instant,
    log_enabled: bool,
}

impl AirSensor {
    fn new() -> Self {
        Self {
            link: None,
            samples: VecDeque::with_capacity(16),
            last_tick: 0,
            airborne: false,
            last_change: std::time::Instant::now(),
            // Defaults tuned for GW2 feel; we’ll confirm with logs.
            sample_every: std::time::Duration::from_millis(70),
            window: std::time::Duration::from_millis(900),
            min_glide_window: std::time::Duration::from_millis(600),
            hard_v: 8.0,
            glide_speed_mag: 0.9,
            settle_ms: 250,
            landing_grace_ms: 300,

            log_every: std::time::Duration::from_millis(250),
            last_log: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(5))
                .unwrap(),
            log_enabled: true, // flip to false when you’re done tuning
        }
    }

    /// True for a short time right after we transitioned to GROUND.
    fn landed_recently(&self) -> bool {
        !self.airborne && self.last_change.elapsed().as_millis() as u64 <= self.landing_grace_ms
    }

    fn ensure_link(&mut self, log: &Arc<dyn ActionLog>) {
        if self.link.is_none() {
            match MumbleLink::new() {
                Ok(l) => {
                    info!(log, "exec: MumbleLink mapped for air sensor");
                    self.link = Some(l);
                }
                Err(e) => {
                    warn!(log, "exec: MumbleLink map failed: {e}");
                }
            }
        }
    }

    /// Call every loop; cheap. Returns current airborne state.
    fn update(&mut self, log: &Arc<dyn ActionLog>) -> bool {
        self.ensure_link(log);
        let now = std::time::Instant::now();

        // read z + ui_tick (implement read_z_ui() in gw2::mumble)
        let (z, tick) = match self.link.as_ref().and_then(|l| l.read_z_ui()) {
            Some(v) => v,
            None => return self.airborne,
        };

        // stale-frame retry: if ui_tick didn't advance for a while
        if let Some((last_t, _)) = self.samples.back() {
            if self.last_tick == tick && last_t.elapsed() > 2 * self.sample_every {
                std::thread::sleep(std::time::Duration::from_millis(20));
                if let Some((z2, tick2)) = self.link.as_ref().and_then(|l| l.read_z_ui()) {
                    self.last_tick = tick2;
                    self.push_sample(now, z2);
                    return self.eval_and_maybe_log(log);
                }
            }
        }

        self.last_tick = tick;

        // throttle sampling into the window
        let push = match self.samples.back() {
            None => true,
            Some((last_t, _)) => last_t.elapsed() >= self.sample_every,
        };
        if push {
            self.push_sample(now, z);
        }

        self.eval_and_maybe_log(log)
    }

    fn push_sample(&mut self, now: std::time::Instant, z: f32) {
        self.samples.push_back((now, z));
        // prune old samples outside the window
        while let Some((t0, _)) = self.samples.front() {
            if now.duration_since(*t0) > self.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn eval_and_maybe_log(&mut self, log: &Arc<dyn ActionLog>) -> bool {
        // compute metrics + decision
        let (v_inst, v_avg_opt, win_ms, spike, glide_avg, settle_hold, airborne_now) =
            self.eval_internal();

        let state_changed = airborne_now != self.airborne;
        if state_changed {
            self.airborne = airborne_now;
            self.last_change = std::time::Instant::now();
            if self.log_enabled {
                info!(
                    log,
                    "air: state={} v_inst={:.2} u/s v_avg={} win={}ms spike={} glide={} settle={} (thr |hard|>{:.1}, |glide|>{:.2})",
                    if self.airborne { "AIRBORNE" } else { "GROUND" },
                    v_inst,
                    v_avg_opt
                        .map(|v| format!("{v:.2}"))
                        .unwrap_or_else(|| "-".into()),
                    win_ms,
                    spike,
                    glide_avg,
                    settle_hold,
                    self.hard_v,
                    self.glide_speed_mag
                );
            }
        } else if self.log_enabled && self.last_log.elapsed() >= self.log_every {
            self.last_log = std::time::Instant::now();
            debug!(
                log,
                "air: hold={} v_inst={:.2} u/s v_avg={} win={}ms spike={} glide={} settle={}",
                self.airborne,
                v_inst,
                v_avg_opt
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "-".into()),
                win_ms,
                spike,
                glide_avg,
                settle_hold
            );
        }

        self.airborne
    }

    /// Returns (v_inst, v_avg_opt, win_ms, spike, glide_avg, settle_hold, airborne_now)
    fn eval_internal(&self) -> (f32, Option<f32>, u64, bool, bool, bool, bool) {
        if self.samples.len() < 2 {
            return (0.0, None, 0, false, false, false, self.airborne);
        }

        let (t_last, z_last) = *self.samples.back().unwrap();
        let (t_prev, z_prev) = self.samples.iter().rev().nth(1).unwrap();

        let dt_inst = t_last.duration_since(*t_prev).as_secs_f32().max(1e-6);
        let v_inst = (z_last - z_prev) / dt_inst; // +up, -down

        // 1) Spike detection (jump/fall)
        let spike = v_inst.abs() > self.hard_v;

        // 2) Glide detection: average over window
        let (t_first, z_first) = *self.samples.front().unwrap();
        let dt_win = t_last.duration_since(t_first);
        let mut v_avg_opt: Option<f32> = None;
        let mut glide_avg = false;
        if dt_win >= self.min_glide_window {
            let v_avg = (z_last - z_first) / dt_win.as_secs_f32();
            v_avg_opt = Some(v_avg);
            // sustained |vertical| speed (but not a spike) => glide
            if v_avg.abs() > self.glide_speed_mag && !spike {
                glide_avg = true;
            }
        }

        // 2b) Landing clamp: if both speeds are ~zero, do NOT glide
        let eps = 0.05_f32; // tune 0.03–0.08 if needed
        let zeroish = v_inst.abs() < eps && v_avg_opt.map(|v| v.abs() < eps).unwrap_or(true);
        if zeroish {
            glide_avg = false;
        }

        // 3) Settle before clearing
        let settle_hold = if !spike && !glide_avg && self.airborne {
            self.last_change.elapsed().as_millis() as u64 <= self.settle_ms
        } else {
            false
        };

        let airborne_now = spike || glide_avg || settle_hold;

        (
            v_inst,
            v_avg_opt,
            dt_win.as_millis() as u64,
            spike,
            glide_avg,
            settle_hold,
            airborne_now,
        )
    }

    fn is_airborne(&self) -> bool {
        self.airborne
    }
}

pub struct Gw2ExecAdapter;

impl Gw2ExecAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for Gw2ExecAdapter {
    fn name(&self) -> &'static str {
        "gw2.exec_adapter"
    }

    fn policy(&self) -> StartPolicy {
        StartPolicy::OnAppLaunch
    }

    fn topics(&self) -> &'static [&'static str] {
        // Single ingress for execution requests
        &[GW2_EXEC_QUEUE.name]
    }

    fn start(
        &self,
        cx: &Context,
        bus: Arc<dyn Bus>,
        inbox: CbReceiver<Arc<ErasedTopic>>,
    ) -> AdapterResult {
        // Stop signal
        let (stop_tx, stop_rx) = bounded::<()>(1);

        // Required extensions
        let binds = cx.try_ext::<SharedBindings>().ok_or(AdapterError::Init(
            "SharedBindings extension not found".into(),
        ))?;

        let logger = cx.log().clone();

        let join = thread::spawn(move || {
            let synth = WinSynth::new();
            let mut queue: VecDeque<Job> = VecDeque::new();
            let mut last_tick = std::time::Instant::now();
            let tick_every = Duration::from_millis(300);
            let mut combat = CombatSensor::new();
            let mut air = AirSensor::new();

            info!(logger, "GW2 exec adapter started");

            // Expand an ExecRequest to a Job (steps baked)
            let expand_job = |req: Gw2ExecQueue, binds: &SharedBindings| -> Job {
                use streamdeck_lib::input::dsl::sleep_ms;
                let between = Duration::from_millis(req.inter_control_ms.unwrap_or(35));
                let mut steps: Vec<InputStep> = Vec::new();
                let guard = binds.0.read().ok();
                let binding_set = guard.as_deref();

                for kc in &req.controls {
                    if let Some(bs) = binding_set.and_then(|s| s.get(*kc)) {
                        debug!(logger, "exec: {:?} -> {} binding(s)", kc, bs.len());
                        if let Some(first) = bs.first() {
                            if let Some(mut s) = first.to_steps() {
                                debug!(logger, "exec: {:?} -> {} step(s)", kc, s.len());
                                steps.append(&mut s);
                            } else {
                                warn!(logger, "exec: {:?} -> binding produced no steps", kc);
                            }
                        } else {
                            warn!(logger, "exec: {:?} -> bindings vec empty", kc);
                        }
                    } else {
                        warn!(logger, "exec: no binding for {:?}", kc);
                    }

                    if !steps.is_empty() && between.as_millis() > 0 {
                        steps.push(sleep_ms(between.as_millis() as u64));
                    }
                }

                Job { req, steps }
            };

            let handle_enqueue = |topic: &str, data: Gw2ExecQueue| -> Option<Job> {
                debug!(
                    logger,
                    "exec: received on topic={}, payload={:?}", topic, data
                );
                let job = expand_job(data.clone(), &binds);
                if job.steps.is_empty() {
                    warn!(
                        logger,
                        "exec: job for topic={} had no steps, ignoring", topic
                    );
                    return None;
                }
                debug!(logger, "exec: enqueuing job with {} steps", job.steps.len());
                bus.action_notify_context_t(data.origin_ctx, GW2_EXEC_PROGRESS, ExecState::Queued);
                Some(job)
            };

            loop {
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(note) => {
                                if let Some(t) = note.downcast(GW2_EXEC_QUEUE) {
                                    if let Some(job) = handle_enqueue(GW2_EXEC_QUEUE.name, t.clone()) {
                                        queue.push_back(job);
                                    }
                                }
                            }
                            Err(e) => {
                                error!(logger, "exec: inbox error: {e}");
                                break;
                            }
                        }
                    }

                    recv(stop_rx) -> _ => {
                        debug!(logger, "Stopping GW2 exec adapter...");
                        break;
                    }

                    // Small idle tick to drive execution without busy-waiting
                    default(Duration::from_millis(2)) => {
                        // Drive UI animation pulse (~3.3 fps)
                        if !queue.is_empty() && last_tick.elapsed() >= tick_every {
                            last_tick = std::time::Instant::now();
                            bus.publish_t(crate::topics::GW2_ANIMATION_TICK, ());
                        }

                        if queue.is_empty() {
                            continue;
                        }

                        // Peek the front job; enforce combat rule using a fresh read
                        let in_air = air.update(&logger);
                        let in_combat = combat.in_combat_now(&logger);
                        let landing_grace = air.landed_recently();

                        let can_run_now = {
                            let front = queue.front().unwrap();
                            (front.req.allow_in_combat || !in_combat) &&
                            (front.req.allow_out_of_combat || in_combat) &&
                            (front.req.allow_gliding_or_falling || !in_air || !landing_grace)
                        };

                        debug!(
                            logger,
                            "exec: queue={}, allow_in_combat={}, allow_out_of_combat={}, combat={}, airborne={}, landed_recently={}, run={}",
                            queue.len(),
                            queue.front().unwrap().req.allow_in_combat,
                            queue.front().unwrap().req.allow_out_of_combat,
                            combat.last_state,
                            air.is_airborne(),
                            landing_grace,
                            can_run_now
                        );

                        if !can_run_now {
                            // stay queued; we'll re-check next loop
                            continue;
                        }

                        // Pop and run this job to completion
                        let job = queue.pop_front().unwrap();
                        bus.action_notify_context_t(
                            job.req.origin_ctx.clone(),
                            GW2_EXEC_PROGRESS,
                            ExecState::Started,
                        );

                        for step in &job.steps {
                            if stop_rx.try_recv().is_ok() {
                                debug!(logger, "exec: stop requested during job; aborting");
                                return;
                            }
                            if let Err(e) = synth.send_step(step) {
                                warn!(logger, "exec: send_step failed: {e}");
                            }
                        }

                        bus.action_notify_context_t(
                            job.req.origin_ctx.clone(),
                            GW2_EXEC_PROGRESS,
                            ExecState::Done,
                        );
                    }
                }
            }

            info!(logger, "GW2 exec adapter stopped");
        });

        Ok(AdapterHandle::from_crossbeam(join, stop_tx))
    }
}
