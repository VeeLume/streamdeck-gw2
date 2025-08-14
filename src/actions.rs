use std::sync::{ atomic::{ AtomicBool, Ordering }, Arc };

use constcat::concat;
use streamdeck_lib::{
    actions::Action,
    adapters::StartPolicy,
    context::Context,
    debug,
    info,
    sd_protocol::StreamDeckEvent,
    sd_protocol::views::*,
    warn,
};

use crate::PLUGIN_ID;

#[derive(Default)]
pub struct SetTemplateAction;

impl Action for SetTemplateAction {
    fn id(&self) -> &str {
        concat!(PLUGIN_ID, ".set-template")
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        info!(cx.log(), "SetTemplateAction initialized for context: {}", ctx_id);
    }

    fn will_appear(&mut self, cx: &Context, ev: &WillAppear) {
        info!(cx.log(), "SetTemplateAction will_appear: {:?}", ev.context);
    }

    fn key_down(&mut self, cx: &Context, ev: &KeyDown) {
        debug!(cx.log(), "SetTemplateAction key_down for ctx_id: {}", ev.context);
    }

    fn key_up(&mut self, cx: &Context, ev: &KeyUp) {
        info!(cx.log(), "HelloAction key_up for ctx_id: {}", ev.context);
    }
}

const LONG_PRESS: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Default)]
pub struct SettingsAction {
    hold_cancel: Option<Arc<AtomicBool>>,
    hold_fired: bool,
}

impl Action for SettingsAction {
    fn id(&self) -> &str {
        concat!(PLUGIN_ID, ".settings")
    }

    fn property_inspector_did_appear(&mut self, cx: &Context, ev: &PropertyInspectorDidAppear) {
        info!(cx.log(), "SettingsAction PI appeared for context: {}", ev.context);
        let settings = cx.globals().snapshot();
        cx.sd().set_settings(ev.context, settings);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &DidReceiveSettings) {
        debug!(cx.log(), "Received settings for context {}: {:?}", ev.context, ev.settings);
        // Handle settings update logic here
        let mut patch = serde_json::Map::new();
        for (k, v) in ev.settings.iter() {
            if v.is_null() {
                patch.insert(k.clone(), serde_json::Value::Null);
            } else {
                patch.insert(k.clone(), v.clone());
            }
        }
        cx.globals().merge_and_push(cx.sd(), patch);

        // If bindings_file changed, tell the watcher adapter
        let bus = cx.bus();
        if let Some(path) = ev.settings.get("bindings_file").and_then(|v| v.as_str()) {
            // Notify adapters in the OnAppLaunch group (where the watcher lives)
            bus.adapters_notify_name(
                "gw2.bindings_watcher".to_string(),
                "bindings-path.set".to_string(),
                Some(serde_json::json!(path))
            );
        }
    }

    fn key_down(&mut self, cx: &Context, ev: &KeyDown) {
        debug!(cx.log(), "SettingsAction key_down for ctx_id: {}", ev.context);
        self.hold_fired = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.hold_cancel = Some(cancel.clone());

        let cx = cx.clone();
        let ctx_id = ev.context.to_string();

        std::thread::spawn(move || {
            std::thread::sleep(LONG_PRESS);
            if !cancel.load(Ordering::SeqCst) {
                cx.globals().remove_keys_and_push(cx.sd(), &["api_key", "bindings_file"]);
            }

            cx.sd().show_ok(&ctx_id);
            debug!(cx.log(), "Long press action completed for context: {}", ctx_id);
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &KeyUp) {
        info!(cx.log(), "SettingsAction key_up for ctx_id: {}", ev.context);
        if let Some(cancel) = self.hold_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
    }
}
