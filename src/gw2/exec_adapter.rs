#![cfg(windows)]

use crossbeam_channel::{Receiver as CbReceiver, bounded, select};
use std::{collections::VecDeque, sync::Arc, thread, time::Duration};

use streamdeck_lib::prelude::*;

use crate::gw2::shared::{InCombat, SharedBindings};
use crate::topics::{GW2_EXEC_QUEUE, Gw2ExecQueue, MUMBLE_FAST, MUMBLE_SLOW};

// Use the Windows synth (or swap behind a feature if you want)
use streamdeck_lib::input::WinSynth;

struct Job {
    req: Gw2ExecQueue,
    /// Pre-expanded steps; built when the job is enqueued so we can log errors early.
    steps: Vec<streamdeck_lib::prelude::InputStep>,
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
        let in_combat = cx
            .try_ext::<InCombat>()
            .ok_or(AdapterError::Init("InCombat extension not found".into()))?;

        let logger = cx.log().clone();

        let join = thread::spawn(move || {
            let synth = WinSynth::new();
            let mut queue: VecDeque<Job> = VecDeque::new();
            let mut fast_sent = false; // whether we’ve told Mumble to go FAST

            info!(logger, "GW2 exec adapter started");

            // Helper: toggle Mumble fast/slow according to queue emptiness
            let mut refresh_mumble_mode = |queue_len: usize| {
                let want_fast = queue_len > 0;
                if want_fast && !fast_sent {
                    debug!(logger, "exec: queue non-empty -> mumble FAST");
                    bus.adapters_notify_topic_t(MUMBLE_FAST, None, ());
                    fast_sent = true;
                } else if !want_fast && fast_sent {
                    debug!(logger, "exec: queue empty -> mumble SLOW");
                    bus.adapters_notify_topic_t(MUMBLE_SLOW, None, ());
                }
            };

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
                Some(job)
            };

            loop {
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(note) => {
                                if let Some(t) = note.downcast(GW2_EXEC_QUEUE) {
                                    if let Some(job) = handle_enqueue(GW2_EXEC_QUEUE.name, t.clone()) {
                                        let was_empty = queue.is_empty();
                                        queue.push_back(job);
                                        if was_empty { refresh_mumble_mode(queue.len()); }
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
                        // Nothing queued? ensure slow mode and idle.
                        if queue.is_empty() {
                            refresh_mumble_mode(0);
                            continue;
                        }

                        // Peek the front job; enforce combat rule
                        let can_run_now = {
                            let front = queue.front().unwrap();
                            front.req.allow_in_combat || !in_combat.get()
                        };

                        if !can_run_now {
                            // Keep FAST while we’re waiting (there’s pending work)
                            refresh_mumble_mode(queue.len());
                            continue;
                        }

                        // Pop and run this job to completion
                        let job = queue.pop_front().unwrap();
                        refresh_mumble_mode(queue.len() + 1); // include this in-flight job

                        for step in &job.steps {
                            // Let stop take precedence
                            if stop_rx.try_recv().is_ok() {
                                debug!(logger, "exec: stop requested during job; aborting");
                                return;
                            }
                            if let Err(e) = synth.send_step(step) {
                                warn!(logger, "exec: send_step failed: {e}");
                            }
                        }

                        // After finishing this job, if queue is now empty, drop back to SLOW
                        if queue.is_empty() {
                            refresh_mumble_mode(0);
                        }
                    }
                }
            }

            // On shutdown, ensure SLOW
            if fast_sent {
                bus.adapters_notify_topic_t(MUMBLE_SLOW, None, ());
            }
            info!(logger, "GW2 exec adapter stopped");
        });

        Ok(AdapterHandle::from_crossbeam(join, stop_tx))
    }
}
