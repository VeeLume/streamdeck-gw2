use crate::app::context::AppCtx;
use crate::core::{ events::Command, sd_client::SdClient };
use crate::infra::sd_protocol::StreamDeckEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    TemplatesUpdated,
    ActiveCharacterChanged { name: String },
}

/// One instance per Stream Deck `context` (i.e., per key on the deck).
pub trait Action: Send {
    /// Called on `willAppear` when the instance is created.
    fn init(&mut self, _sd: &SdClient, _ctx: &AppCtx, _context: String) {}

    /// Handle a typed SD event and emit zero or more Commands.
    fn on_event(
        &mut self,
        sd: &SdClient,
        ctx: &AppCtx,
        ev: &StreamDeckEvent,
        out: &mut Vec<Command>
    );

    fn on_notify(
        &mut self,
        _sd: &SdClient,
        _ctx: &AppCtx,
        _note: Note,
        _out: &mut Vec<Command>
    ) {
        // Default noop; override if you need to handle notifications
    }
}

/// Factory to create fresh instances for each `context`.
pub trait ActionFactory: Send + Sync {
    /// Stream Deck action UUID from your manifest.
    fn kind(&self) -> &'static str;
    fn create(&self) -> Box<dyn Action>;
}
