use std::{
    collections::{ HashMap, VecDeque },
    sync::{ Arc, Mutex },
    thread::JoinHandle,
    time::Duration,
};

use crossbeam_channel::{ unbounded, Receiver, Sender, select, tick };
use notify::Watcher;
use crate::{
    bindings::{
        key_code::{ KeyCode, MouseCode },
        key_control::KeyControl,
        load_input_bindings,
        send_keys::{ send_keyboard_input, send_mouse_input },
        DeviceType,
        KeyBind,
    },
    config,
    log,
    logger::ActionLog,
    plugin_state::{
        mumble::{ Identity, MumbleLink, UiState },
        poller::{ CharacterData, Gw2Poller },
        IdentityCallback,
        QueuedAction,
    },
};

// If you already have these in plugin_state::mod.rs, you can import them instead of redefining:
//   use crate::plugin_state::{ IdentityCallback, QueuedAction };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Stopped,
    Running,
}

#[derive(Debug)]
pub enum SupervisorMsg {
    AppLaunched,
    AppTerminated,
    SetApiKey(Option<String>),
    SetBindingsFile(Option<String>),
    SetBindings(HashMap<KeyControl, KeyBind>),
    QueueAction(QueuedAction),
}

#[derive(Debug)]
enum WorkerCtrl {
    Stop,
}

pub struct PluginRuntime {
    // lifecycled
    mumble_thread: Option<JoinHandle<()>>,
    file_watcher_thread: Option<JoinHandle<()>>,
    gw2_poller: Option<Gw2Poller>,

    // shared data
    pub logger: Arc<dyn ActionLog>,
    pub bindings: Arc<Mutex<HashMap<KeyControl, KeyBind>>>,
    pub queued_actions: Arc<Mutex<VecDeque<QueuedAction>>>,
    pub current_identity: Arc<Mutex<Option<Identity>>>,
    pub identity_callbacks: Arc<Mutex<HashMap<String, IdentityCallback>>>,
    pub character_data: Arc<Mutex<HashMap<String, CharacterData>>>,

    // cfg-ish
    api_key: Option<String>,
    bindings_file: Option<String>,

    // worker ctrl channels
    mumble_tx: Option<Sender<WorkerCtrl>>,
    watcher_tx: Option<Sender<WorkerCtrl>>,

    // run state
    state: RunState,

    // inbound control for supervisor thread created by caller
    rx: Receiver<SupervisorMsg>,
    tx: Sender<SupervisorMsg>,
}

impl PluginRuntime {
    pub fn new(logger: Arc<dyn ActionLog>, cfg: config::Config) -> (Self, Receiver<SupervisorMsg>) {
        let (tx, rx) = unbounded::<SupervisorMsg>();

        let bindings = if let Some(ref path) = cfg.bindings_file {
            load_input_bindings(path, Arc::clone(&logger))
        } else {
            cfg.bindings_cache.unwrap_or_default()
        };

        let rt = Self {
            mumble_thread: None,
            file_watcher_thread: None,
            gw2_poller: None,

            logger,
            bindings: Arc::new(Mutex::new(bindings)),
            queued_actions: Arc::new(Mutex::new(VecDeque::new())),
            current_identity: Arc::new(Mutex::new(None)),
            identity_callbacks: Arc::new(Mutex::new(HashMap::new())),
            character_data: Arc::new(Mutex::new(HashMap::new())),

            api_key: cfg.api_key,
            bindings_file: cfg.bindings_file,

            mumble_tx: None,
            watcher_tx: None,

            state: RunState::Stopped,
            rx,
            tx,
        };
        let rx_clone = rt.rx.clone();
        (rt, rx_clone)
    }

    pub fn sender(&self) -> Sender<SupervisorMsg> {
        self.tx.clone()
    }

    pub fn handle(&mut self, msg: SupervisorMsg) {
        match msg {
            SupervisorMsg::AppLaunched => {
                self.start_all();
                self.restart_poller_if_needed();
            }
            SupervisorMsg::AppTerminated => self.stop_all(),
            SupervisorMsg::SetApiKey(k) => {
                self.api_key = k;
                let cfg = config::Config {
                    api_key: self.api_key.clone(),
                    bindings_file: self.bindings_file.clone(),
                    bindings_cache: Some(
                        self.bindings
                            .lock()
                            .ok()
                            .map(|b| b.clone())
                            .unwrap_or_default()
                    ),
                };
                let _ = cfg.save();
                self.restart_poller_if_needed();
            }
            SupervisorMsg::SetBindingsFile(path) => {
                self.bindings_file = path;
                self.start_binding_watcher();
                let cfg = config::Config {
                    api_key: self.api_key.clone(),
                    bindings_file: self.bindings_file.clone(),
                    bindings_cache: Some(
                        self.bindings
                            .lock()
                            .ok()
                            .map(|b| b.clone())
                            .unwrap_or_default()
                    ),
                };
                let _ = cfg.save();
            }
            SupervisorMsg::SetBindings(b) => {
                if let Ok(mut cur) = self.bindings.lock() {
                    *cur = b;
                }
                let cfg = config::Config {
                    api_key: self.api_key.clone(),
                    bindings_file: self.bindings_file.clone(),
                    bindings_cache: Some(
                        self.bindings
                            .lock()
                            .ok()
                            .map(|b| b.clone())
                            .unwrap_or_default()
                    ),
                };
                let _ = cfg.save();
            }
            SupervisorMsg::QueueAction(a) => {
                if let Ok(mut q) = self.queued_actions.lock() {
                    q.push_back(a);
                }
            }
        }
    }

    fn start_all(&mut self) {
        if self.state == RunState::Running {
            return;
        }
        self.state = RunState::Running;
        self.start_mumble();
        self.start_binding_watcher();
        self.maybe_start_poller();
        log!(self.logger, "✅ Supervisor: all started");
    }
    fn stop_all(&mut self) {
        if self.state == RunState::Stopped {
            return;
        }
        self.state = RunState::Stopped;
        self.stop_poller();
        self.stop_mumble();
        self.stop_watcher();
        log!(self.logger, "🛑 Supervisor: all stopped");
    }

    fn maybe_start_poller(&mut self) {
        if self.gw2_poller.is_some() {
            return;
        }
        if !(self.state == RunState::Running && self.api_key.is_some()) {
            return;
        }
        let key = self.api_key.clone().unwrap();
        self.gw2_poller = Some(
            Gw2Poller::start(key, Arc::clone(&self.character_data), Arc::clone(&self.logger))
        );
        log!(self.logger, "🚀 GW2 poller started");
    }
    fn restart_poller_if_needed(&mut self) {
        match (self.state, self.api_key.is_some()) {
            (RunState::Running, true) => if self.gw2_poller.is_none() {
                self.maybe_start_poller();
            }
            _ => self.stop_poller(),
        }
    }
    fn stop_poller(&mut self) {
        if let Some(mut p) = self.gw2_poller.take() {
            p.stop();
            log!(self.logger, "🛑 GW2 poller stopped");
        }
    }

    fn start_mumble(&mut self) {
        self.stop_mumble();
        let (tx, rx) = unbounded::<WorkerCtrl>();

        let logger = Arc::clone(&self.logger);
        let queued_actions = Arc::clone(&self.queued_actions);
        let bindings = Arc::clone(&self.bindings);
        let current_identity = Arc::clone(&self.current_identity);
        let callbacks = Arc::clone(&self.identity_callbacks);

        let th = std::thread::spawn(move || {
            // retry init but stay responsive
            let retry = tick(Duration::from_millis(800));
            let link = loop {
                match MumbleLink::new() {
                    Ok(l) => {
                        break l;
                    }
                    Err(e) => {
                        log!(logger, "❌ MumbleLink init failed: {}. Retrying…", e);
                        select! {
                            recv(rx) -> _ => { log!(logger, "📴 Mumble: stop during init"); return; }
                            recv(retry) -> _ => {}
                        }
                    }
                }
            };
            log!(logger, "📡 Mumble thread started");
            let cadence = tick(Duration::from_millis(8));
            loop {
                select! {
                    recv(rx) -> _ => { log!(logger, "📴 Mumble thread stopping"); break; }
                    recv(cadence) -> _ => {
                        if let Some((_, maybe_ctx, maybe_identity)) = link.read(true) {
                            if let Some(id) = maybe_identity {
                                if let Ok(mut cur) = current_identity.lock() { *cur = Some(id.clone()); }
                                if let Ok(cbs) = callbacks.lock() {
                                    // FREE FUNCTION (no PluginState type needed)
                                    trigger_identity_callbacks_locked(
                                        Arc::clone(&logger), &Some(id), &*cbs
                                    );
                                }
                            }
                            if let Some(ctx) = maybe_ctx {
                                let ui_state = UiState::from_raw(ctx.ui_state);
                                let in_combat = ui_state.is_in_combat();

                                let next = {
                                    let mut q = match queued_actions.lock() { Ok(q) => q, Err(_) => continue };
                                    if let Some(front) = q.front() {
                                        if !in_combat || front.allow_in_combat { q.pop_front() } else { None }
                                    } else { None }
                                };
                                if let Some(a) = next {
                                    let binds = bindings.lock().ok().map(|b| b.clone());
                                    if let Some(binds) = binds {
                                        // FREE FUNCTION (no PluginState type needed)
                                        press_action_key(Arc::clone(&logger), binds, a.action);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            log!(logger, "🏁 Mumble thread exited");
        });

        self.mumble_tx = Some(tx);
        self.mumble_thread = Some(th);
    }
    fn stop_mumble(&mut self) {
        if let Some(tx) = self.mumble_tx.take() {
            let _ = tx.send(WorkerCtrl::Stop);
        }
        if let Some(th) = self.mumble_thread.take() {
            let _ = th.join();
        }
    }

    fn start_binding_watcher(&mut self) {
        self.stop_watcher();
        let Some(path) = self.bindings_file.clone() else {
            log!(self.logger, "⚠️ No bindings file path set; watcher not started");
            return;
        };

        let (tx, rx) = unbounded::<WorkerCtrl>();
        let logger = Arc::clone(&self.logger);
        let bindings = Arc::clone(&self.bindings);
        let path_buf = std::path::PathBuf::from(path.clone());

        let th = std::thread::spawn(move || {
            let (ntx, nrx) = std::sync::mpsc::channel();
            let mut watcher = match notify::recommended_watcher(ntx) {
                Ok(w) => w,
                Err(e) => {
                    log!(logger, "❌ watcher create: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&path_buf, notify::RecursiveMode::NonRecursive) {
                log!(logger, "❌ watcher watch: {e}");
                return;
            }
            log!(logger, "👀 Watching bindings file: {}", path);

            let cadence = tick(Duration::from_millis(100));
            loop {
                select! {
                    recv(rx) -> _ => { log!(logger, "📴 Watcher stopping"); break; }
                    recv(cadence) -> _ => {
                        while let Ok(Ok(ev)) = nrx.try_recv() {
                            if matches!(ev.kind, notify::EventKind::Modify(_)) {
                                let updated = load_input_bindings(&path, Arc::clone(&logger));
                                if let Ok(mut b) = bindings.lock() { *b = updated; }
                                log!(logger, "🔄 Reloaded bindings after file change");
                            }
                        }
                    }
                }
            }
            log!(logger, "🏁 Watcher exited");
        });

        self.watcher_tx = Some(tx);
        self.file_watcher_thread = Some(th);
    }
    fn stop_watcher(&mut self) {
        if let Some(tx) = self.watcher_tx.take() {
            let _ = tx.send(WorkerCtrl::Stop);
        }
        if let Some(th) = self.file_watcher_thread.take() {
            let _ = th.join();
        }
    }
}

/* ---------- Free helpers (no dependency on old PluginState) ---------- */

fn trigger_identity_callbacks_locked(
    logger: Arc<dyn ActionLog>,
    identity: &Option<Identity>,
    callbacks: &HashMap<String, IdentityCallback>
) {
    if let Some(identity) = identity {
        log!(logger, "🔄 Triggering identity callbacks for: {:?}", identity);
        for (_, cb) in callbacks.iter() {
            cb(identity);
        }
    }
}

fn press_action_key(
    logger: Arc<dyn ActionLog>,
    bindings: HashMap<KeyControl, KeyBind>,
    control_id: i32
) {
    let control = match KeyControl::try_from(control_id) {
        Ok(ctrl) => ctrl,
        Err(_) => {
            log!(logger, "❌ Invalid KeyControl ID: {}", control_id);
            return;
        }
    };
    let Some(keybind) = bindings.get(&control) else {
        log!(logger, "⚠️ No binding found for {:?}", control);
        return;
    };
    let Some(key) = keybind.primary.as_ref().or(keybind.secondary.as_ref()) else {
        log!(logger, "⚠️ No key assigned to {:?}", control);
        return;
    };
    match key.device_type {
        DeviceType::Keyboard => {
            let keycode: KeyCode = match KeyCode::try_from(key.code) {
                Ok(k) => k,
                Err(_) => {
                    log!(logger, "❌ Invalid keyboard key code: {}", key.code);
                    return;
                }
            };
            log!(logger, "Pressing key: {:?}", keycode);
            send_keyboard_input(Arc::clone(&logger), keycode, &key.modifier);
        }
        DeviceType::Mouse => {
            let mouse_code = match MouseCode::try_from(key.code) {
                Ok(c) => c,
                Err(_) => {
                    log!(logger, "❌ Invalid mouse code: {}", key.code);
                    return;
                }
            };
            if let Some(button) = mouse_code.to_send_input_mouse_button() {
                send_mouse_input(Arc::clone(&logger), button);
            } else {
                log!(logger, "⚠️ Mouse code {:?} not supported for SendInput", mouse_code);
            }
        }
        DeviceType::Unset => log!(logger, "⚠️ Tried to press key with device type UNSET"),
    }
}
