#![cfg(windows)]

use crossbeam_channel::{Receiver as CbReceiver, bounded, select};
use std::time::Instant;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use streamdeck_lib::prelude::*;

use crate::gw2::airborne::{AirClassifier, classify::Movement};
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

            // --- Airborne worker: shared snapshot + thread ---
            #[derive(Copy, Clone)]
            struct AirSnapshot {
                state: Movement,
                in_air: bool,
                landed_recently: bool,
            }
            impl Default for AirSnapshot {
                fn default() -> Self {
                    Self {
                        state: Movement::Idle,
                        in_air: false,
                        landed_recently: false,
                    }
                }
            }

            let air_snapshot: Arc<Mutex<AirSnapshot>> =
                Arc::new(Mutex::new(AirSnapshot::default()));

            // clone handles for the worker
            let air_snapshot_worker = Arc::clone(&air_snapshot);
            let stop_rx_air = stop_rx.clone();
            let logger_air = logger.clone();

            thread::spawn(move || {
                let mut air = AirClassifier::new(Instant::now());
                let mut link: Option<MumbleLink> = None;

                loop {
                    if stop_rx_air.try_recv().is_ok() {
                        debug!(logger_air, "airborne worker: stop received");
                        break;
                    }

                    // ensure link
                    if link.is_none() {
                        match MumbleLink::new() {
                            Ok(l) => {
                                info!(logger_air, "airborne worker: MumbleLink mapped");
                                link = Some(l);
                            }
                            Err(e) => {
                                // back off a bit if mapping fails
                                warn!(logger_air, "airborne worker: map failed: {e}");
                                thread::sleep(Duration::from_millis(200));
                                continue;
                            }
                        }
                    }

                    if let Some(ref l) = link {
                        let state = air.update_with(l); // runs classifier
                        let snap = AirSnapshot {
                            state,
                            in_air: air.is_airborne(),
                            landed_recently: air.landed_recently(),
                        };
                        if let Ok(mut guard) = air_snapshot_worker.lock() {
                            *guard = snap;
                        }
                    }

                    // ~25 Hz (Mumble updates ~25 Hz)
                    thread::sleep(Duration::from_millis(40));
                }
            });

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
                        if queue.is_empty() {
                            continue;
                        }

                        // Drive UI animation pulse (~3.3 fps)
                        if last_tick.elapsed() >= tick_every {
                            last_tick = std::time::Instant::now();
                            bus.publish_t(crate::topics::GW2_ANIMATION_TICK, ());
                        }


                        // Read latest airborne snapshot (produced by worker at ~25 Hz)
                        let (_mv, in_air, landing_grace) = {
                            let g = air_snapshot.lock().unwrap_or_else(|p| p.into_inner());
                            (g.state, g.in_air, g.landed_recently)
                        };

                        // Fresh combat read
                        let in_combat = combat.in_combat_now(&logger);

                        let front = queue.front().unwrap();

                        let ok_combat = if in_combat {
                            front.req.allow_in_combat
                        } else {
                            front.req.allow_out_of_combat
                        };

                        // Block when airborne OR in landing grace, unless explicitly allowed.
                        let air_blocked = in_air || landing_grace;
                        let ok_air = if air_blocked {
                            front.req.allow_gliding_or_falling
                        } else {
                            true
                        };

                        let can_run_now = ok_combat && ok_air;

                        // debug!(
                        //     logger,
                        //     "exec: queue={}, allow_in_combat={}, allow_out_of_combat={}, allow_glide={}, combat={}, mv={:?}, in_air={}, landed_recently={}, run={}",
                        //     queue.len(),
                        //     front.req.allow_in_combat,
                        //     front.req.allow_out_of_combat,
                        //     front.req.allow_gliding_or_falling,
                        //     combat.last_state,
                        //     mv,
                        //     in_air,
                        //     landing_grace,
                        //     can_run_now
                        // );


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
