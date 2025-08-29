#![cfg(windows)]

use std::sync::Arc;
use std::{thread, time::Duration};

use crossbeam_channel::{Receiver as CbReceiver, bounded, select, tick};

use streamdeck_lib::prelude::*;

use crate::gw2::mumble::MumbleLink;
use crate::gw2::shared::ActiveChar;
use crate::topics::MUMBLE_ACTIVE_CHARACTER;

/// Publishes:
/// - "mumble.combat"           -> bool
/// - "mumble.active-character" -> String  (only emitted in SLOW mode)
///
/// Listens:
/// - "mumble.fast"             -> ~16ms polling, combat only
/// - "mumble.slow"             -> ~10s polling, parses identity too
pub struct MumbleAdapter;

impl MumbleAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for MumbleAdapter {
    fn name(&self) -> &'static str {
        "gw2.mumble_adapter"
    }

    fn policy(&self) -> StartPolicy {
        StartPolicy::OnAppLaunch
    }

    fn topics(&self) -> &'static [&'static str] {
        &[]
    }

    fn start(
        &self,
        cx: &Context,
        bus: Arc<dyn Bus>,
        inbox: CbReceiver<Arc<ErasedTopic>>,
    ) -> AdapterResult {
        let (stop_tx, stop_rx) = bounded::<()>(1);
        let logger = cx.log().clone();
        let active_char_ext = cx
            .try_ext::<ActiveChar>()
            .ok_or(AdapterError::Init("ActiveChar extension not found".into()))?
            .clone();

        let join = thread::spawn(move || {
            // Tickers
            let ticker = tick(Duration::from_secs(10));
            // mapping
            let mut link: Option<MumbleLink> = None;

            // de-dupe
            let mut last_name: Option<String> = None;

            info!(logger, "🎧 Mumble adapter started (slow)");

            loop {
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(_) => {}
                            Err(_) => break, // inbox closed
                        }
                    }

                    recv(stop_rx) -> _ => {
                        debug!(logger, "Stopping Mumble adapter...");
                        break;
                    }

                    recv(ticker) -> _ => {
                        // Ensure mapping
                        if link.is_none() {
                            match MumbleLink::new() {
                                Ok(l) => {
                                    info!(logger, "✅ MumbleLink mapped");
                                    link = Some(l);
                                    last_name = None; // force re-emit

                                }
                                Err(e) => {
                                    warn!(logger, "⚠️ MumbleLink open failed: {} (retrying)", e);
                                    continue;
                                }
                            }
                        }

                        // Read: identity only in slow mode
                        if let Some(l) = link.as_ref() {
                            if let Some((_, ident)) = l.read_full(true) {
                                if let Some(id) = ident {
                                    let name = id.name.trim();
                                    if !name.is_empty() {
                                        if last_name.as_deref() != Some(name) {
                                            last_name = Some(name.to_string());
                                            active_char_ext.set(Some(name.into()));
                                            bus.publish_t(MUMBLE_ACTIVE_CHARACTER, Some(name.into()));
                                        }
                                    } else if last_name.is_some() {
                                        last_name = None;
                                        active_char_ext.set(None);
                                        bus.publish_t(MUMBLE_ACTIVE_CHARACTER, None);
                                    }
                                }
                            } else {
                                // read failed -> drop mapping and retry next tick
                                warn!(logger, "⚠️ MumbleLink read failed; remapping");
                                link = None;
                            }
                        }
                    }
                }
            }

            if link.take().is_some() {
                debug!(logger, "Unmapping MumbleLink on shutdown");
            }
            info!(logger, "🛑 Mumble adapter stopped");
        });

        Ok(AdapterHandle::from_crossbeam(join, stop_tx))
    }
}
