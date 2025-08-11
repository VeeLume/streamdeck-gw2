use std::{ collections::HashMap, fs, io::{ Read, Write }, path::PathBuf };
use directories::BaseDirs;
use serde::{ Deserialize, Serialize };

use crate::{ bindings::{ key_control::KeyControl, KeyBind }, plugin::PLUGIN_UUID };

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub api_key: Option<String>,

    /// Optional path to a user-exported XML file
    pub bindings_file: Option<String>,

    /// Parsed bindings (loaded from file or ArcDPS in the future)
    pub bindings_cache: Option<HashMap<KeyControl, KeyBind>>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let mut file = fs::File::open(&path).map_err(|e| format!("Failed to open config: {e}"))?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(|e| format!("Failed to read config: {e}"))?;

        serde_json::from_str(&contents).map_err(|e| format!("Failed to parse config: {e}"))
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path()?;
        let json = serde_json
            ::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        let mut file = fs::File
            ::create(&path)
            .map_err(|e| format!("Failed to create config: {e}"))?;
        file.write_all(json.as_bytes()).map_err(|e| format!("Failed to write config: {e}"))
    }

    fn config_path() -> Result<PathBuf, String> {
        let base = BaseDirs::new().ok_or("Could not find user data directory")?;
        let dir = base.data_dir().join(PLUGIN_UUID);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
        Ok(dir.join("settings.json"))
    }
}
