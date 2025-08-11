use std::{ collections::HashMap, fs, sync::Arc };

use num_enum::TryFromPrimitive;
use roxmltree::Document;
use serde::{ Deserialize, Serialize };

use crate::{
    bindings::{ default::default_input_bindings, key_code::KeyCode, key_control::KeyControl },
    log,
    logger::ActionLog,
};

pub mod key_control;
pub mod key_code;
mod default;
pub mod send_keys;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, Serialize, Deserialize)]
pub enum DeviceType {
    Unset = 0,
    Mouse = 1,
    Keyboard = 2,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Modifier: i32 {
        const SHIFT = 1;
        const CTRL = 2;
        const ALT = 4;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub device_type: DeviceType,
    pub code: i32,
    pub modifier: Modifier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBind {
    pub primary: Option<Key>,
    pub secondary: Option<Key>,
}

/// Returns a combined map of default bindings and those loaded from XML.
pub fn load_input_bindings(path: &str, logger: Arc<dyn ActionLog>) -> HashMap<KeyControl, KeyBind> {
    let mut bindings = default_input_bindings();

    let xml_data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) => {
            log!(logger, "❌ Failed to read bindings XML: {}", e);
            return bindings;
        }
    };

    let doc = match Document::parse(&xml_data) {
        Ok(doc) => doc,
        Err(e) => {
            log!(logger, "❌ Failed to parse bindings XML: {}", e);
            return bindings;
        }
    };

    for node in doc.descendants().filter(|n| n.has_tag_name("action")) {
        // Get the KeyControl from the id attribute
        // If the id is not a valid KeyControl, skip this node
        let control = match
            node
                .attribute("id")
                .map(|id| id.parse::<i32>().ok())
                .flatten()
                .map(KeyControl::try_from)
        {
            Some(Ok(control)) => control,
            _ => {
                log!(logger, "⚠️ Invalid or missing id attribute in node: {:?}", node);
                continue;
            }
        };

        let bind = bindings.entry(control).or_insert_with(|| KeyBind {
            primary: None,
            secondary: None,
        });

        // Match primary keybind
        if let Some(device) = node.attribute("device") {
            let device_type = match device {
                "Mouse" => DeviceType::Mouse,
                "Keyboard" => DeviceType::Keyboard,
                "None" => DeviceType::Unset,
                _ => {
                    log!(logger, "⚠️ Unknown device type: {}", device);
                    continue;
                }
            };

            // Unset action if unbound incase there is a default binding
            if device_type == DeviceType::Unset {
                bind.primary = None;
                continue;
            }

            let code = match
                node
                    .attribute("button")
                    .map(|c| c.parse::<i32>().ok())
                    .flatten()
            {
                Some(code) => code,
                None => {
                    log!(logger, "⚠️ Missing or invalid code for control: {:?}", control);
                    log!(logger, "⚠️ Assuming Left Alt for control: {:?}", control);
                    // Default to Left Alt if no code is provided
                    // This is probably because Left Alt has 0 as its code
                    KeyCode::LeftAlt as i32
                }
            };

            let modifier = node
                .attribute("mod")
                .map(|m| m.parse::<i32>().ok())
                .flatten()
                .map(Modifier::from_bits)
                .flatten()
                .unwrap_or(Modifier::empty());

            let key = Key {
                device_type,
                code,
                modifier,
            };

            bind.primary = Some(key);
        }

        // Match secondary keybind
        if let Some(device) = node.attribute("device2") {
            let device_type = match device {
                "Mouse" => DeviceType::Mouse,
                "Keyboard" => DeviceType::Keyboard,
                "None" => DeviceType::Unset,
                _ => {
                    log!(logger, "⚠️ Unknown device type: {}", device);
                    continue;
                }
            };

            // Unset action if unbound incase there is a default binding
            if device_type == DeviceType::Unset {
                bind.secondary = None;
                continue;
            }

            let code: i32 = match
                node
                    .attribute("button2")
                    .map(|c| c.parse::<i32>().ok())
                    .flatten()
            {
                Some(code) => code,
                None => {
                    log!(logger, "⚠️ Missing or invalid code for control: {:?}", control);
                    log!(logger, "⚠️ Assuming Left Alt for control: {:?}", control);
                    // Default to Left Alt if no code is provided
                    // This is probably because Left Alt has 0 as its code
                    KeyCode::LeftAlt as i32
                }
            };

            let modifier = node
                .attribute("mod2")
                .map(|m| m.parse::<i32>().ok())
                .flatten()
                .map(Modifier::from_bits)
                .flatten()
                .unwrap_or(Modifier::empty());

            let key = Key {
                device_type,
                code,
                modifier,
            };

            bind.secondary = Some(key);
        }
    }

    bindings
}
