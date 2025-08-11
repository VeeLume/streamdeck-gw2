use std::{
    collections::HashMap,
    sync::{ Arc, Mutex, atomic::{ AtomicBool, Ordering } },
    thread::{ self, JoinHandle },
    time::Duration,
};

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::{ log, logger::ActionLog };

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterData {
    pub name: String,
    #[serde(default)]
    pub build_tabs: Vec<BuildTab>,
    #[serde(default)]
    pub equipment_tabs: Vec<EquipmentTab>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub is_active: bool,
    pub build: Build,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EquipmentTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub name: Option<String>,
    pub is_active: bool,
}

pub struct Gw2Poller {
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Gw2Poller {
    pub fn start(
        api_key: String,
        character_store: Arc<Mutex<HashMap<String, CharacterData>>>,
        logger: Arc<dyn ActionLog>
    ) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);

        let thread = thread::spawn(move || {
            let client = Client::new();
            while !shutdown_clone.load(Ordering::Relaxed) {
                if let Err(e) = fetch_all_characters(&client, &api_key, &character_store, &logger) {
                    logger.log(&format!("❌ GW2 API polling error: {e}"));
                }
                thread::sleep(Duration::from_secs(60));
            }
            logger.log("🛑 Stopped GW2 API poller");
        });

        Self {
            shutdown,
            thread: Some(thread),
        }
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

pub fn fetch_all_characters(
    client: &Client,
    api_key: &str,
    store: &Arc<Mutex<HashMap<String, CharacterData>>>,
    logger: &Arc<dyn ActionLog>
) -> Result<(), String> {
    let characters = client
        .get("https://api.guildwars2.com/v2/characters?ids=all")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("X-Schema-Version", "2024-07-20T01:00:00.000Z")
        .send()
        .map_err(|e| format!("Character data request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Status error fetching character data: {e}"))?
        .json::<Vec<CharacterData>>()
        .map_err(|e| format!("Failed to parse character data: {e}"))?;

    let new_data: HashMap<String, CharacterData> = characters
        .into_iter()
        .map(|char| (char.name.clone(), char))
        .collect();

    *store.lock().map_err(|_| "Failed to lock character store".to_string())? = new_data;

    log!(logger, "✅ GW2 API character templates updated");

    Ok(())
}
