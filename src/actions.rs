use std::sync::{ atomic::{ AtomicBool, Ordering }, Arc };

use constcat::concat;
use streamdeck_lib::{
    actions::Action,
    context::Context,
    debug,
    info,
    sd_protocol::views::*,
};

use crate::PLUGIN_ID;

#[derive(Default)]
pub struct SetTemplateAction;

impl Action for SetTemplateAction {
    fn id(&self) -> &str {
        concat!(PLUGIN_ID, ".set-template")
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        info!(cx.log(), "SetTemplateAction initialized for context: {}", ctx_id);
    }

    fn will_appear(&mut self, cx: &Context, ev: &WillAppear) {
        info!(cx.log(), "SetTemplateAction will_appear: {:?}", ev.context);
    }

    fn key_down(&mut self, cx: &Context, ev: &KeyDown) {
        debug!(cx.log(), "SetTemplateAction key_down for ctx_id: {}", ev.context);
    }

    fn key_up(&mut self, cx: &Context, ev: &KeyUp) {
        info!(cx.log(), "HelloAction key_up for ctx_id: {}", ev.context);
    }
}
