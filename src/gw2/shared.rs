// src/gw2/shared.rs
use std::sync::{Arc, RwLock};

use dashmap::DashMap;
use streamdeck_lib::prelude::{GlobalSettings, SdClient};

use crate::gw2::{binds::BindingSet, enums::TemplateNames};

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
        globals: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        let Some(v) = globals.get("bindings") else {
            return Err("No bindings found in globals".to_string());
        };

        // 1) Try flat shape: bindings -> BindingSet
        if let Ok(bs) = serde_json::from_value::<BindingSet>(v.clone()) {
            return self.replace_bindings(bs);
        }

        // 2) Try wrapped shape: bindings -> { bindings: BindingSet }
        if let Some(inner) = v.as_object().and_then(|o| o.get("bindings")) {
            if let Ok(bs) = serde_json::from_value::<BindingSet>(inner.clone()) {
                return self.replace_bindings(bs);
            }
        }

        Err("Failed to parse bindings from globals (unexpected shape)".to_string())
    }

    pub fn write_to_globals(&self, globals: GlobalSettings, _sd: &SdClient) -> Result<(), String> {
        let binds = self.0.read().map_err(|e| e.to_string())?;
        let binds_json = serde_json::to_value(&*binds).map_err(|e| e.to_string())?;

        // Write flat shape; DO NOT wrap in another {"bindings": ...}
        globals.set("bindings", binds_json);
        Ok(())
    }
}

/// Cloneable, thread-safe map: character -> names
#[derive(Clone, Default)]
pub struct TemplateStore(Arc<DashMap<String, TemplateNames>>);

impl TemplateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, character: &str) -> Option<TemplateNames> {
        // clone so actions can own it briefly without a borrow guard
        self.0.get(character).map(|r| r.clone())
    }

    pub fn insert(&self, character: String, names: TemplateNames) {
        self.0.insert(character, names);
    }

    /// Remove a character’s templates. Returns true if an entry existed.
    pub fn remove(&self, character: &str) -> bool {
        self.0.remove(character).is_some()
    }

    /// Retain only entries that satisfy the given predicate.
    /// Example: store.retain(|name, _| new_map.contains_key(name))
    pub fn retain<F>(&self, f: F)
    where
        F: FnMut(&String, &mut TemplateNames) -> bool,
    {
        self.0.retain(f);
    }
}

#[derive(Clone, Default)]
pub struct ActiveChar(Arc<RwLock<Option<String>>>);
impl ActiveChar {
    pub fn get(&self) -> Option<String> {
        self.0.read().ok().and_then(|g| g.clone())
    }
    pub fn set(&self, v: Option<String>) {
        if let Ok(mut w) = self.0.write() {
            *w = v;
        }
    }
}
