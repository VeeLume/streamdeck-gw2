// src/plugin.rs
use std::collections::HashMap;
use std::sync::{ Arc, Mutex };

use serde_json::Value;
use websocket::{ ClientBuilder, OwnedMessage };

use crate::action_handlers::{ self, ActionHandler };
use crate::app::{ AppContext, Controller };
use crate::config;
use crate::{ logger::ActionLog, log };
use crate::plugin_state::runtime::{ PluginRuntime, SupervisorMsg };

pub const PLUGIN_UUID: &str = "icu.veelume.gw2";
pub type WriteSink = Arc<Mutex<websocket::client::sync::Writer<std::net::TcpStream>>>;

// Send: getGlobalSettings
fn request_global_settings(write: &crate::plugin::WriteSink, plugin_uuid: &str) {
    if let Ok(mut w) = write.lock() {
        let msg =
            serde_json::json!({
            "event": "getGlobalSettings",
            "context": plugin_uuid, // Stream Deck expects the plugin's registration UUID here
        });
        let _ = w.send_message(&OwnedMessage::Text(msg.to_string()));
    }
}

// Optional: setGlobalSettings (if you want to persist from a handler/PI)
pub fn set_global_settings(
    write: &crate::plugin::WriteSink,
    plugin_uuid: &str,
    settings: serde_json::Map<String, Value>
) {
    if let Ok(mut w) = write.lock() {
        let msg =
            serde_json::json!({
            "event": "setGlobalSettings",
            "context": plugin_uuid,
            "payload": settings,
        });
        let _ = w.send_message(&OwnedMessage::Text(msg.to_string()));
    }
}

// Map incoming JSON -> controller calls
fn apply_global_settings(
    ctrl: &crate::app::Controller,
    payload_settings: &serde_json::Map<String, Value>,
    logger: &Arc<dyn crate::logger::ActionLog>
) {
    if let Some(Value::String(key)) = payload_settings.get("api_key") {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            ctrl.set_api_key(None);
            log!(logger, "🔐 Cleared API key (global)");
        } else {
            ctrl.set_api_key(Some(trimmed.to_string()));
            log!(logger, "🔑 Set API key (global)");
        }
    }
    if let Some(Value::String(path)) = payload_settings.get("bindings") {
        ctrl.set_bindings_file(Some(path.to_string()));
        log!(logger, "🧩 Set bindings file (global)");
    }
}

pub enum PluginRunError {
    WebSocketError,
    RegistrationError,
}

pub fn run_plugin(
    url: String,
    uuid: &String,
    register_event: &String,
    logger: Arc<dyn ActionLog>
) -> Result<(), PluginRunError> {
    log!(logger, "🛠️ Initializing supervisor/runtime");
    let cfg = config::Config::load().unwrap_or_default();
    let (mut runtime, rx) = PluginRuntime::new(Arc::clone(&logger), cfg);

    // Clone necessary fields before moving runtime
    let identity_callbacks_arc = Arc::clone(&runtime.identity_callbacks);
    let character_data_arc = Arc::clone(&runtime.character_data);

    // Controller for commands to supervisor
    let ctrl = Controller::new(runtime.sender());

    // Supervisor loop
    let supervisor_logger = Arc::clone(&logger);
    std::thread::spawn(move || {
        log!(supervisor_logger, "🧭 Supervisor started");
        while let Ok(msg) = rx.recv() {
            runtime.handle(msg);
        }
        log!(supervisor_logger, "🧭 Supervisor exited");
    });

    // AppContext handed to handlers (read-only shared state)
    let app = AppContext {
        identity_callbacks: identity_callbacks_arc,
        character_data: character_data_arc,
        plugin_uuid: uuid.clone(),
        global_settings: Arc::new(
            Mutex::new(
                serde_json::Map
                    ::new()
                    .insert("api_key".to_string(), Value::String("".to_string()))
                    .insert("bindings".to_string(), Value::String("".to_string()))
            )
        ),
    };

    // WebSocket connect/register
    let client = ClientBuilder::new(&url)
        .map_err(|e| {
            log!(logger, "❌ create WS: {}", e);
            PluginRunError::WebSocketError
        })?
        .connect_insecure()
        .map_err(|e| {
            log!(logger, "❌ connect WS: {}", e);
            PluginRunError::WebSocketError
        })?;

    let (mut receiver, sender) = client.split().map_err(|e| {
        log!(logger, "❌ split WS: {}", e);
        PluginRunError::WebSocketError
    })?;
    let write = Arc::new(Mutex::new(sender));

    let register_msg = serde_json::json!({ "event": register_event, "uuid": uuid });
    log!(logger, "📨 Registering plugin with UUID: {}", uuid);
    {
        let mut w = write.lock().map_err(|e| {
            log!(logger, "❌ writer lock: {}", e);
            PluginRunError::WebSocketError
        })?;
        w.send_message(&OwnedMessage::Text(register_msg.to_string())).map_err(|e| {
            log!(logger, "❌ send register: {}", e);
            PluginRunError::RegistrationError
        })?;
        request_global_settings(&write, &uuid);
    }
    log!(logger, "✅ Plugin registered successfully");

    // Build handler table
    let handlers: HashMap<&str, Arc<dyn ActionHandler>> = HashMap::from([
        (
            action_handlers::settings::SettingsKey::PLUGIN_UUID,
            Arc::new(action_handlers::settings::SettingsKey::new(Arc::clone(&logger))) as Arc<
                dyn ActionHandler
            >,
        ),
        (
            action_handlers::set_template::SetTemplateKey::PLUGIN_UUID,
            Arc::new(
                action_handlers::set_template::SetTemplateKey::new(Arc::clone(&logger))
            ) as Arc<dyn ActionHandler>,
        ),
    ]);

    log!(logger, "🔄 Starting message loop");
    for message in receiver.incoming_messages() {
        match message {
            Ok(OwnedMessage::Text(text)) => {
                log!(logger, "📥 Received message: {}", text);
                let msg: HashMap<String, serde_json::Value> = match serde_json::from_str(&text) {
                    Ok(val) => val,
                    Err(e) => {
                        log!(logger, "❌ parse: {e}");
                        continue;
                    }
                };

                let action = msg.get("action").and_then(|v| v.as_str());
                let event = msg.get("event").and_then(|v| v.as_str());

                if let Some(name) = action {
                    if let Some(h) = handlers.get(name) {
                        h.on_message(Arc::clone(&write), &ctrl, &app, &msg);
                        log!(logger, "🔧 Handled action {} (event {:?})", name, event);
                    } else {
                        log!(logger, "⚠️ No handler for action: {}", name);
                    }
                } else if let Some(evt) = event {
                    log!(logger, "🌀 Global event: {}", evt);
                    match evt {
                        "didReceiveGlobalSettings" => {
                            if let Some(payload) = msg.get("payload").and_then(Value::as_object) {
                                if
                                    let Some(settings) = payload
                                        .get("settings")
                                        .and_then(Value::as_object)
                                {
                                    if let Some(Value::String(key)) = settings.get("api_key") {
                                        let trimmed = key.trim();
                                        if trimmed.is_empty() {
                                            ctrl.set_api_key(None);
                                        } else {
                                            ctrl.set_api_key(Some(trimmed.to_string()));
                                        }
                                    }
                                    if let Some(Value::String(path)) = settings.get("bindings") {
                                        ctrl.set_bindings_file(Some(path.to_string()));
                                    }
                                }
                            }
                        }
                        "applicationDidLaunch" => {
                            ctrl.app_launched();
                        }
                        "applicationDidTerminate" => {
                            ctrl.app_terminated();
                        }
                        _ => log!(logger, "⚠️ Unhandled event: {}", evt),
                    }
                } else {
                    log!(logger, "⚠️ Message without action/event");
                }
            }
            Ok(OwnedMessage::Close(_)) => {
                log!(logger, "🔌 Connection closed");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                log!(logger, "❌ WS error: {e}");
                break;
            }
        }
    }

    log!(logger, "🛑 WebSocket loop terminated");
    Ok(())
}
