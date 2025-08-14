// src/gw2/bindings_adapter.rs
use std::{ fs, path::PathBuf, thread, time::Duration };
use crossbeam_channel::{ bounded, select, Receiver as CbReceiver };
use notify::{ Event, RecommendedWatcher, RecursiveMode, Watcher };
use streamdeck_lib::{
    adapters::{ Adapter, AdapterHandle, AdapterNotify, StartPolicy },
    context::Context,
    logger::Level,
    bus::{ Bus },
};

use crate::gw2::{ binds::BindingSet, shared::SharedBindings };

pub struct Gw2BindingsAdapter;

impl Gw2BindingsAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for Gw2BindingsAdapter {
    fn name(&self) -> &'static str {
        "gw2.bindings_watcher"
    }

    fn policy(&self) -> StartPolicy {
        StartPolicy::OnAppLaunch
    }

    fn topics(&self) -> &'static [&'static str] {
        &["bindings-path.set", "bindings-path.reload"]
    }

    fn start(
        &self,
        cx: &Context,
        bus: std::sync::Arc<dyn Bus>,
        inbox: CbReceiver<AdapterNotify>
    ) -> Result<AdapterHandle, String> {
        // Channel to stop the worker
        let (stop_tx, stop_rx) = bounded::<()>(1);

        // Grab initial path from globals, if present
        let initial_path: Option<PathBuf> = cx
            .globals()
            .snapshot()
            .get("bindings_file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let shared_binds = match cx.try_ext::<SharedBindings>() {
            Some(b) => b,
            None => {
                return Err("SharedBindings extension not found".to_string());
            }
        };
        let logger = cx.log().clone();
        let cx = cx.clone();

        let join = thread::spawn(move || {
            // State
            let mut watched_path: Option<PathBuf> = initial_path;
            let (notify_tx, notify_rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
            let mut watcher: Option<RecommendedWatcher> = None;

            // helper: (re)configure watcher
            let mut rewatch = |path: &PathBuf| {
                // Drop old watcher first
                watcher.take();
                match notify::recommended_watcher(notify_tx.clone()) {
                    Ok(mut w) => {
                        if let Err(e) = w.watch(path, RecursiveMode::NonRecursive) {
                            bus.log(format!("watch error: {e}"), Level::Warn);
                        } else {
                            watcher = Some(w);
                            bus.log(format!("watching file: {}", path.display()), Level::Info);
                        }
                    }
                    Err(e) => bus.log(format!("create watcher failed: {e}"), Level::Error),
                }
            };

            // helper to update bindings from file
            let update_bindings_from_file = |path: &PathBuf| {
                bus.log(
                    format!("Updating bindings from file: {}", path.display()),
                    Level::Info
                );
                let mut new_binds = BindingSet::with_default();
                if let Ok(content) = fs::read_to_string(path) {
                    new_binds.patch_from_xml(content.as_str(), logger.clone());
                } else {
                    bus.log(
                        format!("Failed to read bindings file: {}", path.display()),
                        Level::Error
                    );
                }

                match shared_binds.replace_bindings(new_binds) {
                    Ok(_) => {
                        bus.log("Bindings updated successfully.".to_string(), Level::Info);
                        // Optionally write to globals if needed
                        if let Err(e) = shared_binds.write_to_globals(cx.globals(), cx.sd()) {
                            bus.log(
                                format!("Failed to write bindings to globals: {}", e),
                                Level::Error
                            );
                        }
                        bus.action_notify_all("bindings.updated".to_string(), None);
                    }
                    Err(e) => {
                        bus.log(format!("Failed to replace bindings: {}", e), Level::Error);
                    }
                }
            };

            bus.log(format!("Watched Path: {:?}", watched_path), Level::Debug);
            // Kick off if we had a path at boot
            if let Some(p) = watched_path.clone() {
                update_bindings_from_file(&p);
                rewatch(&p);
            }

            bus.log("Bindings watcher started.".to_string(), Level::Info);
            // Bridge std::sync::mpsc (notify) with crossbeam select via try_recv
            loop {
                // 1) Handle adapter inbox
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(note) => {
                                match note.topic.as_str() {
                                    "bindings-path.set" => {
                                        if let Some(v) = note.data.as_ref().and_then(|v| v.as_str()) {
                                            let p = PathBuf::from(v);
                                            watched_path = Some(p.clone());
                                            update_bindings_from_file(&p);
                                            rewatch(&p);
                                        }
                                    }
                                    "bindings-path.reload" => {
                                        if let Some(p) = watched_path.clone() {
                                            update_bindings_from_file(&p);
                                            rewatch(&p);
                                        }
                                    }
                                    _ => { /* ignore */ }
                                }
                            }
                            Err(_) => break, // inbox closed
                        }
                    }
                    recv(stop_rx) -> _ => {
                        bus.log("Stopping bindings watcher...".to_string(), Level::Debug);
                        break;
                     }
                    default(Duration::from_millis(100)) => {
                        // 2) Poll notify events
                        match notify_rx.try_recv() {
                            Ok(Ok(_)) => {
                                bus.log("Bindings file changed, reloading...".to_string(), Level::Info);
                                update_bindings_from_file(&watched_path.as_ref().unwrap());
                            }
                            Ok(Err(e)) => {
                                bus.log(format!("notify error: {e}"), Level::Warn);
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                // No events, continue
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                bus.log("notify channel disconnected".into(), Level::Warn);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(AdapterHandle {
            join: Some(join),
            shutdown: Box::new(move || {
                let _ = stop_tx.send(());
            }),
        })
    }
}
