use std::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicBool};

use crate::PLUGIN_ID;
use crate::topics::GW2_BINDINGS_PATH_SET;
use constcat::concat;
use streamdeck_lib::prelude::*;

const LONG_PRESS: std::time::Duration = std::time::Duration::from_secs(5);

/// Notifies:
/// "gw2.bindings_watcher" -> "bindings.updated" when bindings are updated
/// Listens:
/// None, this action does not listen to any events
#[derive(Default)]
pub struct SettingsAction {
    hold_cancel: Option<Arc<AtomicBool>>,
    hold_fired: bool,
}

impl ActionStatic for SettingsAction {
    const ID: &'static str = concat!(PLUGIN_ID, ".settings");
}

impl Action for SettingsAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn property_inspector_did_appear(&mut self, cx: &Context, ev: &PropertyInspectorDidAppear) {
        debug!(
            cx.log(),
            "SettingsAction PI appeared for context: {}", ev.context
        );
        let settings = cx.globals().snapshot();
        cx.sd().set_settings(ev.context, settings);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &DidReceiveSettings) {
        debug!(
            cx.log(),
            "Received settings for context {}: {:?}", ev.context, ev.settings
        );

        cx.globals().with_mut(|globals| {
            // Update globals with the new settings
            for (k, v) in ev.settings.iter() {
                if v.is_null() {
                    globals.remove(k);
                } else {
                    globals.insert(k.clone(), v.clone());
                }
            }
        });

        // If bindings_file changed, tell the watcher adapter
        let bus = cx.bus();
        if let Some(path) = ev.settings.get("bindings_file").and_then(|v| v.as_str()) {
            // Notify adapters in the OnAppLaunch group (where the watcher lives)
            bus.publish_t(GW2_BINDINGS_PATH_SET, path.into());
        }
    }

    fn key_down(&mut self, cx: &Context, ev: &KeyDown) {
        debug!(
            cx.log(),
            "SettingsAction key_down for ctx_id: {}", ev.context
        );
        self.hold_fired = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.hold_cancel = Some(cancel.clone());

        let cx = cx.clone();
        let ctx_id = ev.context.to_string();

        std::thread::spawn(move || {
            std::thread::sleep(LONG_PRESS);
            if !cancel.load(Ordering::SeqCst) {
                cx.globals().delete_many(&["api_key", "bindings_file"]);
            }

            cx.sd().show_ok(&ctx_id);
            debug!(
                cx.log(),
                "Long press action completed for context: {}", ctx_id
            );
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &KeyUp) {
        info!(cx.log(), "SettingsAction key_up for ctx_id: {}", ev.context);
        if let Some(cancel) = self.hold_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
    }
}
