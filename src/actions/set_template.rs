use constcat::concat;
use serde_json::{Map, Value};
use std::fmt::Write as _;

use streamdeck_lib::prelude::*;

use crate::{
    PLUGIN_ID,
    gw2::{
        enums::{KeyControl, TemplateNames}, // { build: [Option<String>; 9], equipment: [Option<String>; 9] }
        shared::{ActiveChar, TemplateStore},
    },
    topics::{
        GW2_API_CHARACTER_CHANGED, GW2_API_TEMPLATE_CHANGED, GW2_EXEC_QUEUE, Gw2ExecQueue,
        MUMBLE_ACTIVE_CHARACTER,
    },
};

// ── Action ────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SetTemplateAction {
    selected_build: Option<u8>,     // 1..=9
    selected_equipment: Option<u8>, // 1..=9
    last_title: Option<String>,
}

impl ActionStatic for SetTemplateAction {
    const ID: &'static str = concat!(PLUGIN_ID, ".set-template");
}

impl Action for SetTemplateAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn topics(&self) -> &'static [&'static str] {
        &[
            MUMBLE_ACTIVE_CHARACTER.name,
            MUMBLE_ACTIVE_CHARACTER.name,
            GW2_API_CHARACTER_CHANGED.name,
        ]
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        info!(
            cx.log(),
            "SetTemplateAction initialized for context: {}", ctx_id
        );
        // Nothing persistent to subscribe here; we rebuild title on each appear/settings event.
        // If you add an action-level notify hook, just call `self.refresh_title(cx, ctx_id)`.
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &DidReceiveSettings) {
        self.apply_settings_json(ev.settings, cx, ev.context);
    }

    fn will_appear(&mut self, cx: &Context, ev: &WillAppear) {
        debug!(cx.log(), "SetTemplateAction will_appear: {:?}", ev.context);
        self.apply_settings_json(ev.settings, cx, ev.context);
        debug!(cx.log(), "Refreshing title for context: {}", ev.context);
        self.refresh_title(cx, ev.context);
    }

    fn on_notify(&mut self, cx: &Context, ctx_id: &str, event: &ErasedTopic) {
        // Recompute title whenever inputs it depends on might change.
        if let Some(m) = event.downcast(MUMBLE_ACTIVE_CHARACTER) {
            debug!(cx.log(), "Received Mumble active character event: {:?}", m);
            self.refresh_title(cx, ctx_id);
            return;
        }

        if let Some(m) = event.downcast(GW2_API_CHARACTER_CHANGED) {
            debug!(
                cx.log(),
                "Received GW2 API character changed event: {:?}", m
            );
            // Only refresh if the event concerns the active character
            let active = cx
                .try_ext::<ActiveChar>()
                .and_then(|a| a.get())
                .unwrap_or_default(); // "" allowed

            if m.name == active {
                debug!(
                    cx.log(),
                    "Refreshing title due to GW2 API change for active character: {}", active
                );
                self.refresh_title(cx, ctx_id);
            }
            return;
        }

        if let Some(m) = event.downcast(GW2_API_TEMPLATE_CHANGED) {
            debug!(cx.log(), "Received GW2 API template changed event: {:?}", m);
            // Only refresh if the event concerns the active character
            let active = cx
                .try_ext::<ActiveChar>()
                .and_then(|a| a.get())
                .unwrap_or_default(); // "" allowed

            if m.name == active {
                debug!(
                    cx.log(),
                    "Refreshing title due to GW2 API template change for active character: {}",
                    active
                );
                self.refresh_title(cx, ctx_id);
            }
        }
    }

    fn key_down(&mut self, cx: &Context, ev: &KeyDown) {
        // gather controls
        let mut controls: Vec<KeyControl> = Vec::new();
        if let Some(b) = self.selected_build {
            if let Some(kc) = build_slot_to_control(b) {
                controls.push(kc);
            }
        }
        if let Some(e) = self.selected_equipment {
            if let Some(kc) = equipment_slot_to_control(e) {
                controls.push(kc);
            }
        }

        if controls.is_empty() {
            cx.sd().show_alert(ev.context);
            return;
        }

        cx.bus().adapters_notify_topic_t(
            GW2_EXEC_QUEUE,
            None,
            Gw2ExecQueue {
                controls,
                allow_in_combat: false,
                inter_control_ms: None, // optional pacing between controls
            },
        );
    }

    fn key_up(&mut self, cx: &Context, ev: &KeyUp) {
        debug!(
            cx.log(),
            "SetTemplateAction key_up for ctx_id: {}", ev.context
        );
        // No-op. If you prefer “press on key-up”, move the executor here.
    }
}

// ── Impl details ─────────────────────────────────────────────────────────────

impl SetTemplateAction {
    fn refresh_title(&mut self, cx: &Context, cx_id: &str) {
        let title = self
            .compute_title(cx)
            .unwrap_or_else(|| self.compute_fallback_title());
        if self.last_title.as_deref() != Some(title.as_str()) {
            self.last_title = Some(title.clone());
            cx.sd().set_title(cx_id, Some(title), None, None);
        }
    }

    fn apply_settings_json(&mut self, settings: &Map<String, Value>, cx: &Context, cx_id: &str) {
        self.selected_build = settings
            .get("build_index")
            .and_then(|v| v.as_u64())
            .and_then(|n| u8::try_from(n).ok())
            .filter(|&n| (1..=9).contains(&n));

        self.selected_equipment = settings
            .get("equipment_index")
            .and_then(|v| v.as_u64())
            .and_then(|n| u8::try_from(n).ok())
            .filter(|&n| (1..=9).contains(&n));

        debug!(
            cx.log(),
            "settings -> build={:?} equipment={:?}", self.selected_build, self.selected_equipment
        );

        // Try to build a pretty title using TemplateStore + active character ("" allowed)
        let title = self
            .compute_title(cx)
            .unwrap_or_else(|| self.compute_fallback_title());

        cx.sd().set_title(cx_id, Some(title), None, None);
    }

    fn compute_title(&self, cx: &Context) -> Option<String> {
        let Some(active_ext) = cx.try_ext::<ActiveChar>() else {
            return None;
        };

        let active_name_opt = active_ext.get();
        let active_name = match active_name_opt.as_deref() {
            Some(name) if !name.is_empty() => name,
            _ => {
                return None;
            } // No active character or empty name
        };

        let store = cx.try_ext::<TemplateStore>()?;
        let names: TemplateNames = match store.get(active_name) {
            Some(n) => n.clone(),
            None => {
                return None;
            }
        };

        let mut parts: Vec<String> = Vec::new();

        if let Some(b) = self.selected_build {
            let idx = (b as usize).saturating_sub(1);
            let label = names
                .build
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|s| wrap_title_or_fallback(s, &format!("Build {b}"), 10))
                .unwrap_or_else(|| format!("Build {b}"));
            parts.push(label);
        }

        if let Some(e) = self.selected_equipment {
            let idx = (e as usize).saturating_sub(1);
            let label = names
                .equipment
                .get(idx)
                .and_then(|o| o.as_ref())
                .map(|s| wrap_title_or_fallback(s, &format!("Equip {e}"), 10))
                .unwrap_or_else(|| format!("Equip {e}"));
            parts.push(label);
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
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn wrap_title_or_fallback(name: &str, fallback: &str, max_len: usize) -> String {
    if name.is_empty() {
        return fallback.to_string();
    }
    wrap_title(name, max_len)
}

fn wrap_title(title: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut cur_len = 0usize;

    for (i, word) in title.split_whitespace().enumerate() {
        let need = if i == 0 { word.len() } else { 1 + word.len() };
        if cur_len > 0 && cur_len + need > max_len {
            out.push('\n');
            out.push_str(word);
            cur_len = word.len();
        } else {
            if i != 0 && cur_len > 0 {
                out.push(' ');
                cur_len += 1;
            }
            out.push_str(word);
            cur_len += word.len();
        }
    }

    if out.is_empty() {
        // title had no whitespace or was empty — still clamp hard to avoid long single-line strings
        let mut s = String::new();
        let _ = write!(&mut s, "{title}");
        return s;
    }

    out
}

// Same mapping you had before
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
