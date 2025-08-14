use std::collections::VecDeque;

use crate::infra::bindings::KeyControl;

/// Minimal reactor-owned state for now.
/// We’ll grow this when GW2/mumble adapters land.
#[derive(Default)]
pub struct PluginState {
    pub in_combat: bool,
    pending_execs: VecDeque<KeyControl>,
}

impl PluginState {
    pub fn queue_exec(&mut self, kc: KeyControl) {
        self.pending_execs.push_back(kc);
    }

    pub fn drain_pending(&mut self) -> impl Iterator<Item = KeyControl> + '_ {
        std::iter::from_fn(move || self.pending_execs.pop_front())
    }

    pub fn has_pending(&self) -> bool {
        !self.pending_execs.is_empty()
    }
}
