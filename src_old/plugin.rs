use std::panic::{ catch_unwind, AssertUnwindSafe };
// src/plugin.rs
use std::sync::{ Arc, Mutex };
use std::thread;
use std::time::{ Duration, Instant };

use chrono::Utc;
use crossbeam_channel::{ unbounded, Receiver, RecvTimeoutError, Sender };
use serde_json::{ Map, Value };
use websocket::{ ClientBuilder, OwnedMessage };

use crate::app::action::Note;
use crate::app::action_manager::ActionManager;
use crate::app::actions;
use crate::app::context::{ AppCtx };
use crate::app::state::PluginState;
use crate::core::events::{ AppEvent, Command };
use crate::core::{ events, sd_client };
use crate::infra::bindings_manager::BindingsManager;
use crate::infra::gw2::Gw2Poller;
use crate::infra::mumble::Mumble;
use crate::logger::ActionLog;
use crate::{ log, LaunchArgs };
use crate::infra::{ bindings, sd_protocol, Outgoing };

pub static PLUGIN_UUID: &str = "icu.veelume.gw2";

//
// Public entrypoint
//
pub fn run(args: LaunchArgs, logger: Arc<dyn ActionLog>) -> anyhow::Result<()> {
    // --- Connect to Stream Deck -------------------------------------------------
    let url = format!("ws://127.0.0.1:{}", args.port);
    log!(logger, "🔗 connecting websocket: {}", url);

    let client = ClientBuilder::new(&url)?.connect_insecure()?;
    let (mut reader, writer) = client.split().map_err(|e| anyhow::anyhow!(e))?;
    let writer = Arc::new(Mutex::new(writer)); // <— shareable writer

    // --- Channels ---------------------------------------------------------------
    // Outgoing typed messages to SD (UI updates, etc.)
    let (sd_out_tx, sd_out_rx) = unbounded::<Outgoing>();
    // App events (Stream Deck events + adapter events) into the reactor/ActionManager
    let (app_ev_tx, app_ev_rx) = unbounded::<events::AppEvent>();
    // Commands from reactor to infra/supervisor (execute keys, switch mumble mode, persist config, …)
    let (cmd_tx, cmd_rx) = unbounded::<events::Command>();

    // A small typed client actions can use to send UI updates.
    let sd_client = sd_client::SdClient::new(sd_out_tx.clone(), args.plugin_uuid.clone());

    // --- Register plugin --------------------------------------------------------
    {
        let register_msg =
            serde_json::json!({
            "event": args.register_event,
            "uuid": args.plugin_uuid
        });

        if let Ok(mut w) = writer.lock() {
            w.send_message(&OwnedMessage::Text(register_msg.to_string()))?;
        } else {
            anyhow::bail!("failed to lock writer for registration");
        }
    }
    log!(logger, "✅ registered: {}", args.plugin_uuid);
    sd_client.get_global_settings();

    // --- Spawn writer (Outgoing -> websocket) ----------------------------------
    {
        let logger = Arc::clone(&logger);
        let writer_arc = Arc::clone(&writer);
        let app_ev_tx = app_ev_tx.clone();

        thread::spawn({
            let logger = Arc::clone(&logger);
            let writer_arc = Arc::clone(&writer);
            let app_ev_tx = app_ev_tx.clone();

            move || {
                if
                    let Err(p) = catch_unwind(
                        AssertUnwindSafe(|| {
                            for msg in sd_out_rx.iter() {
                                match sd_protocol::serialize_outgoing(&msg) {
                                    Ok(text) =>
                                        match writer_arc.lock() {
                                            Ok(mut w) => {
                                                if
                                                    let Err(e) = w.send_message(
                                                        &OwnedMessage::Text(text)
                                                    )
                                                {
                                                    log!(logger, "❌ websocket send: {:?}", e);
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                log!(logger, "❌ writer mutex poisoned: {:?}", e);
                                                break;
                                            }
                                        }
                                    Err(e) => log!(logger, "❌ serialize outgoing: {:?}", e),
                                }
                            }
                        })
                    )
                {
                    // p: Box<dyn Any + Send>
                    log!(logger, "❌ writer thread panicked: {:?}", p);
                }

                log!(logger, "🧵 writer thread exited");
                if app_ev_tx.send(AppEvent::Shutdown).is_err() {
                    log!(logger, "❌ failed to send shutdown event to reactor");
                }
            }
        });
    }

    // --- Spawn reader (websocket -> AppEvent::StreamDeck) ----------------------
    {
        let logger = Arc::clone(&logger);
        let app_ev_tx = app_ev_tx.clone();
        thread::spawn(move || {
            if
                let Err(p) = catch_unwind(
                    AssertUnwindSafe(|| {
                        for incoming in reader.incoming_messages() {
                            match incoming {
                                Ok(OwnedMessage::Text(text)) => {
                                    log!(logger, "📥 websocket message: {}", text);
                                    // parse once into a typed event
                                    let parsed = serde_json
                                        ::from_str::<Map<String, Value>>(&text)
                                        .map_err(|e| format!("json parse error: {e}"))
                                        .and_then(|m| sd_protocol::parse_incoming(&m));

                                    match parsed {
                                        Ok(sd_ev) => {
                                            log!(logger, "📥 Stream Deck event: {}", sd_ev);
                                            let _ = app_ev_tx.send(
                                                events::AppEvent::StreamDeck(sd_ev)
                                            );
                                        }
                                        Err(err) => {
                                            log!(
                                                logger,
                                                "⚠️ unrecognized Stream Deck event: {} | raw = {}",
                                                err,
                                                text
                                            );
                                        }
                                    }
                                }
                                Ok(OwnedMessage::Close(_)) => {
                                    break;
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    log!(logger, "❌ websocket read: {:?}", e);
                                    break;
                                }
                            }
                        }
                        log!(logger, "🔌 connection closed (reader)");
                        if app_ev_tx.send(AppEvent::Shutdown).is_err() {
                            log!(logger, "❌ failed to send shutdown event to reactor");
                        }
                    })
                )
            {
                log!(logger, "❌ reader thread panicked: {:?}", p);
                if app_ev_tx.send(AppEvent::Shutdown).is_err() {
                    log!(logger, "❌ failed to send shutdown event to reactor");
                }
            }
        });
    }

    let (bindings_mgr, bindings_store) = BindingsManager::spawn(
        Arc::clone(&logger),
        app_ev_tx.clone()
    );
    let ctx = AppCtx::with_bindings_store(bindings_store.clone());

    // --- Spawn supervisor/router for Commands ----------------------------------
    {
        let logger = Arc::clone(&logger);
        let sd_out_tx = sd_out_tx.clone();
        let ctx_for_router = ctx.clone();

        thread::spawn(move || {
            if
                let Err(p) = catch_unwind(
                    AssertUnwindSafe(|| {
                        command_router(
                            cmd_rx,
                            sd_out_tx,
                            app_ev_tx.clone(),
                            Arc::clone(&logger),
                            args.plugin_uuid.clone(),
                            ctx_for_router,
                            bindings_mgr
                        );
                        log!(logger, "🧵 command router exited");
                    })
                )
            {
                log!(logger, "❌ reader thread panicked: {:?}", p);
                let _ = app_ev_tx.send(AppEvent::Shutdown);
            }
        });
    }

    thread::sleep(Duration::from_millis(1000)); // give the router a moment to start

    // --- Build ActionManager with your actions ---------------------------------
    let mut action_manager = ActionManager::new(
        vec![
            &actions::SET_TEMPLATE, // static factory instances
            &actions::SETTINGS
        ],
        ctx.clone()
    );

    // --- Reactor loop: handle AppEvents, update PluginState, emit Commands ------
    let mut state = PluginState::default();
    let mut watched_bindings_path: Option<std::path::PathBuf> = None;
    let mut did_initial_bindings_load = false;

    log!(logger, "🔄 reactor loop started");

    use events::AppEvent::*;
    for ev in app_ev_rx.iter() {
        match ev {
            StreamDeck(sd_ev) => {
                use crate::infra::sd_protocol::StreamDeckEvent as SDE;
                match sd_ev {
                    SDE::DidReceiveGlobalSettings { settings } => {
                        // strip bindings_cache for log output
                        // Clone settings and remove bindings_cache for cleaner log output
                        let mut settings_clean = settings.clone();
                        settings_clean.remove("bindings_cache");
                        log!(logger, "📥 global settings: {:?}", settings_clean);
                        action_manager.ctx().global_settings.set(settings.clone());

                        let new_path = settings
                            .get("bindings")
                            .and_then(|v| v.as_str())
                            .map(std::path::PathBuf::from);

                        if new_path != watched_bindings_path {
                            watched_bindings_path = new_path.clone();
                            let _ = cmd_tx.send(Command::SetBindingsPath(new_path.clone()));
                        }

                        let api = settings
                            .get("api_key")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let _ = cmd_tx.send(Command::SetApiKey(api));
                        // optional: kick a refresh if API just appeared
                        let _ = cmd_tx.send(Command::RequestGw2Refresh);

                        // If the path is missing or does not exist, try cache
                        let need_cache = new_path
                            .as_ref()
                            .map(|p| !p.exists())
                            .unwrap_or(true);
                        if need_cache {
                            if let Some(cache) = settings.get("bindings_cache").cloned() {
                                let _ = cmd_tx.send(Command::RestoreBindingsCache(cache));
                            }
                        }
                    }
                    SDE::ApplicationDidLaunch { application } => {
                        log!(logger, "📥 application did launch: {}", application);
                        let _ = cmd_tx.send(Command::StartGw2Adapters);
                    }
                    SDE::ApplicationDidTerminate { application } => {
                        log!(logger, "📥 application did terminate: {}", application);
                        let _ = cmd_tx.send(Command::StopGw2Adapters);
                    }
                    _ => {
                        if
                            let Err(e) = std::panic::catch_unwind(
                                std::panic::AssertUnwindSafe(|| {
                                    // your existing dispatch to ActionManager
                                    let mut out = Vec::new();
                                    action_manager.dispatch(&sd_client, sd_ev, &mut out);
                                    for c in out {
                                        match c {
                                            Command::QueueAction(kc) => {
                                                log!(logger, "🔄 queued action: {:?}", kc);
                                                if state.in_combat {
                                                    // park it and go into fast mode so we can bail ASAP
                                                    state.queue_exec(kc);
                                                    let _ = cmd_tx.send(
                                                        Command::MumbleFastMode(true)
                                                    );
                                                    let _ = cmd_tx.send(Command::RequestGw2Refresh);
                                                } else {
                                                    // not in combat? fire immediately
                                                    let _ = cmd_tx.send(Command::ExecuteAction(kc));
                                                }
                                            }
                                            otherwise => {
                                                let _ = cmd_tx.send(otherwise);
                                            }
                                        }
                                    }
                                })
                            )
                        {
                            log!(logger, "🔥 panic handling SDE event: {:?}", e);
                        }
                    }
                }
            }
            MumbleCombat(in_combat) => {
                state.in_combat = in_combat;
                if !in_combat && state.has_pending() {
                    for kc in state.drain_pending() {
                        let _ = cmd_tx.send(Command::ExecuteAction(kc));
                    }
                    let _ = cmd_tx.send(Command::MumbleFastMode(false));
                }
            }
            MumbleActiveCharacter(character) => {
                action_manager.ctx().active_character.set(Some(character.clone()));
                let mut out = Vec::new();
                action_manager.notify_all(
                    &sd_client,
                    Note::ActiveCharacterChanged { name: character },
                    &mut out
                );
                for c in out {
                    let _ = cmd_tx.send(c);
                }
            }
            Gw2TemplateNames { character, names } => {
                action_manager.ctx().templates.insert(character, names);
                // notify actions that care about templates
                let mut out = Vec::new();
                action_manager.notify_all(&sd_client, Note::TemplatesUpdated, &mut out);
                for c in out {
                    let _ = cmd_tx.send(c);
                }
            }

            BindingsLoaded { .. } => {
                if !did_initial_bindings_load {
                    did_initial_bindings_load = true;
                    log!(logger, "📝 initial bindings load; skipping persist");
                    // no PersistBindingsCache here
                } else {
                    let _ = cmd_tx.send(Command::PersistBindingsCache);
                }
                let _ = cmd_tx.send(Command::PersistBindingsCache);
            }

            Shutdown => {
                log!(logger, "🔄 shutdown requested");
                cmd_tx.send(Command::Quit).unwrap_or_else(|_| {
                    log!(logger, "❌ failed to send Quit command");
                });
            }
        }
    }

    log!(logger, "👋 reactor loop stopping (channel closed)");
    Ok(())
}

// helper: snapshot -> JSON
fn snapshot_bindings_json(
    store: &crate::infra::bindings_manager::BindingsStore
) -> serde_json::Value {
    use serde_json::{ Map, Value };
    let mut obj = Map::new();
    for (kc, kb) in store.get() {
        // key as the numeric id (stable + compact)
        let key = (kc as i32).to_string();
        // KeyBind already derives Serialize
        obj.insert(key, serde_json::to_value(&kb).unwrap_or(Value::Null));
    }
    Value::Object(obj)
}

// helper: JSON -> store
fn restore_bindings_from_json(
    logger: Arc<dyn ActionLog>,
    store: &crate::infra::bindings_manager::BindingsStore,
    json: &serde_json::Value
) {
    use std::collections::HashMap;
    use crate::infra::bindings::{ KeyControl, KeyBind };
    let mut map: HashMap<KeyControl, KeyBind> = HashMap::new();

    let Some(obj) = json.as_object() else {
        log!(logger, "⚠️ bindings_cache is not an object");
        return;
    };

    for (k, v) in obj {
        let Ok(id) = k.parse::<i32>() else {
            continue;
        };
        let Ok(kc) = KeyControl::try_from(id) else {
            continue;
        };
        if let Ok(kb) = serde_json::from_value::<KeyBind>(v.clone()) {
            map.insert(kc, kb);
        }
    }

    if !map.is_empty() {
        store.set(map);
        log!(logger, "✅ restored bindings from cache ({} entries)", store.len());
    } else {
        log!(logger, "⚠️ empty/invalid bindings cache; keeping current map");
    }
}

//
// Command router: translate high-level Commands to infra actions.
// Right now this only routes to Stream Deck (Outgoing) and stubs the rest.
//
fn command_router(
    rx: Receiver<events::Command>,
    sd_out: Sender<Outgoing>,
    app_ev_tx: Sender<events::AppEvent>,
    logger: Arc<dyn ActionLog>,
    plugin_uuid: String,
    ctx: AppCtx,
    bindings_mgr: BindingsManager
) {
    let mut gw2: Option<Gw2Poller> = None;
    let mut mumble: Option<Mumble> = None;

    let mut persist_due: Option<Instant> = None;
    let mut last_hash: u64 = 0;
    let debounce = Duration::from_millis(800);
    let startup_grace_until = Instant::now() + Duration::from_millis(500);

    fn hash_json(v: &serde_json::Value) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{ Hash, Hasher };
        let s = serde_json::to_string(v).unwrap_or_default();
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        h.finish()
    }

    // helper: perform the actual settings write, returns whether it sent anything
    fn flush_persist(
        ctx: &AppCtx,
        sd_out: &Sender<Outgoing>,
        plugin_uuid: &str,
        last_hash: &mut u64,
        logger: &Arc<dyn ActionLog>
    ) -> bool {
        let cache = snapshot_bindings_json(&ctx.bindings_store);
        let new_h = hash_json(&cache);

        // Seed baseline once from existing globals so a restart doesn't force a write
        if *last_hash == 0 {
            if let Some(existing) = ctx.global_settings.get().get("bindings_cache") {
                *last_hash = hash_json(existing);
            }
        }

        if new_h == *last_hash {
            // unchanged; nothing to send
            return false;
        }
        *last_hash = new_h;

        let mut obj = serde_json::Map::new();
        obj.insert("bindings_cache".into(), cache);
        obj.insert("bindings_cache_ts".into(), chrono::Utc::now().to_rfc3339().into());
        let payload = ctx.global_settings.merge(obj);

        let _ = sd_out.send(Outgoing::SetGlobalSettings {
            context: plugin_uuid.to_string(),
            payload,
        });
        log!(logger, "💾 persisted bindings cache to global settings");
        true
    }

    use Command::*;
    loop {
        // 1) handle incoming commands with a short timeout so we can tick the debounce timer
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(cmd) =>
                match cmd {
                    Log(msg) => log!(logger, "{}", msg),

                    StartGw2Adapters => {
                        log!(logger, "🟢 adapters started");
                    }

                    StopGw2Adapters => {
                        if let Some(h) = gw2.take() {
                            h.shutdown();
                        }
                        if let Some(h) = mumble.take() {
                            h.shutdown();
                        }
                        log!(logger, "🔴 adapters stopped");
                    }

                    RequestGw2Refresh => {
                        if let Some(ref h) = gw2 {
                            h.force_refresh();
                        }
                        log!(logger, "🔁 gw2 refresh requested");
                    }

                    SetApiKey(_) => {
                        // intentionally no-op here for now
                    }

                    SdSend(msg) => {
                        let _ = sd_out.send(msg);
                    }

                    MumbleFastMode(fast) => {
                        if let Some(ref m) = mumble {
                            m.set_fast(fast);
                        }
                    }

                    SetBindingsPath(p) => {
                        if let Some(ref p) = p {
                            log!(logger, "🧩 set bindings path: {}", p.display());
                        } else {
                            log!(logger, "🧩 clear bindings path (staying on last good in memory)");
                        }
                        bindings_mgr.set_path(p);
                    }

                    ExecuteAction(kc) => {
                        let _used = ctx.bindings_store.execute(kc, Arc::clone(&logger));
                    }

                    QueueAction(_) => {
                        log!(logger, "⚠️ unexpected QueueAction command; this should not happen");
                    }

                    RestoreBindingsCache(json) => {
                        restore_bindings_from_json(Arc::clone(&logger), &ctx.bindings_store, &json);
                        let _ = app_ev_tx.send(AppEvent::BindingsLoaded {
                            count: ctx.bindings_store.len(),
                            path: None,
                        });
                    }

                    // <<< CHANGED: don't send immediately; just schedule
                    PersistBindingsCache => {
                        persist_due = Some(Instant::now() + debounce);
                    }

                    Quit => {
                        // flush any pending persist before quitting
                        if persist_due.is_some() && Instant::now() >= startup_grace_until {
                            let _ = flush_persist(
                                &ctx,
                                &sd_out,
                                &plugin_uuid,
                                &mut last_hash,
                                &logger
                            );
                            persist_due = None;
                        }

                        log!(logger, "👋 plugin shutdown requested");
                        if let Some(h) = gw2.take() {
                            h.shutdown();
                        }
                        if let Some(h) = mumble.take() {
                            h.shutdown();
                        }
                        bindings_mgr.shutdown();
                        break;
                    }
                }

            Err(RecvTimeoutError::Timeout) => {/* fall through to debounce tick below */}
            Err(RecvTimeoutError::Disconnected) => {
                break;
            }
        }

        // 2) debounce tick
        if let Some(due) = persist_due {
            if Instant::now() >= due && Instant::now() >= startup_grace_until {
                let _ = flush_persist(&ctx, &sd_out, &plugin_uuid, &mut last_hash, &logger);
                persist_due = None;
            }
        }
    }

    if let Some(h) = gw2.take() {
        h.shutdown();
    }
}
