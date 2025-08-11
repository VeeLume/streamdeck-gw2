use std::{ collections::HashMap };
use serde::{ Deserialize, Deserializer, Serialize };
use serde_json::Value;
use websocket::{ OwnedMessage };

use crate::{ app::{ AppContext, Controller }, plugin::WriteSink };
pub mod settings;
pub mod set_template;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum DataSourceResult {
    Item(Item),
    ItemGroup(ItemGroup),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Item {
    pub value: String,
    pub label: Option<String>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ItemGroup {
    pub label: String,
    pub children: Vec<Item>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataSourcePayload {
    pub event: Option<String>,
    pub items: Vec<DataSourceResult>,
}

pub fn string_or_integer_to_i64_opt<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where D: Deserializer<'de>
{
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::String(s) => {
            match s.is_empty() {
                true => Ok(None),
                false => s.parse::<i64>().map(Some).map_err(serde::de::Error::custom),
            }
        }
        Value::Number(n) =>
            n
                .as_i64()
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("Invalid number")),
        _ => Err(serde::de::Error::custom("Expected string or number")),
    }
}

pub fn string_or_integer_to_u64_opt<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where D: Deserializer<'de>
{
    // Deserialize the value as an Option<u64>, if the value is 0 it will be None
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::String(s) =>
            s
                .parse::<u64>()
                .map(|n| if n == 0 { None } else { Some(n) })
                .map_err(serde::de::Error::custom),
        Value::Number(n) =>
            n
                .as_u64()
                .map(|n| if n == 0 { None } else { Some(n) })
                .ok_or_else(|| serde::de::Error::custom("Invalid number")),
        Value::Null => Ok(None), // Allow null to be deserialized as None
        _ => Err(serde::de::Error::custom("Expected string, number, or null")),
    }
}

pub fn string_to_string_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where D: Deserializer<'de>
{
    // Deserialize the value as an Option<String>, if the string is empty it will be None
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(if s.is_empty() { None } else { Some(s) }),
        Value::Null => Ok(None),
        _ => Err(serde::de::Error::custom("Expected string or null")),
    }
}

#[derive(Debug, Clone)]
pub struct KeyCoordinates {
    pub column: i32,
    pub row: i32,
}

pub trait ActionHandler: Send + Sync {
    fn on_message(
        &self,
        write: WriteSink,
        ctrl: &Controller,
        app: &AppContext,
        msg: &HashMap<String, Value>
    ) {
        let Some(event) = msg.get("event").and_then(Value::as_str) else {
            return;
        };

        let context = msg.get("context").and_then(Value::as_str).unwrap_or_default();
        let payload = msg.get("payload");

        match event {
            "dialDown" => {
                if
                    let (Some(device), Some(coords), Some(settings)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("coordinates")),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                    )
                {
                    let coordinates = KeyCoordinates {
                        column: coords.get("column").and_then(Value::as_i64).unwrap_or(0) as i32,
                        row: coords.get("row").and_then(Value::as_i64).unwrap_or(0) as i32,
                    };
                    self.on_dial_down(write, ctrl, app, context, device, &coordinates, settings);
                }
            }

            "dialRotate" => {
                if
                    let (Some(device), Some(coords), Some(settings), Some(pressed), Some(ticks)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("coordinates")),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("pressed")).and_then(Value::as_bool),
                        payload.and_then(|p| p.get("ticks")).and_then(Value::as_f64),
                    )
                {
                    let coordinates = KeyCoordinates {
                        column: coords.get("column").and_then(Value::as_i64).unwrap_or(0) as i32,
                        row: coords.get("row").and_then(Value::as_i64).unwrap_or(0) as i32,
                    };
                    self.on_dial_rotate(
                        write,
                        ctrl,
                        app,
                        context,
                        device,
                        &coordinates,
                        pressed,
                        settings,
                        ticks
                    );
                }
            }

            "dialUp" => {
                if
                    let (Some(device), Some(coords), Some(settings)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("coordinates")),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                    )
                {
                    let coordinates = KeyCoordinates {
                        column: coords.get("column").and_then(Value::as_i64).unwrap_or(0) as i32,
                        row: coords.get("row").and_then(Value::as_i64).unwrap_or(0) as i32,
                    };
                    self.on_dial_up(write, ctrl, app, context, device, &coordinates, settings);
                }
            }

            "sendToPlugin" => {
                if let Some(payload) = payload {
                    self.on_did_receive_property_inspector_message(
                        write,
                        ctrl,
                        app,
                        context,
                        payload
                    );
                }
            }

            "didReceiveSettings" => {
                if
                    let (Some(device), Some(controller), Some(settings), Some(is_multi)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("controller")).and_then(Value::as_str),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("isInMultiAction")).and_then(Value::as_bool),
                    )
                {
                    let coordinates = payload
                        .and_then(|p| p.get("coordinates"))
                        .and_then(|coords| {
                            Some(KeyCoordinates {
                                column: coords.get("column")?.as_i64()? as i32,
                                row: coords.get("row")?.as_i64()? as i32,
                            })
                        });

                    let state = payload
                        .and_then(|p| p.get("state"))
                        .and_then(Value::as_u64)
                        .map(|s| s as u8);

                    self.on_did_receive_settings(
                        write,
                        ctrl,
                        app,
                        context,
                        device,
                        controller,
                        is_multi,
                        coordinates.as_ref(),
                        settings,
                        state
                    );
                }
            }

            "keyDown" | "keyUp" => {
                if
                    let (Some(device), Some(settings), Some(is_multi)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("isInMultiAction")).and_then(Value::as_bool),
                    )
                {
                    let coordinates = payload
                        .and_then(|p| p.get("coordinates"))
                        .and_then(|coords| {
                            Some(KeyCoordinates {
                                column: coords.get("column")?.as_i64()? as i32,
                                row: coords.get("row")?.as_i64()? as i32,
                            })
                        });

                    let state = payload
                        .and_then(|p| p.get("state"))
                        .and_then(Value::as_u64)
                        .map(|s| s as u8);
                    let user_state = payload
                        .and_then(|p| p.get("userDesiredState"))
                        .and_then(Value::as_u64)
                        .map(|s| s as u8);

                    match event {
                        "keyDown" => {
                            self.on_key_down(
                                write,
                                ctrl,
                                app,
                                context,
                                device,
                                is_multi,
                                coordinates.as_ref(),
                                settings,
                                state,
                                user_state
                            );
                        }
                        "keyUp" => {
                            self.on_key_up(
                                write,
                                ctrl,
                                app,
                                context,
                                device,
                                is_multi,
                                coordinates.as_ref(),
                                settings,
                                state
                            );
                        }
                        _ => {}
                    }
                }
            }

            "propertyInspectorDidAppear" => {
                if let Some(device) = msg.get("device").and_then(Value::as_str) {
                    self.on_property_inspector_did_appear(write, ctrl, app, context, device);
                }
            }

            "propertyInspectorDidDisappear" => {
                if let Some(device) = msg.get("device").and_then(Value::as_str) {
                    self.on_property_inspector_did_disappear(write, ctrl, app, context, device);
                }
            }

            "titleParametersDidChange" => {
                if
                    let (
                        Some(device),
                        Some(controller),
                        Some(coords),
                        Some(settings),
                        Some(title),
                        Some(params),
                    ) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("controller")).and_then(Value::as_str),
                        payload.and_then(|p| p.get("coordinates")),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("title")).and_then(Value::as_str),
                        payload.and_then(|p| p.get("titleParameters")).and_then(Value::as_object),
                    )
                {
                    let coordinates = KeyCoordinates {
                        column: coords.get("column").and_then(Value::as_i64).unwrap_or(0) as i32,
                        row: coords.get("row").and_then(Value::as_i64).unwrap_or(0) as i32,
                    };
                    self.on_title_parameters_did_change(
                        write,
                        ctrl,
                        app,
                        context,
                        device,
                        controller,
                        &coordinates,
                        settings,
                        payload
                            .and_then(|p| p.get("state"))
                            .and_then(Value::as_u64)
                            .map(|s| s as u8),
                        title,
                        params.get("fontFamily").and_then(Value::as_str).unwrap_or_default(),
                        params.get("fontSize").and_then(Value::as_i64).unwrap_or(12) as i32,
                        params.get("fontStyle").and_then(Value::as_str).unwrap_or_default(),
                        params.get("fontUnderline").and_then(Value::as_bool).unwrap_or(false),
                        params.get("showTitle").and_then(Value::as_bool).unwrap_or(true),
                        params.get("titleAlignment").and_then(Value::as_str).unwrap_or_default(),
                        params.get("titleColor").and_then(Value::as_str).unwrap_or_default()
                    );
                }
            }

            "touchTap" => {
                if
                    let (Some(device), Some(coords), Some(settings), Some(tab_pos)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("coordinates")),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("tapPos")).and_then(Value::as_array),
                    )
                {
                    let coordinates = KeyCoordinates {
                        column: coords.get("column").and_then(Value::as_i64).unwrap_or(0) as i32,
                        row: coords.get("row").and_then(Value::as_i64).unwrap_or(0) as i32,
                    };
                    let pos = (
                        tab_pos.get(0).and_then(Value::as_f64).unwrap_or(0.0),
                        tab_pos.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                    );
                    let hold = payload
                        .and_then(|p| p.get("hold"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    self.on_touch_tab(
                        write,
                        ctrl,
                        app,
                        context,
                        device,
                        &coordinates,
                        hold,
                        settings,
                        pos
                    );
                }
            }

            "willAppear" | "willDisappear" => {
                if
                    let (Some(device), Some(controller), Some(settings), Some(is_multi)) = (
                        msg.get("device").and_then(Value::as_str),
                        payload.and_then(|p| p.get("controller")).and_then(Value::as_str),
                        payload.and_then(|p| p.get("settings")).and_then(Value::as_object),
                        payload.and_then(|p| p.get("isInMultiAction")).and_then(Value::as_bool),
                    )
                {
                    let coordinates = payload
                        .and_then(|p| p.get("coordinates"))
                        .and_then(|coords| {
                            Some(KeyCoordinates {
                                column: coords.get("column")?.as_i64()? as i32,
                                row: coords.get("row")?.as_i64()? as i32,
                            })
                        });

                    let state = payload
                        .and_then(|p| p.get("state"))
                        .and_then(Value::as_u64)
                        .map(|s| s as u8);

                    match event {
                        "willAppear" => {
                            self.on_will_appear(
                                write,
                                ctrl,
                                app,
                                context,
                                device,
                                controller,
                                is_multi,
                                coordinates.as_ref(),
                                settings,
                                state
                            );
                        }
                        "willDisappear" => {
                            self.on_will_disappear(
                                write,
                                ctrl,
                                app,
                                context,
                                device,
                                controller,
                                is_multi,
                                coordinates.as_ref(),
                                settings,
                                state
                            );
                        }
                        _ => {}
                    }
                }
            }

            _ => {}
        }
    }

    fn on_dial_down(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _coordinates: &KeyCoordinates,
        _settings: &serde_json::Map<std::string::String, Value>
    ) {}

    fn on_dial_rotate(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _coordinates: &KeyCoordinates,
        _pressed: bool,
        _settings: &serde_json::Map<std::string::String, Value>,
        _ticks: f64
    ) {}

    fn on_dial_up(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _device: &str,
        _context: &str,
        _coordinates: &KeyCoordinates,
        _settings: &serde_json::Map<std::string::String, Value>
    ) {}

    fn on_did_receive_property_inspector_message(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _payload: &Value
    ) {}

    fn on_did_receive_settings(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&KeyCoordinates>,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>
    ) {}

    fn on_key_down(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&KeyCoordinates>,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>,
        _user_desired_state: Option<u8>
    ) {}

    fn on_key_up(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&KeyCoordinates>,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>
    ) {}

    fn on_property_inspector_did_appear(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str
    ) {}

    fn on_property_inspector_did_disappear(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str
    ) {}

    fn on_title_parameters_did_change(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _controller: &str,
        _coordinates: &KeyCoordinates,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>,
        _title: &str,
        _font_family: &str,
        _font_size: i32,
        _font_style: &str,
        _font_underline: bool,
        _show_title: bool,
        _title_alignment: &str,
        _title_color: &str
    ) {}

    fn on_touch_tab(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _coordinates: &KeyCoordinates,
        _hold: bool,
        _settings: &serde_json::Map<std::string::String, Value>,
        _tap_pos: (f64, f64)
    ) {}

    fn on_will_appear(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&KeyCoordinates>,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>
    ) {}

    fn on_will_disappear(
        &self,
        _write: WriteSink,
        _ctrl: &Controller,
        _app: &AppContext,
        _context: &str,
        _device: &str,
        _controller: &str,
        _is_in_multi_action: bool,
        _coordinates: Option<&KeyCoordinates>,
        _settings: &serde_json::Map<std::string::String, Value>,
        _state: Option<u8>
    ) {}
}

fn get_setting(write: WriteSink, context: &str) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "getSettings",
        "context": context,
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn send_to_property_inspector(write: WriteSink, context: &str, payload: DataSourcePayload) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "sendToPropertyInspector",
        "context": context,
        "payload": payload
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_feedback(write: WriteSink, context: &str, layout: HashMap<String, Value>) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setFeedback",
        "context": context,
        "layout": layout
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_feedback_layout(write: WriteSink, context: &str, layout: &str) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setFeedbackLayout",
        "context": context,
        "layout": layout
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_image(
    write: WriteSink,
    context: &str,
    image: Option<String>,
    state: Option<u8>,
    target: Option<String>
) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setImage",
        "context": context,
        "payload": {
            "image": image,
            "state": state,
            "target": target
        }
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_settings(write: WriteSink, context: &str, settings: HashMap<String, Value>) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setSettings",
        "context": context,
        "payload": settings
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_state(write: WriteSink, context: &str, state: u8) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setState",
        "context": context,
        "payload": {
            "state": state
        }
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

pub fn set_title(
    write: WriteSink,
    context: &str,
    state: Option<u8>,
    target: Option<String>,
    title: Option<String>
) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setTitle",
        "context": context,
        "payload": {
            "title": title,
            "state": state,
            "target": target
        }
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn set_trigger_description(
    write: WriteSink,
    context: &str,
    long_touch: Option<String>,
    push: Option<String>,
    rotate: Option<String>,
    touch: Option<String>
) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
        "event": "setTriggerDescription",
        "context": context,
        "payload": {
            "longTouch": long_touch,
            "push": push,
            "rotate": rotate,
            "touch": touch
        }
    });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn show_alert(write: WriteSink, context: &str) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
            "event": "showAlert",
            "context": context
        });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}

fn show_ok(write: WriteSink, context: &str) {
    if let Ok(mut writer) = write.lock() {
        let msg =
            serde_json::json!({
            "event": "showOk",
            "context": context
        });

        let msg = OwnedMessage::Text(msg.to_string());
        let _ = writer.send_message(&msg);
    }
}
