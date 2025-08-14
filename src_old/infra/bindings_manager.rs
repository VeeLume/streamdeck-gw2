use std::{
    collections::HashMap,
    panic::{ catch_unwind, AssertUnwindSafe },
    path::{ Path, PathBuf },
    sync::{ Arc, RwLock },
    thread,
    time::{ Duration, Instant },
};

use crossbeam_channel::{ unbounded, Receiver };
use notify::{ RecommendedWatcher, Watcher, EventKind, RecursiveMode };

use crate::{
    infra::{
        bindings::{ default_input_bindings, load_input_bindings },
        send_keys,
        DeviceType,
        KeyBind,
        KeyCode,
        KeyControl,
        MouseCode,
    },
    logger::ActionLog,
};
use crate::{ log };
use crate::core::events::AppEvent;

/// Shared, always-available bindings map
#[derive(Clone, Default)]
pub struct BindingsStore(pub Arc<RwLock<HashMap<KeyControl, KeyBind>>>);

impl BindingsStore {
    pub fn get(&self) -> HashMap<KeyControl, KeyBind> {
        self.0
            .read()
            .ok()
            .map(|m| m.clone())
            .unwrap_or_default()
    }
    pub fn set(&self, v: HashMap<KeyControl, KeyBind>) {
        if let Ok(mut w) = self.0.write() {
            *w = v;
        }
    }
    pub fn len(&self) -> usize {
        self.0
            .read()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Look up the binding for `kc` and send the input (primary then secondary).
    /// Returns true if anything was sent.
    pub fn execute(&self, kc: KeyControl, logger: Arc<dyn ActionLog>) -> bool {
        let map = self.get();
        let Some(kb) = map.get(&kc) else {
            log!(logger, "⚠️ no binding for {:?}", kc);
            return false;
        };

        // Try primary first, then secondary
        let try_keys = [kb.primary.as_ref(), kb.secondary.as_ref()];

        for k in try_keys.into_iter().flatten() {
            match k.device_type {
                DeviceType::Keyboard => {
                    match KeyCode::try_from(k.code) {
                        Ok(code) => {
                            send_keys::send_keyboard_input(Arc::clone(&logger), code, &k.modifier);
                            return true;
                        }
                        Err(_) => {
                            log!(logger, "❌ unknown keyboard code {} for {:?}", k.code, kc);
                        }
                    }
                }
                DeviceType::Mouse => {
                    match MouseCode::try_from(k.code) {
                        Ok(mc) => {
                            if let Some(btn) = mc.to_send_input_mouse_button() {
                                send_keys::send_mouse_input(Arc::clone(&logger), btn);
                                return true;
                            } else {
                                log!(logger, "⚠️ mouse code {:?} not supported for {:?}", mc, kc);
                            }
                        }
                        Err(_) => {
                            log!(logger, "❌ unknown mouse code {} for {:?}", k.code, kc);
                        }
                    }
                }
                DeviceType::Unset => {/* unbound; try next */}
            }
        }

        log!(logger, "⚠️ binding {:?} has no actionable keys", kc);
        false
    }
}

enum Msg {
    SetPath(Option<PathBuf>),
    Shutdown,
}

pub struct BindingsManager {
    tx: crossbeam_channel::Sender<Msg>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl BindingsManager {
    pub fn spawn(
        logger: Arc<dyn ActionLog>,
        app_tx: crossbeam_channel::Sender<AppEvent>
    ) -> (Self, BindingsStore) {
        let (tx, rx) = unbounded::<Msg>();
        let store = BindingsStore::default();

        let store_clone = store.clone();
        let join = thread::spawn(move || {
            if
                let Err(p) = catch_unwind(
                    AssertUnwindSafe(|| { run(logger.clone(), app_tx, rx, store_clone) })
                )
            {
                log!(logger, "❌ reader thread panicked: {:?}", p);
            }
        });

        (Self { tx, join: Some(join) }, store)
    }

    /// Change watched file (None = stop watching, keep last good bindings)
    pub fn set_path(&self, path: Option<PathBuf>) {
        let _ = self.tx.send(Msg::SetPath(path));
    }

    pub fn shutdown(mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for BindingsManager {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn run(
    logger: Arc<dyn ActionLog>,
    app_tx: crossbeam_channel::Sender<AppEvent>,
    rx: crossbeam_channel::Receiver<Msg>,
    store: BindingsStore
) {
    // always start with defaults in memory
    store.set(default_input_bindings());
    let _ = app_tx.send(AppEvent::BindingsLoaded { count: store.len(), path: None });

    let mut cur_path: Option<PathBuf> = None;
    let mut watcher: Option<RecommendedWatcher> = None;

    log!(logger, "🔭 bindings manager started");

    loop {
        match rx.recv() {
            Ok(Msg::SetPath(path)) => {
                // no-op if unchanged
                if path == cur_path {
                    // log!(logger, "bindings path unchanged; ignoring");
                    continue;
                }

                // tear down old watcher
                if let Some(mut w) = watcher.take() {
                    if let Some(ref path) = cur_path {
                        let _ = w.unwatch(path);
                    }
                }

                cur_path = path.clone();

                if let Some(ref p) = cur_path {
                    // initial load
                    load_into_store(&logger, &store, p, &app_tx);

                    // start watching
                    let (event_tx, event_rx) = unbounded();

                    let mut w = match RecommendedWatcher::new(
                        move |res: notify::Result<notify::Event>| {
                            let _ = event_tx.send(res);
                        },
                        notify::Config::default()
                    ) {
                        Ok(w) => w,
                        Err(e) => {
                            log!(logger, "❌ failed to create watcher: {}", e);
                            continue;
                        }
                    };

                    if let Err(e) = w.watch(p, RecursiveMode::NonRecursive) {
                        log!(logger, "❌ watch failed for {}: {}", p.display(), e);
                    } else {
                        log!(logger, "👀 watching {}", p.display());
                        watcher = Some(w);
                    }

                    // spawn a tiny pump to debounce and reload
                    let logger2 = Arc::clone(&logger);
                    let store2 = store.clone();
                    let app_tx2 = app_tx.clone();
                    let path2 = p.clone();
                    thread::spawn(move || pump_events(event_rx, logger2, store2, app_tx2, path2));
                } else {
                    log!(logger, "🪵 no bindings file; keeping last good (defaults or previous)");
                    let _ = app_tx.send(AppEvent::BindingsLoaded {
                        count: store.len(),
                        path: None,
                    });
                }
            }
            Ok(Msg::Shutdown) | Err(_) => {
                break;
            }
        }
    }

    log!(logger, "🛑 bindings manager stopped");
}

fn pump_events(
    rx: Receiver<notify::Result<notify::Event>>,
    logger: Arc<dyn ActionLog>,
    store: BindingsStore,
    app_tx: crossbeam_channel::Sender<AppEvent>,
    path: PathBuf
) {
    let mut last_load: Option<Instant> = None;
    const RELOAD_DEBOUNCE: Duration = Duration::from_millis(200);

    for ev in rx.iter() {
        match ev {
            Ok(event) => {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any => {
                        let now = Instant::now();
                        if
                            last_load
                                .map(|t| now.duration_since(t) < RELOAD_DEBOUNCE)
                                .unwrap_or(false)
                        {
                            continue;
                        }
                        last_load = Some(now);
                        load_into_store(&logger, &store, &path, &app_tx);
                    }
                    EventKind::Remove(_) => {
                        log!(logger, "🗑️ bindings file removed: {}", path.display());
                        // keep last good map in memory; just log
                    }
                    _ => {}
                }
            }
            Err(e) => log!(logger, "⚠️ watcher error: {}", e),
        }
    }
}

fn load_into_store(
    logger: &Arc<dyn ActionLog>,
    store: &BindingsStore,
    path: &Path,
    app_tx: &crossbeam_channel::Sender<AppEvent>
) {
    let map = load_input_bindings(path, Arc::clone(logger));
    let count = map.len();
    store.set(map);
    log!(logger, "✅ bindings loaded ({count}) from {}", path.display());
    let _ = app_tx.send(AppEvent::BindingsLoaded { count, path: Some(path.to_path_buf()) });
}
