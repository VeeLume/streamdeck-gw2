use crossbeam_channel::Sender;
use serde_json::{ Map, Value };

use crate::infra::sd_protocol::{
    Outgoing,
    SdState,
    SetImagePayload,
    SetTitlePayload,
    TriggerPayload,
};

/// Thin, typed facade over Stream Deck "Outgoing" messages.
/// No IO here — just sends to a channel the writer thread consumes.
#[derive(Clone)]
pub struct SdClient {
    tx: Sender<Outgoing>,
    plugin_uuid: String,
}

impl SdClient {
    pub fn new(tx: Sender<Outgoing>, plugin_uuid: String) -> Self {
        Self { tx, plugin_uuid }
    }

    pub fn get_global_settings(&self) {
        let _ = self.tx.send(Outgoing::GetGlobalSettings { context: self.plugin_uuid.clone() });
    }

    pub fn get_settings(&self, context: &str) {
        let _ = self.tx.send(Outgoing::GetSettings { context: context.to_string() });
    }

    pub fn log_message(&self, message: String) {
        let _ = self.tx.send(Outgoing::LogMessage { message });
    }

    pub fn open_url(&self, url: String) {
        let _ = self.tx.send(Outgoing::OpenUrl { url });
    }

    pub fn send_to_property_inspector(&self, context: &str, payload: Value) {
        let _ = self.tx.send(Outgoing::SendToPropertyInspector {
            context: context.to_string(),
            payload,
        });
    }

    pub fn set_feedback(&self, context: &str, payload: Value) {
        let _ = self.tx.send(Outgoing::SetFeedback { context: context.to_string(), payload });
    }

    pub fn set_feedback_layout(&self, context: &str, layout: &str) {
        let _ = self.tx.send(Outgoing::SetFeedbackLayout {
            context: context.to_string(),
            layout: layout.to_string(),
        });
    }

    pub fn set_global_settings(&self, settings: Map<String, Value>) {
        let _ = self.tx.send(Outgoing::SetGlobalSettings {
            context: self.plugin_uuid.clone(),
            payload: settings,
        });
    }

    pub fn set_image(
        &self,
        context: &str,
        image_base64: Option<String>,
        state: Option<SdState>,
        target: Option<String>
    ) {
        let _ = self.tx.send(Outgoing::SetImage {
            context: context.to_string(),
            payload: SetImagePayload { image: image_base64, state, target },
        });
    }

    pub fn set_settings(&self, context: &str, settings: Map<String, Value>) {
        let _ = self.tx.send(Outgoing::SetSettings {
            context: context.to_string(),
            payload: settings,
        });
    }

    pub fn set_state(&self, context: &str, state: SdState) {
        let _ = self.tx.send(Outgoing::SetState {
            context: context.to_string(),
            state,
        });
    }

    pub fn set_title(
        &self,
        context: &str,
        title: Option<String>,
        state: Option<SdState>,
        target: Option<String>
    ) {
        let _ = self.tx.send(Outgoing::SetTitle {
            context: context.to_string(),
            payload: SetTitlePayload { title, state, target },
        });
    }

    pub fn set_trigger_description(
        &self,
        context: &str,
        long_touch: Option<String>,
        push: Option<String>,
        rotate: Option<String>,
        touch: Option<String>
    ) {
        let _ = self.tx.send(Outgoing::SetTriggerDescription {
            context: context.to_string(),
            payload: TriggerPayload { long_touch, push, rotate, touch },
        });
    }

    pub fn show_alert(&self, context: &str) {
        let _ = self.tx.send(Outgoing::ShowAlert { context: context.to_string() });
    }

    pub fn show_ok(&self, context: &str) {
        let _ = self.tx.send(Outgoing::ShowOk { context: context.to_string() });
    }
}
