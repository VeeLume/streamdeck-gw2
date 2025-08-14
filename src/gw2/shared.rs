// src/gw2/shared.rs
use std::sync::{ Arc, RwLock };

use serde_json::Map;
use streamdeck_lib::prelude::{ GlobalSettings, SdClient };

use crate::gw2::binds::BindingSet;

/// Arc<RwLock<…>> so SettingsAction can update at runtime and mappers read it.
#[derive(Clone)]
pub struct SharedBindings(pub Arc<RwLock<BindingSet>>);

impl SharedBindings {
    pub fn default() -> Self {
        SharedBindings(Arc::new(RwLock::new(BindingSet::with_default())))
    }

    pub fn replace_bindings(&self, new_binds: BindingSet) -> Result<(), String> {
        let mut binds = self.0.write().map_err(|e| e.to_string())?;
        *binds = new_binds;
        Ok(())
    }

    pub fn replace_from_globals(
        &self,
        globals: &Map<String, serde_json::Value>
    ) -> Result<(), String> {
        if let Some(bindings) = globals.get("bindings") {
            if let Ok(new_binds) = serde_json::from_value::<BindingSet>(bindings.clone()) {
                self.replace_bindings(new_binds)
            } else {
                Err("Failed to parse bindings from globals".to_string())
            }
        } else {
            Err("No bindings found in globals".to_string())
        }
    }

    pub fn write_to_globals(&self, globals: GlobalSettings, sd: &SdClient) -> Result<(), String> {
        let binds = self.0.read().map_err(|e| e.to_string())?;
        let binds_json = serde_json::to_value(&*binds).map_err(|e| e.to_string())?;
        let mut patch = serde_json::Map::new();
        patch.insert("bindings".to_string(), binds_json);
        globals.merge_and_push(sd, patch);
        Ok(())
    }
}
