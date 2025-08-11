pub mod runtime;
pub mod mumble;
pub mod poller;

pub type IdentityCallback = Box<dyn Fn(&mumble::Identity) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct QueuedAction {
    pub action: i32,
    pub allow_in_combat: bool,
}
