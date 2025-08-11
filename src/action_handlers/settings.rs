use std::collections::HashMap;
use std::sync::Arc;
use constcat::concat;
use serde_json::Value;

use crate::{
    action_handlers::ActionHandler,
    app::{ AppContext, Controller },
    log,
    logger::ActionLog,
    plugin::{ set_global_settings, WriteSink, PLUGIN_UUID },
};

pub struct SettingsKey {
    logger: Arc<dyn ActionLog>,
}

impl SettingsKey {
    pub const PLUGIN_UUID: &str = concat!(PLUGIN_UUID, ".settings");
    pub fn new(logger: Arc<dyn ActionLog>) -> Self {
        Self { logger }
    }
}

impl ActionHandler for SettingsKey {
    fn on_did_receive_settings(
        &self,
        write: WriteSink,
        ctrl: &Controller,
        app: &AppContext,
        _context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&crate::action_handlers::KeyCoordinates>,
        settings: &serde_json::Map<String, Value>,
        _state: Option<u8>
    ) {
        // 1) Apply to runtime immediately
        if let Some(Value::String(key)) = settings.get("api_key") {
            let trimmed = key.trim();
            if trimmed.is_empty() {
                ctrl.set_api_key(None);
                log!(self.logger, "🔐 Cleared API key (global)");
            } else {
                ctrl.set_api_key(Some(trimmed.to_string()));
                log!(self.logger, "🔑 Set API key (global)");
            }
        }
        if let Some(Value::String(path)) = settings.get("bindings") {
            ctrl.set_bindings_file(Some(path.to_string()));
            log!(self.logger, "🧩 Set bindings file (global)");
        }

        // 2) Persist as GLOBAL settings so future launches pick it up
        let mut persist: serde_json::Map<String, Value> = serde_json::Map::new();
        if let Some(v) = settings.get("api_key").cloned() {
            persist.insert("api_key".into(), v);
        }
        if let Some(v) = settings.get("bindings").cloned() {
            persist.insert("bindings".into(), v);
        }

        if !persist.is_empty() {
            set_global_settings(&write, &app.plugin_uuid, persist);
            log!(self.logger, "💾 Wrote global settings");
        }
    }
}
