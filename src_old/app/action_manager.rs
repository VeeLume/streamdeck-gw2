use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::app::action::Note;
use crate::app::context::AppCtx;
use crate::core::{ events::Command, sd_client::SdClient };
use crate::infra::sd_protocol::StreamDeckEvent;

use super::action::{ Action, ActionFactory };

pub struct ActionManager {
    registry: HashMap<&'static str, &'static dyn ActionFactory>,
    instances: HashMap<String, Box<dyn Action>>, // key: context
    ctx: AppCtx,
}

impl ActionManager {
    pub fn new(registry: Vec<&'static dyn ActionFactory>, ctx: AppCtx) -> Self {
        let mut map = HashMap::new();
        for f in registry {
            map.insert(f.kind(), f);
        }
        Self { registry: map, instances: HashMap::new(), ctx }
    }

    pub fn dispatch(&mut self, sd: &SdClient, ev: StreamDeckEvent, out: &mut Vec<Command>) {
        use StreamDeckEvent::*;
        match &ev {
            StreamDeckEvent::WillAppear { action, context, .. } => {
                // 1) Find the factory first; bail if not registered
                let Some(factory) = self.registry.get(action.as_str()) else {
                    // log here if you want
                    // log!(logger, "unknown action kind: {}", action);
                    out.push(Command::Log(format!("⚠️ unknown action kind: {}", action)));
                    return;
                };

                // 2) Insert-or-get the instance; init only on first create
                match self.instances.entry(context.clone()) {
                    Entry::Occupied(mut e) => {
                        e.get_mut().on_event(sd, &self.ctx, &ev, out);
                    }
                    Entry::Vacant(v) => {
                        let mut a = factory.create();
                        a.init(sd, &self.ctx, context.clone());
                        let a = v.insert(a);
                        a.on_event(sd, &self.ctx, &ev, out);
                    }
                }
            }
            WillDisappear { context, .. } => {
                if let Some(inst) = self.instances.get_mut(context) {
                    inst.on_event(sd, &self.ctx, &ev, out);
                }
                self.instances.remove(context);
            }
            _ => {
                let context = match &ev {
                    | DialDown { context, .. }
                    | DialRotate { context, .. }
                    | DialUp { context, .. }
                    | DidReceivePropertyInspectorMessage { context, .. }
                    | DidReceiveSettings { context, .. }
                    | KeyDown { context, .. }
                    | KeyUp { context, .. }
                    | PropertyInspectorDidAppear { context, .. }
                    | PropertyInspectorDidDisappear { context, .. }
                    | TitleParametersDidChange { context, .. }
                    | TouchTap { context, .. } => Some(context.clone()),
                    _ => None,
                };

                if let Some(ctx) = context {
                    if let Some(inst) = self.instances.get_mut(&ctx) {
                        inst.on_event(sd, &self.ctx, &ev, out);
                    }
                }
            }
        }
    }

    /// Broadcast a note to all live action instances.
    pub fn notify_all(&mut self, sd: &SdClient, note: Note, out: &mut Vec<Command>) {
        for inst in self.instances.values_mut() {
            inst.on_notify(sd, &self.ctx, note.clone(), out);
        }
    }

    /// (Optional) Notify a single context.
    pub fn notify_context(
        &mut self,
        sd: &SdClient,
        context: &str,
        note: Note,
        out: &mut Vec<Command>
    ) {
        if let Some(inst) = self.instances.get_mut(context) {
            inst.on_notify(sd, &self.ctx, note, out);
        }
    }

    pub fn ctx(&self) -> &AppCtx {
        &self.ctx
    }
    pub fn ctx_mut(&mut self) -> &mut AppCtx {
        &mut self.ctx
    }
}
