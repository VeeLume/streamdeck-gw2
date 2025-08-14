use std::sync::atomic::{ AtomicBool, Ordering };
use std::sync::Arc;
use std::time::{ Duration, Instant };
use constcat::concat;
use serde_json::Map;

use crate::app::context::AppCtx;
use crate::core::{ events::Command, sd_client::SdClient };
use crate::infra::sd_protocol::StreamDeckEvent;
use crate::plugin::PLUGIN_UUID;

use super::super::action::{ Action, ActionFactory };

const LONG_PRESS: Duration = Duration::from_secs(5);

pub struct SettingsAction {
    // armed hold (exists between KeyDown and either timer firing or KeyUp)
    hold_cancel: Option<Arc<AtomicBool>>,
    // if the long-press already triggered, we skip doing anything at KeyUp
    hold_fired: bool,
}

impl SettingsAction {
    pub fn new() -> Self {
        Self { hold_cancel: None, hold_fired: false }
    }
}

impl Default for SettingsAction {
    fn default() -> Self {
        Self::new()
    }
}

impl Action for SettingsAction {
    fn on_event(
        &mut self,
        sd: &SdClient,
        ctx: &AppCtx,
        ev: &StreamDeckEvent,
        out: &mut Vec<Command>
    ) {
        match ev {
            // Prefill local (per-action) settings when PI opens
            StreamDeckEvent::PropertyInspectorDidAppear { context, .. } => {
                let settings = ctx.global_settings.get().clone();
                sd.set_settings(context, settings);
            }

            // PI saved changes: mirror to globals and refresh
            StreamDeckEvent::DidReceiveSettings { context, settings, .. } => {
                out.push(
                    Command::Log(
                        format!(
                            "📥 Settings PI saved: context = {context}, settings = {:?}",
                            settings
                        )
                    )
                );
                let mut patch = serde_json::Map::new();
                if let Some(v) = settings.get("api_key").cloned() {
                    patch.insert("api_key".into(), v);
                }
                if let Some(v) = settings.get("bindings").cloned() {
                    patch.insert("bindings".into(), v);
                }

                let payload = ctx.global_settings.merge(patch);
                sd.set_global_settings(payload);

                sd.show_ok(context);
            }

            // Arm the one-shot long-press
            StreamDeckEvent::KeyDown { context, .. } => {
                self.hold_fired = false;

                let cancel = Arc::new(AtomicBool::new(false));
                self.hold_cancel = Some(cancel.clone());

                // we need an SdClient + context we can move into the thread
                let sd_cloned = sd.clone();
                let context_id = context.clone();
                let ctx_clone = ctx.clone();

                std::thread::spawn(move || {

                    // sleep once; if not canceled, fire the wipe
                    std::thread::sleep(LONG_PRESS);
                    if !cancel.load(Ordering::SeqCst) {
                        let payload = ctx_clone.global_settings.remove_keys([
                            "api_key",
                            "bindings",
                        ]);
                        sd_cloned.set_settings(&context_id, Map::new());

                        sd_cloned.show_ok(&context_id);
                    }
                });
            }

            // On release: cancel if not yet fired; if it already fired, do nothing
            StreamDeckEvent::KeyUp { .. } => {
                if let Some(cancel) = self.hold_cancel.take() {
                    // if timer hasn’t run yet, cancel it
                    cancel.store(true, Ordering::SeqCst);
                }
            }

            _ => {}
        }
    }
}

pub struct SettingsFactory;
impl ActionFactory for SettingsFactory {
    fn kind(&self) -> &'static str {
        concat!(PLUGIN_UUID, ".settings")
    }
    fn create(&self) -> Box<dyn super::Action> {
        Box::new(SettingsAction::new())
    }
}
pub static SETTINGS: SettingsFactory = SettingsFactory;
