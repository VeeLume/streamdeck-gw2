use constcat::concat;
use crate::app::{action, context};
use crate::app::context::AppCtx;
use crate::core::{ events::Command, sd_client::SdClient };
use crate::infra::bindings::KeyControl;
use crate::infra::sd_protocol::{ StreamDeckEvent, SdState };
use crate::plugin::PLUGIN_UUID;
use super::super::action::{ Action, ActionFactory };

#[derive(Default)]
pub struct SetTemplateAction {
    selected_build: Option<u8>, // 1..=9
    selected_equipment: Option<u8>, // 1..=9
    context: Option<String>, // current context
}

impl SetTemplateAction {
    fn wrap_title(title: &str, max_len: usize) -> String {
        let mut lines = vec![];
        let mut cur = String::new();
        for word in title.split_whitespace() {
            if !cur.is_empty() && cur.len() + 1 + word.len() > max_len {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines.join("\n")
    }

    /// Build a nice title using the in-game names if we have them.
    fn compute_title_from_ctx(&self, ctx: &AppCtx) -> Option<String> {
        let Some(char_name) = ctx.active_character.get() else {
            return None;
        };
        let Some(names) = ctx.templates.get(&char_name) else {
            return None;
        };

        // Adjust field names here if your TemplateNames struct differs:
        // I assume: { builds: Vec<String>, equipment: Vec<String> }
        let mut parts: Vec<String> = Vec::new();

        if let Some(b) = self.selected_build {
            let i = (b as usize).saturating_sub(1);
            if let Some(n) = names.build.get(i) {
                if let Some(n) = n {
                    if !n.is_empty() {
                        parts.push(Self::wrap_title(n, 10));
                    } else {
                        parts.push(format!("Build {b}"));
                    }
                } else {
                    parts.push(format!("Build {b}"));
                }
            } else {
                parts.push(format!("Build {b}"));
            }
        }

        if let Some(e) = self.selected_equipment {
            let i = (e as usize).saturating_sub(1);
            if let Some(n) = names.equipment.get(i) {
                if let Some(n) = n {
                    if !n.is_empty() {
                        parts.push(Self::wrap_title(n, 10));
                    } else {
                        parts.push(format!("Equip {e}"));
                    }
                } else {
                    parts.push(format!("Equip {e}"));
                }
            } else {
                parts.push(format!("Equip {e}"));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    fn compute_fallback_title(&self) -> String {
        match (self.selected_build, self.selected_equipment) {
            (Some(b), Some(e)) => format!("B{b} / E{e}"),
            (Some(b), None) => format!("B{b}"),
            (None, Some(e)) => format!("E{e}"),
            _ => "Set Template".to_string(),
        }
    }

    fn refresh_title(&self, sd: &SdClient, ctx: &AppCtx, context: &str) {
        let title = self
            .compute_title_from_ctx(ctx)
            .unwrap_or_else(|| self.compute_fallback_title());
        sd.set_title(context, Some(title), None, None);
    }
}

impl Action for SetTemplateAction {
    fn init(&mut self, _sd: &SdClient, ctx: &AppCtx, context: String) {
        self.context = Some(context);
        if let Some(context) = &self.context {
            self.refresh_title(_sd, ctx, context);
        }
    }

    fn on_event(
        &mut self,
        sd: &SdClient,
        ctx: &AppCtx,
        ev: &StreamDeckEvent,
        out: &mut Vec<Command>
    ) {
        match ev {
            StreamDeckEvent::WillDisappear { .. } => {
                self.context = None; // clear context on disappear
            }
            | StreamDeckEvent::WillAppear { context, settings, .. }
            | StreamDeckEvent::DidReceiveSettings { context, settings, .. } => {
                self.context = Some(context.clone());

                // Accept both old & new PI field names
                self.selected_build = settings
                    .get("build_index")
                    .and_then(|v| v.as_u64())
                    .or_else(|| settings.get("buildTemplate").and_then(|v| v.as_u64()))
                    .and_then(|n| u8::try_from(n).ok())
                    .filter(|&n| (1..=9).contains(&n));

                self.selected_equipment = settings
                    .get("equipment_index")
                    .and_then(|v| v.as_u64())
                    .or_else(|| settings.get("equipmentTemplate").and_then(|v| v.as_u64()))
                    .and_then(|n| u8::try_from(n).ok())
                    .filter(|&n| (1..=9).contains(&n));

                self.refresh_title(sd, ctx, context);
            }

            StreamDeckEvent::KeyDown { .. } => {
                let mut to_run: Vec<KeyControl> = Vec::new();
                if let Some(b) = self.selected_build {
                    to_run.extend(build_slot_to_control(b));
                }
                if let Some(e) = self.selected_equipment {
                    to_run.extend(equipment_slot_to_control(e));
                }
                if to_run.is_empty() {
                    if let StreamDeckEvent::KeyDown { context, .. } = ev {
                        sd.show_alert(context);
                    }
                    return;
                }
                for kc in to_run {
                    out.push(Command::QueueAction(kc));
                }
            }

            _ => {}
        }
    }

    // OPTIONAL: if you add a broadcast hook in ActionManager, call this when
    // Gw2TemplateNames or MumbleActiveCharacter changes so titles live-update.
    fn on_notify(
        &mut self,
        sd: &SdClient,
        ctx: &AppCtx,
        note: action::Note,
        _out: &mut Vec<Command>
    ) {
        match note {
            action::Note::TemplatesUpdated | action::Note::ActiveCharacterChanged { .. } => {
                if let Some(context) = &self.context {
                    self.refresh_title(sd, ctx, context);
                }
            }
        }
    }
}

pub struct SetTemplateFactory;
impl ActionFactory for SetTemplateFactory {
    fn kind(&self) -> &'static str {
        concat!(PLUGIN_UUID, ".set-template")
    }
    fn create(&self) -> Box<dyn super::Action> {
        Box::new(SetTemplateAction::default())
    }
}
pub static SET_TEMPLATE: SetTemplateFactory = SetTemplateFactory;

// helpers (unchanged)
fn build_slot_to_control(n: u8) -> Option<KeyControl> {
    use KeyControl::*;
    Some(match n {
        1 => TemplatesBuildTemplate1,
        2 => TemplatesBuildTemplate2,
        3 => TemplatesBuildTemplate3,
        4 => TemplatesBuildTemplate4,
        5 => TemplatesBuildTemplate5,
        6 => TemplatesBuildTemplate6,
        7 => TemplatesBuildTemplate7,
        8 => TemplatesBuildTemplate8,
        9 => TemplatesBuildTemplate9,
        _ => {
            return None;
        }
    })
}
fn equipment_slot_to_control(n: u8) -> Option<KeyControl> {
    use KeyControl::*;
    Some(match n {
        1 => TemplatesEquipmentTemplate1,
        2 => TemplatesEquipmentTemplate2,
        3 => TemplatesEquipmentTemplate3,
        4 => TemplatesEquipmentTemplate4,
        5 => TemplatesEquipmentTemplate5,
        6 => TemplatesEquipmentTemplate6,
        7 => TemplatesEquipmentTemplate7,
        8 => TemplatesEquipmentTemplate8,
        9 => TemplatesEquipmentTemplate9,
        _ => {
            return None;
        }
    })
}
