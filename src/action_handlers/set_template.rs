use std::sync::Arc;
use constcat::concat;
use serde_json::Value;

use crate::{
    action_handlers::ActionHandler,
    app::{ AppContext, Controller },
    bindings::key_control::KeyControl,
    log,
    logger::ActionLog,
    plugin::{ WriteSink, PLUGIN_UUID },
    plugin_state::poller::CharacterData,
};

fn wrap_title(title: &str, max_len: usize) -> String {
    let mut lines = vec![];
    let mut current_line = String::new();

    for word in title.split_whitespace() {
        if !current_line.is_empty() && current_line.len() + 1 + word.len() > max_len {
            lines.push(std::mem::take(&mut current_line));
        }
        if !current_line.is_empty() {
            current_line.push(' ');
        }
        current_line.push_str(word);
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines.join("\n")
}

fn get_index_from_settings(settings: &serde_json::Map<String, Value>, key: &str) -> Option<u8> {
    settings
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u8)
}

fn resolve_key_control(template_type: &str, index: u8) -> Option<KeyControl> {
    use KeyControl::*;
    if !(1..=9).contains(&index) {
        return None;
    }
    let i = index - 1;
    Some(match template_type {
        "build" =>
            match i {
                0 => TemplatesBuildTemplate1,
                1 => TemplatesBuildTemplate2,
                2 => TemplatesBuildTemplate3,
                3 => TemplatesBuildTemplate4,
                4 => TemplatesBuildTemplate5,
                5 => TemplatesBuildTemplate6,
                6 => TemplatesBuildTemplate7,
                7 => TemplatesBuildTemplate8,
                8 => TemplatesBuildTemplate9,
                _ => {
                    return None;
                }
            }
        "equipment" =>
            match i {
                0 => TemplatesEquipmentTemplate1,
                1 => TemplatesEquipmentTemplate2,
                2 => TemplatesEquipmentTemplate3,
                3 => TemplatesEquipmentTemplate4,
                4 => TemplatesEquipmentTemplate5,
                5 => TemplatesEquipmentTemplate6,
                6 => TemplatesEquipmentTemplate7,
                7 => TemplatesEquipmentTemplate8,
                8 => TemplatesEquipmentTemplate9,
                _ => {
                    return None;
                }
            }
        _ => {
            return None;
        }
    })
}

pub struct SetTemplateKey {
    logger: Arc<dyn ActionLog>,
}

impl SetTemplateKey {
    pub const PLUGIN_UUID: &str = concat!(PLUGIN_UUID, ".set-template");

    pub fn new(logger: Arc<dyn ActionLog>) -> Self {
        Self { logger }
    }

    fn register_identity_title_callback(
        &self,
        app: &AppContext,
        writer: WriteSink,
        context: String,
        build_index: Option<u8>,
        equipment_index: Option<u8>
    ) {
        let character_data = Arc::clone(&app.character_data);
        let logger = Arc::clone(&self.logger);

        // Store the closure in the shared map; supervisor doesn't need the fn itself.
        if let Ok(mut map) = app.identity_callbacks.lock() {
            map.insert(
                context.clone(),
                Box::new(move |identity| {
                    if let Ok(char_data) = character_data.lock() {
                        if let Some(character) = char_data.get(&identity.name) {
                            let title = build_title_for_indices(
                                character,
                                build_index,
                                equipment_index,
                                &logger
                            );
                            let writer = writer.clone();
                            crate::action_handlers::set_title(
                                writer,
                                &context,
                                None,
                                None,
                                Some(title)
                            );
                        } else {
                            log!(logger, "⚠️ Character data not found for {}", identity.name);
                        }
                    } else {
                        log!(logger, "❌ Failed to lock character data");
                    }
                })
            );
        }
    }
}

fn build_title_for_indices(
    character: &CharacterData,
    build_index: Option<u8>,
    equipment_index: Option<u8>,
    logger: &Arc<dyn ActionLog>
) -> String {
    let mut parts = vec![];

    if let Some(bi) = build_index {
        if let Some(tab) = character.build_tabs.get((bi as usize) - 1) {
            let name = tab.build.name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Build Template {}", bi));
            parts.push(wrap_title(&name, 10));
        }
    }

    if let Some(ei) = equipment_index {
        if let Some(tab) = character.equipment_tabs.get((ei as usize) - 1) {
            let name = tab.name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("Equipment Template {}", ei));
            parts.push(wrap_title(&name, 10));
        }
    }

    if parts.is_empty() {
        log!(logger, "⚠️ No templates set");
        "Unknown\nTemplate".to_string()
    } else {
        parts.join("\n")
    }
}

impl ActionHandler for SetTemplateKey {
    fn on_key_down(
        &self,
        _write: WriteSink,
        ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&crate::action_handlers::KeyCoordinates>,
        settings: &serde_json::Map<String, Value>,
        _state: Option<u8>,
        _user_desired_state: Option<u8>
    ) {
        for (template_type, key) in &[
            ("build", "build_index"),
            ("equipment", "equipment_index"),
        ] {
            if let Some(index) = get_index_from_settings(settings, key) {
                if let Some(action) = resolve_key_control(template_type, index) {
                    ctrl.queue_action(action as i32, /*allow_in_combat=*/ false);
                    log!(self.logger, "✅ Queued {} action: {:?}", template_type, action);
                } else {
                    log!(self.logger, "❌ Invalid {} index: {}", template_type, index);
                }
            }
        }
    }

    fn on_will_appear(
        &self,
        write: WriteSink,
        _ctrl: &Controller,
        app: &AppContext,
        context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&crate::action_handlers::KeyCoordinates>,
        settings: &serde_json::Map<String, Value>,
        _state: Option<u8>
    ) {
        // (Re)register identity->title callback for this context
        if let Ok(mut map) = app.identity_callbacks.lock() {
            map.remove(context);
        }
        let build_index = get_index_from_settings(settings, "build_index");
        let equipment_index = get_index_from_settings(settings, "equipment_index");
        self.register_identity_title_callback(
            app,
            write.clone(),
            context.to_string(),
            build_index,
            equipment_index
        );
    }

    fn on_did_receive_settings(
        &self,
        write: WriteSink,
        _ctrl: &Controller,
        app: &AppContext,
        context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&crate::action_handlers::KeyCoordinates>,
        settings: &serde_json::Map<String, Value>,
        _state: Option<u8>
    ) {
        // Same behavior as willAppear: refresh the callback on settings change
        if let Ok(mut map) = app.identity_callbacks.lock() {
            map.remove(context);
        }
        let build_index = get_index_from_settings(settings, "build_index");
        let equipment_index = get_index_from_settings(settings, "equipment_index");
        self.register_identity_title_callback(
            app,
            write.clone(),
            context.to_string(),
            build_index,
            equipment_index
        );
    }

    fn on_will_disappear(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        app: &AppContext,
        context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&crate::action_handlers::KeyCoordinates>,
        _settings: &serde_json::Map<String, Value>,
        _state: Option<u8>
    ) {
        if let Ok(mut map) = app.identity_callbacks.lock() {
            map.remove(context);
        }
        log!(self.logger, "🗑️ Unregistered identity callback for context: {}", context);
    }
}
