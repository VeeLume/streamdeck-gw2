use std::path::PathBuf;

use crate::infra::bindings::KeyControl;
use crate::infra::sd_protocol::{ Outgoing, StreamDeckEvent };

/// Everything that can happen in the app flows through here.
/// Stream Deck events, adapter updates, lifecycle, etc.
#[derive(Debug, Clone)]
pub enum AppEvent {
    // From Stream Deck transport (parsed/typed)
    StreamDeck(StreamDeckEvent),

    // From adapters (wire these later)
    Gw2TemplateNames {
        character: String,
        names: TemplateNames,
    },
    MumbleActiveCharacter(String),
    MumbleCombat(bool),
    BindingsLoaded {
        count: usize,
        path: Option<PathBuf>,
    },
    // Lifecycle / control
    Shutdown,
}

/// Commands the reactor emits for infra/adapters to execute.
/// The command_router owns the effects.
#[derive(Debug, Clone)]
pub enum Command {
    Log(String),
    // Stream Deck (UI) messages
    SdSend(Outgoing),

    // Config
    SetApiKey(Option<String>),
    SetBindingsPath(Option<std::path::PathBuf>),

    // Adapters
    RequestGw2Refresh,
    MumbleFastMode(bool), // true = 16ms polling, false = 10s
    ExecuteAction(KeyControl), // use current bindings
    QueueAction(KeyControl),
    PersistBindingsCache,
    RestoreBindingsCache(serde_json::Value),

    // Gw2 Poller
    StartGw2Adapters,
    StopGw2Adapters,

    // Control
    Quit,
}

/// Convenience: let callers write `cmd_tx.send(Outgoing::SetTitle{...}.into())`
impl From<Outgoing> for Command {
    fn from(o: Outgoing) -> Self {
        Command::SdSend(o)
    }
}

/// Optional domain stubs so you can compile before wiring GW2.
/// Flesh these out when you add the poller.
#[derive(Debug, Clone)]
pub struct Gw2Character {
    pub name: String,
    // add id, profession, etc. as needed
}

#[derive(Debug, Clone, Default)]
pub struct TemplateNames {
    /// 1-based slots -> optional display names
    pub build: [Option<String>; 9],
    pub equipment: [Option<String>; 9],
}
