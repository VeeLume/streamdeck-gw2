use crossbeam_channel::{Receiver as CbReceiver, bounded, select};
use reqwest::blocking::Client;
use std::{collections::HashMap, sync::Arc, thread, time::Duration};
use streamdeck_lib::prelude::*;

use crate::{
    gw2::{
        enums::{CharacterData, TemplateNames},
        shared::TemplateStore,
    },
    topics::{
        GW2_API_CHARACTER_CHANGED, GW2_API_FETCHED, GW2_API_GET_CHARACTERS,
        GW2_API_TEMPLATE_CHANGED, Gw2ApiCharacterChanged, Gw2ApiFetched, Gw2ApiTemplateChanged,
    },
};

const SCHEMA_VERSION: &str = "2024-07-20T01:00:00.000Z";

/// Publishes:
/// - "gw2-api.fetched"            -> { total, added, removed, changed }
/// - "gw2-api.character-changed"  -> { name, change: "added" | "removed" }
/// - "gw2-api.template-changed"   -> { name, before: TemplateNames, after: TemplateNames }
///
/// Listens:
/// - "gw2.api.get_characters"     -> triggers a fetch now
pub struct Gw2ApiAdapter {
    client: Client,
    // local snapshot to diff between runs
    last_seen: HashMap<String, TemplateNames>,
}

impl Gw2ApiAdapter {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("StreamDeck-GW2/1.0")
            .build()
            .expect("Failed to create HTTP client");

        Gw2ApiAdapter {
            client,
            last_seen: HashMap::new(),
        }
    }
}

impl Adapter for Gw2ApiAdapter {
    fn name(&self) -> &'static str {
        "gw2.api_adapter"
    }

    fn policy(&self) -> StartPolicy {
        StartPolicy::OnAppLaunch
    }

    fn topics(&self) -> &'static [&'static str] {
        &[GW2_API_GET_CHARACTERS.name]
    }

    fn start(
        &self,
        cx: &Context,
        bus: std::sync::Arc<dyn Bus>,
        inbox: CbReceiver<Arc<ErasedTopic>>,
    ) -> AdapterResult {
        let (stop_tx, stop_rx) = bounded::<()>(1);

        let logger = cx.log().clone();
        let client = self.client.clone();
        let cx = cx.clone();
        let template_store = cx
            .try_ext::<TemplateStore>()
            .ok_or(AdapterError::Init(
                "TemplateStore extension not found".into(),
            ))?
            .clone();

        // Keep a local, mutable snapshot inside the worker thread
        let mut last_seen: HashMap<String, TemplateNames> = self.last_seen.clone();

        let get_api_key = |cx: Context| {
            cx.globals()
                .get("api_key")
                .and_then(|v| v.as_str().map(|s| s.to_owned()))
        };

        let fetch_apply_and_emit =
            move |cx: &Context,
                  logger: &Arc<dyn ActionLog>,
                  bus: &Arc<dyn Bus>,
                  last_seen: &mut HashMap<String, TemplateNames>| {
                let api_key = match get_api_key(cx.clone()) {
                    Some(key) => key,
                    None => {
                        warn!(logger, "API key not found in globals");
                        return;
                    }
                };

                let characters = match fetch_characters(&client, &api_key) {
                    Ok(chars) => chars,
                    Err(e) => {
                        error!(logger, "Failed to fetch characters: {}", e);
                        return;
                    }
                };

                // Build new snapshot (name -> TemplateNames)
                let mut new_map: HashMap<String, TemplateNames> = HashMap::new();
                for c in &characters {
                    let names = into_template_names(c);
                    new_map.insert(c.name.clone(), names);
                }

                // Counters
                let mut added = 0usize;
                let mut removed = 0usize;
                let mut changed = 0usize;

                // 2a) Removals in one pass over the shared store.
                // Anything not in `new_map` gets dropped + we emit a removal event.
                template_store.retain(|name, _| {
                    if new_map.contains_key(name) {
                        true
                    } else {
                        removed += 1;
                        bus.action_notify_topic_t(
                            GW2_API_CHARACTER_CHANGED,
                            None,
                            Gw2ApiCharacterChanged {
                                name: name.clone(),
                                change: crate::gw2::enums::CharacterChange::Removed,
                            },
                        );
                        false
                    }
                });

                // 2b) Additions & changes; also upsert into the shared store.
                for (name, new_t) in &new_map {
                    match last_seen.get(name) {
                        None => {
                            added += 1;
                            bus.action_notify_topic_t(
                                GW2_API_CHARACTER_CHANGED,
                                None,
                                Gw2ApiCharacterChanged {
                                    name: name.clone(),
                                    change: crate::gw2::enums::CharacterChange::Added,
                                },
                            );
                            template_store.insert(name.clone(), new_t.clone());
                        }
                        Some(old_t) => {
                            if old_t != new_t {
                                changed += 1;
                                bus.action_notify_topic_t(
                                    GW2_API_TEMPLATE_CHANGED,
                                    None,
                                    Gw2ApiTemplateChanged {
                                        name: name.clone(),
                                        before: old_t.clone(),
                                        after: new_t.clone(),
                                    },
                                );
                                template_store.insert(name.clone(), new_t.clone());
                            } else {
                                // unchanged, ensure store still has it (no-op if present)
                                template_store.insert(name.clone(), new_t.clone());
                            }
                        }
                    }
                }

                // 2c) Replace local snapshot and emit summary.
                last_seen.clear();
                last_seen.extend(new_map);

                bus.action_notify_topic_t(
                    GW2_API_FETCHED,
                    None,
                    Gw2ApiFetched {
                        total: last_seen.len(),
                        added,
                        removed,
                        changed,
                    },
                );
            };

        let bus_clone = bus.clone();
        let join = thread::spawn(move || {
            info!(logger, "GW2 API adapter started");

            // ✅ Immediate fetch on start
            debug!(logger, "Initial fetch on start");
            fetch_apply_and_emit(&cx, &logger, &bus_clone, &mut last_seen);
            loop {
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(note) => {
                                if note.downcast(GW2_API_GET_CHARACTERS).is_some() {
                                    debug!(logger, "Trigger fetch: gw2.api.get_characters");
                                    fetch_apply_and_emit(&cx, &logger, &bus_clone, &mut last_seen);
                                }
                            },
                            Err(e) => error!(logger, "Error receiving message: {}", e),
                        }
                    },
                    recv(stop_rx) -> _ => break,
                    default(Duration::from_secs(60)) => {
                        // periodic refresh
                        fetch_apply_and_emit(&cx, &logger, &bus_clone, &mut last_seen);
                    }
                }
            }
            info!(logger, "GW2 API adapter stopped");
        });

        Ok(AdapterHandle::from_crossbeam(join, stop_tx))
    }
}

fn fetch_characters(client: &Client, api_key: &str) -> Result<Vec<CharacterData>, String> {
    client
        .get("https://api.guildwars2.com/v2/characters?ids=all")
        .header("Authorization", format!("Bearer {api_key}"))
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
