use std::collections::{ HashMap, VecDeque };
use std::sync::{ Arc, Mutex };

use crossbeam_channel::Sender;

use crate::bindings::{ key_control::KeyControl, KeyBind };
use crate::plugin_state::runtime::SupervisorMsg;

/// Everything handlers need to *read* (shared state).
pub struct AppContext {
    pub identity_callbacks: Arc<Mutex<HashMap<String, crate::plugin_state::IdentityCallback>>>,
    pub character_data: Arc<Mutex<HashMap<String, crate::plugin_state::poller::CharacterData>>>,
    pub plugin_uuid: String,
    pub global_settings: Arc<Mutex<serde_json::Map<String, serde_json::Value>>>,
}

/// The only way a handler mutates the world is by sending a command.
#[derive(Clone)]
pub struct Controller {
    tx: Sender<SupervisorMsg>,
}

impl Controller {
    pub fn new(tx: Sender<SupervisorMsg>) -> Self {
        Self { tx }
    }

    pub fn set_api_key(&self, key: Option<String>) {
        let _ = self.tx.send(SupervisorMsg::SetApiKey(key));
    }
    pub fn set_bindings_file(&self, path: Option<String>) {
        let _ = self.tx.send(SupervisorMsg::SetBindingsFile(path));
    }
    pub fn set_bindings(&self, b: HashMap<KeyControl, KeyBind>) {
        let _ = self.tx.send(SupervisorMsg::SetBindings(b));
    }
    pub fn queue_action(&self, action: i32, allow_in_combat: bool) {
        let _ = self.tx.send(
            SupervisorMsg::QueueAction(crate::plugin_state::QueuedAction {
                action,
                allow_in_combat,
            })
        );
    }

    // Lifecycle (used by plugin.rs on app events)
    pub fn app_launched(&self) {
        let _ = self.tx.send(SupervisorMsg::AppLaunched);
    }
    pub fn app_terminated(&self) {
        let _ = self.tx.send(SupervisorMsg::AppTerminated);
    }
}
