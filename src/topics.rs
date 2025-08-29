use streamdeck_lib::prelude::*;

use crate::gw2::enums::{CharacterChange, KeyControl, TemplateNames};

pub const MUMBLE_ACTIVE_CHARACTER: TopicId<Option<String>> =
    TopicId::new("mumble.active-character");

pub const GW2_API_GET_CHARACTERS: TopicId<()> = TopicId::new("gw2-api.get-characters");
pub const GW2_API_TEMPLATE_CHANGED: TopicId<Gw2ApiTemplateChanged> =
    TopicId::new("gw2-api.template-changed");
#[derive(Debug, Clone)]
pub struct Gw2ApiTemplateChanged {
    pub name: String,
    pub before: TemplateNames,
    pub after: TemplateNames,
}
pub const GW2_API_FETCHED: TopicId<Gw2ApiFetched> = TopicId::new("gw2-api.fetched");
#[derive(Debug, Clone)]
pub struct Gw2ApiFetched {
    pub total: usize,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
}
pub const GW2_API_CHARACTER_CHANGED: TopicId<Gw2ApiCharacterChanged> =
    TopicId::new("gw2-api.character-changed");
#[derive(Debug, Clone)]
pub struct Gw2ApiCharacterChanged {
    pub name: String,
    pub change: CharacterChange,
}

pub const GW2_ANIMATION_TICK: TopicId<()> = TopicId::new("gw2.animation.tick");
pub const GW2_EXEC_PROGRESS: TopicId<ExecState> = TopicId::new("gw2.exec_progress");
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecState {
    Queued,
    Started,
    Done,
}

pub const GW2_EXEC_QUEUE: TopicId<Gw2ExecQueue> = TopicId::new("gw2-exec.queue");
#[derive(Debug, Clone)]
pub struct Gw2ExecQueue {
    pub controls: Vec<KeyControl>,
    pub allow_in_combat: bool,
    pub allow_out_of_combat: bool,
    pub allow_gliding_or_falling: bool,
    pub inter_control_ms: Option<u64>,
    pub origin_ctx: String,
}

pub const GW2_BINDINGS_UPDATED: TopicId<()> = TopicId::new("gw2.bindings.updated");
pub const GW2_BINDINGS_PATH_SET: TopicId<String> = TopicId::new("gw2.bindings.path.set");
pub const GW2_BINDINGS_PATH_RELOAD: TopicId<()> = TopicId::new("gw2.bindings.path.reload");
