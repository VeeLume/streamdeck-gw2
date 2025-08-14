use std::sync::{ Arc, RwLock };
use dashmap::DashMap;
use serde_json::{ Map, Value };
use crate::{core::events::TemplateNames, infra::bindings_manager::BindingsStore};

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
}

#[derive(Clone, Default)]
pub struct ActiveChar(Arc<RwLock<Option<String>>>);
impl ActiveChar {
    pub fn get(&self) -> Option<String> {
        self.0
            .read()
            .ok()
            .and_then(|g| g.clone())
    }
    pub fn set(&self, v: Option<String>) {
        if let Ok(mut w) = self.0.write() {
            *w = v;
        }
    }
}

#[derive(Clone, Default)]
pub struct GlobalSettings(Arc<RwLock<Map<String, Value>>>);

impl GlobalSettings {
    /// Replace the whole map (only use when you *really* want to).
    pub fn set(&self, m: Map<String, Value>) {
        if let Ok(mut w) = self.0.write() {
            *w = m;
        }
    }

    /// Get a snapshot clone (cheap enough for our sizes).
    pub fn get(&self) -> Map<String, Value> {
        self.0
            .read()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Safely mutate-in-place with a closure and return the merged snapshot.
    /// Great for building a payload for setGlobalSettings without losing keys.
    pub fn update<F>(&self, f: F) -> Map<String, Value> where F: FnOnce(&mut Map<String, Value>) {
        if let Ok(mut w) = self.0.write() {
            f(&mut w);
            return w.clone();
        }
        Map::new()
    }

    /// Shallow merge: insert/overwrite keys from `patch`, keep everything else.
    /// Returns the merged snapshot (use as payload for setGlobalSettings).
    pub fn merge(&self, patch: Map<String, Value>) -> Map<String, Value> {
        self.update(|m| {
            // Manually extend since Map::drain does not exist for serde_json::Map.
            for (k, v) in patch {
                m.insert(k, v);
            }
        })
    }

    /// Remove specific keys (e.g. to clear api_key/bindings without touching cache).
    pub fn remove_keys<I, S>(&self, keys: I) -> Map<String, Value>
        where I: IntoIterator<Item = S>, S: AsRef<str>
    {
        self.update(|m| {
            for k in keys {
                m.remove(k.as_ref());
            }
        })
    }

    /// Optional: JSON Merge Patch–style semantics where `null` means delete.
    /// Useful if callers prefer sending `{ "bindings_cache": null }` to clear it.
    pub fn merge_patch(&self, patch: Map<String, Value>) -> Map<String, Value> {
        self.update(|m| {
            for (k, v) in patch {
                if v.is_null() {
                    m.remove(&k);
                } else {
                    m.insert(k, v);
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct AppCtx {
    pub templates: TemplateStore,
    pub active_character: ActiveChar, // used by actions to know which character is active
    pub global_settings: GlobalSettings, // used by actions to read global settings
    pub bindings_store: BindingsStore, // shared bindings map
}

impl AppCtx {
    pub fn new() -> Self {
        Self {
            templates: TemplateStore::new(),
            active_character: ActiveChar::default(),
            global_settings: GlobalSettings::default(),
            bindings_store: BindingsStore::default(),
        }
    }

    pub fn with_bindings_store(bindings_store: BindingsStore) -> Self {
        Self {
            templates: TemplateStore::new(),
            active_character: ActiveChar::default(),
            global_settings: GlobalSettings::default(),
            bindings_store,
        }
    }
}
