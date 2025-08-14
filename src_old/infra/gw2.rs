use std::collections::HashMap;
use std::panic::{ catch_unwind, AssertUnwindSafe };
use std::thread::{ self, JoinHandle };
use std::time::Duration;
use std::sync::Arc;

use crossbeam_channel::{ unbounded, tick, select, Receiver, Sender };
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::core::events::{ AppEvent, TemplateNames };
use crate::plugin::PLUGIN_UUID;
use crate::{ log };
use crate::logger::ActionLog;

const SCHEMA_VERSION: &str = "2024-07-20T01:00:00.000Z";

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterData {
    pub name: String,
    #[serde(default)]
    pub build_tabs: Vec<BuildTab>,
    #[serde(default)]
    pub equipment_tabs: Vec<EquipmentTab>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub is_active: bool,
    pub build: Build,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EquipmentTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub name: Option<String>,
    pub is_active: bool,
}

enum Cmd {
    SetApiKey(Option<String>),
    ForceRefresh,
    Shutdown,
}

/// Handle to the background poller thread.
pub struct Gw2Poller {
    tx: Sender<Cmd>,
    join: Option<JoinHandle<()>>,
}

impl Gw2Poller {
    /// Spawn the poller thread. It will:
    /// - poll every `interval`
    /// - emit `AppEvent::Gw2TemplateNames { character, names }`
    pub fn spawn(logger: Arc<dyn ActionLog>, app_tx: Sender<AppEvent>, interval: Duration) -> Self {
        let (tx, rx) = unbounded::<Cmd>();

        let join = thread::spawn(move || {
            if
                let Err(p) = catch_unwind(
                    AssertUnwindSafe(|| {
                        let client = Client::builder()
                            .user_agent(PLUGIN_UUID)
                            .build()
                            .unwrap_or_else(|_| Client::new());

                        let ticker = tick(interval);
                        let mut api_key: Option<String> = None;

                        log!(logger, "🛰️ GW2 poller started (interval: {:?})", interval);

                        loop {
                            select! {
                    recv(rx) -> msg => match msg {
                        Ok(Cmd::SetApiKey(k)) => {
                            api_key = k;
                            log!(logger, "🔑 GW2 API key {}", if api_key.is_some() { "set" } else { "cleared" });
                        }
                        Ok(Cmd::ForceRefresh) => {
                            if let Some(ref key) = api_key {
                                fetch_and_emit(&client, key, &app_tx, &logger);
                            } else {
                                log!(logger, "⚠️ Force refresh ignored: no API key");
                            }
                        }
                        Ok(Cmd::Shutdown) | Err(_) => {
                            break;
                        }
                    },
                    recv(ticker) -> _ => {
                        if let Some(ref key) = api_key {
                            fetch_and_emit(&client, key, &app_tx, &logger);
                        }
                    }
                }
                        }

                        log!(logger, "🛑 GW2 poller stopped");
                    })
                )
            {
                log!(logger, "❌ reader thread panicked: {:?}", p);
            }
        });

        Self { tx, join: Some(join) }
    }

    /// Update/clear API key.
    pub fn set_api_key(&self, key: Option<String>) {
        let _ = self.tx.send(Cmd::SetApiKey(key));
    }

    /// Trigger an immediate fetch.
    pub fn force_refresh(&self) {
        let _ = self.tx.send(Cmd::ForceRefresh);
    }

    /// Stop the thread and join.
    pub fn shutdown(mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for Gw2Poller {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn fetch_and_emit(
    client: &Client,
    api_key: &str,
    app_tx: &Sender<AppEvent>,
    logger: &Arc<dyn ActionLog>
) {
    match fetch_characters(client, api_key) {
        Ok(chars) => {
            // Emit per-character template names; reactor can store/merge.
            for c in chars {
                let names = into_template_names(&c);
                let _ = app_tx.send(AppEvent::Gw2TemplateNames {
                    character: c.name.clone(),
                    names,
                });
            }
            log!(logger, "✅ GW2 templates updated");
        }
        Err(e) => {
            log!(logger, "❌ GW2 API polling error: {}", e);
        }
    }
}

fn fetch_characters(client: &Client, api_key: &str) -> Result<Vec<CharacterData>, String> {
    client
        .get("https://api.guildwars2.com/v2/characters?ids=all")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("X-Schema-Version", SCHEMA_VERSION)
        .send()
        .map_err(|e| format!("request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("status error: {e}"))?
        .json::<Vec<CharacterData>>()
        .map_err(|e| format!("parse error: {e}"))
}

fn into_template_names(c: &CharacterData) -> TemplateNames {
    let mut t = TemplateNames::default();

    for bt in &c.build_tabs {
        if (1..=9).contains(&bt.tab_index) {
            let idx = (bt.tab_index - 1) as usize;
            t.build[idx] = bt.build.name.clone().filter(|s| !s.trim().is_empty());
        }
    }
    for et in &c.equipment_tabs {
        if (1..=9).contains(&et.tab_index) {
            let idx = (et.tab_index - 1) as usize;
            t.equipment[idx] = et.name.clone().filter(|s| !s.trim().is_empty());
        }
    }
    t
}
