#![cfg(not(windows))]

use crossbeam_channel::Sender;
use crate::core::events::AppEvent;

pub struct Mumble;
impl Mumble {
    pub fn spawn(_: std::sync::Arc<dyn crate::logger::ActionLog>, _: Sender<AppEvent>) -> Self {
        Self
    }
    pub fn set_fast(&self, _: bool) {}
    pub fn shutdown(self) {}
}
