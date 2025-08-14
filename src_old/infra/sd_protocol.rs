use num_enum::{ IntoPrimitive, TryFromPrimitive };
use serde::{ Deserialize, Serialize };
use serde_json::{ Map, Value };

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    IntoPrimitive,
    TryFromPrimitive,
    Serialize,
    Deserialize
)]
pub enum SdState {
    Primary = 0,
    Secondary = 1,
}

impl SdState {
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        v.as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .and_then(|b| SdState::try_from(b).ok())
    }
    pub fn as_u8(self) -> u8 {
        self.into()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Size {
    pub columns: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coordinates {
    pub column: i64,
    pub row: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub r#type: i64,
    pub size: Size,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleParameters {
    pub font_family: String,
    pub font_size: i64,
    pub font_style: String,
    pub font_underline: bool,
    pub show_title: bool,
    pub title_alignment: String,
    pub title_color: String,
}

#[derive(Debug, Clone)]
pub enum StreamDeckEvent {
    ApplicationDidLaunch {
        application: String,
    },
    ApplicationDidTerminate {
        application: String,
    },
    DeviceDidChange {
        device: String,
        device_info: DeviceInfo,
    },
    DeviceDidConnect {
        device: String,
        device_info: DeviceInfo,
    },
    DeviceDidDisconnect {
        device: String,
    },
    DialDown {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        coordinates: Coordinates,
    },
    DialRotate {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        coordinates: Coordinates,
        pressed: bool,
        ticks: i64,
    },
    DialUp {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        coordinates: Coordinates,
    },
    DidReceiveDeepLink {
        url: String,
    },
    DidReceiveGlobalSettings {
        settings: Map<String, Value>,
    },
    DidReceivePropertyInspectorMessage {
        action: String,
        context: String,
        message: Value,
    },
    DidReceiveSettings {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        is_in_multi_action: bool,
        state: Option<SdState>,
        coordinates: Option<Coordinates>,
    },
    KeyDown {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        is_in_multi_action: bool,
        state: Option<SdState>,
        coordinates: Option<Coordinates>,
    },
    KeyUp {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        is_in_multi_action: bool,
        state: Option<SdState>,
        coordinates: Option<Coordinates>,
    },
    PropertyInspectorDidAppear {
        action: String,
        context: String,
        device: String,
    },
    PropertyInspectorDidDisappear {
        action: String,
        context: String,
        device: String,
    },
    SystemDidWakeUp,
    TitleParametersDidChange {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        coordinates: Coordinates,
        state: Option<SdState>,
        title: String,
        title_parameters: TitleParameters,
    },
    TouchTap {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        coordinates: Coordinates,
        hold: bool,
        tap_pos: (i64, i64),
    },
    WillAppear {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        is_in_multi_action: bool,
        state: Option<SdState>,
        coordinates: Option<Coordinates>,
    },
    WillDisappear {
        action: String,
        context: String,
        device: String,
        settings: Map<String, Value>,
        controller: String,
        is_in_multi_action: bool,
        state: Option<SdState>,
        coordinates: Option<Coordinates>,
    },
}

impl std::fmt::Display for StreamDeckEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use StreamDeckEvent::*;
        match self {
            ApplicationDidLaunch { .. } => write!(f, "ApplicationDidLaunch"),
            ApplicationDidTerminate { .. } => write!(f, "ApplicationDidTerminate"),
            DeviceDidChange { .. } => write!(f, "DeviceDidChange"),
            DeviceDidConnect { .. } => write!(f, "DeviceDidConnect"),
            DeviceDidDisconnect { .. } => write!(f, "DeviceDidDisconnect"),
            DialDown { action, context, .. } =>
                write!(f, "DialDown(action={}, context={})", action, context),
            DialRotate { action, context, .. } =>
                write!(f, "DialRotate(action={}, context={})", action, context),
            DialUp { action, context, .. } =>
                write!(f, "DialUp(action={}, context={})", action, context),
            DidReceiveDeepLink { .. } => write!(f, "DidReceiveDeepLink"),
            DidReceiveGlobalSettings { .. } => write!(f, "DidReceiveGlobalSettings"),
            DidReceivePropertyInspectorMessage { action, context, .. } =>
                write!(
                    f,
                    "DidReceivePropertyInspectorMessage(action={}, context={})",
                    action,
                    context
                ),
            DidReceiveSettings { action, context, .. } =>
                write!(f, "DidReceiveSettings(action={}, context={})", action, context),
            KeyDown { action, context, .. } =>
                write!(f, "KeyDown(action={}, context={})", action, context),
            KeyUp { action, context, .. } =>
                write!(f, "KeyUp(action={}, context={})", action, context),
            PropertyInspectorDidAppear { action, context, .. } =>
                write!(f, "PropertyInspectorDidAppear(action={}, context={})", action, context),
            PropertyInspectorDidDisappear { action, context, .. } =>
                write!(f, "PropertyInspectorDidDisappear(action={}, context={})", action, context),
            SystemDidWakeUp => write!(f, "SystemDidWakeUp"),
            TitleParametersDidChange { action, context, .. } =>
                write!(f, "TitleParametersDidChange(action={}, context={})", action, context),
            TouchTap { action, context, .. } =>
                write!(f, "TouchTap(action={}, context={})", action, context),
            WillAppear { action, context, .. } =>
                write!(f, "WillAppear(action={}, context={})", action, context),
            WillDisappear { action, context, .. } =>
                write!(f, "WillDisappear(action={}, context={})", action, context),
        }
    }
}

pub fn parse_incoming(m: &Map<String, Value>) -> Result<StreamDeckEvent, String> {
    use StreamDeckEvent::*;
    macro_rules! req {
        ($opt:expr, $name:literal) => {
            $opt.ok_or_else(|| format!("missing {}", $name))?
        };
    }

    let event = req!(m.get("event").and_then(Value::as_str), "event");
    let action = m
        .get("action")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let context = m.get("context").and_then(Value::as_str);

    let device = m.get("device").and_then(Value::as_str);
    let payload = m.get("payload");

    let settings = payload
        .and_then(|p| p.get("settings"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let controller = payload
        .and_then(|p| p.get("controller"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let coordinates = payload
        .and_then(|p| p.get("coordinates"))
        .and_then(Value::as_object)
        .and_then(|c| {
            Some(Coordinates {
                column: c.get("column")?.as_i64()?,
                row: c.get("row")?.as_i64()?,
            })
        });

    let is_in_multi_action = payload
        .and_then(|p| p.get("isInMultiAction"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let state = payload.and_then(|p| p.get("state")).and_then(SdState::from_json);

    let title = payload
        .and_then(|p| p.get("title"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let title_parameters = payload
        .and_then(|p| p.get("titleParameters"))
        .and_then(Value::as_object)
        .and_then(|tp| {
            Some(TitleParameters {
                font_family: tp.get("fontFamily")?.as_str()?.to_string(),
                font_size: tp.get("fontSize")?.as_i64()?,
                font_style: tp.get("fontStyle")?.as_str()?.to_string(),
                font_underline: tp.get("fontUnderline")?.as_bool()?,
                show_title: tp.get("showTitle")?.as_bool()?,
                title_alignment: tp.get("titleAlignment")?.as_str()?.to_string(),
                title_color: tp.get("titleColor")?.as_str()?.to_string(),
            })
        });

    match event {
        "willAppear" =>
            Ok(WillAppear {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                is_in_multi_action,
                state,
                coordinates,
            }),
        "didReceiveSettings" =>
            Ok(DidReceiveSettings {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                is_in_multi_action,
                state,
                coordinates,
            }),
        "keyDown" =>
            Ok(KeyDown {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: "Keypad".to_string(),
                is_in_multi_action,
                state,
                coordinates,
            }),
        "keyUp" =>
            Ok(KeyUp {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: "Keypad".to_string(),
                is_in_multi_action,
                state,
                coordinates,
            }),
        "willDisappear" =>
            Ok(WillDisappear {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                is_in_multi_action,
                state,
                coordinates,
            }),
        "propertyInspectorDidAppear" =>
            Ok(PropertyInspectorDidAppear {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
            }),
        "propertyInspectorDidDisappear" =>
            Ok(PropertyInspectorDidDisappear {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
            }),
        "titleParametersDidChange" =>
            Ok(TitleParametersDidChange {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                coordinates: req!(coordinates, "coordinates"),
                state,
                title: req!(title, "title").to_string(),
                title_parameters: req!(title_parameters, "titleParameters"),
            }),
        "touchTap" =>
            Ok(TouchTap {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                coordinates: req!(coordinates, "coordinates"),
                hold: req!(
                    payload.and_then(|p| p.get("hold")).and_then(Value::as_bool),
                    "payload.hold"
                ),
                tap_pos: req!(
                    payload
                        .and_then(|p| p.get("tapPos"))
                        .and_then(Value::as_array)
                        .and_then(|arr| {
                            if arr.len() == 2 {
                                Some((arr[0].as_i64()?, arr[1].as_i64()?))
                            } else {
                                None
                            }
                        }),
                    "payload.tapPos"
                ),
            }),
        "dialDown" =>
            Ok(DialDown {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                coordinates: req!(coordinates, "coordinates"),
            }),
        "dialRotate" =>
            Ok(DialRotate {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                coordinates: req!(coordinates, "coordinates"),
                pressed: req!(
                    payload.and_then(|p| p.get("pressed")).and_then(Value::as_bool),
                    "payload.pressed"
                ),
                ticks: req!(
                    payload.and_then(|p| p.get("ticks")).and_then(Value::as_i64),
                    "payload.ticks"
                ),
            }),
        "dialUp" =>
            Ok(DialUp {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                device: req!(device, "device").to_string(),
                settings,
                controller: req!(controller, "controller").to_string(),
                coordinates: req!(coordinates, "coordinates"),
            }),
        "applicationDidLaunch" =>
            Ok(ApplicationDidLaunch {
                application: req!(
                    payload.and_then(|p| p.get("application")).and_then(Value::as_str),
                    "payload.application"
                ).to_string(),
            }),
        "applicationDidTerminate" =>
            Ok(ApplicationDidTerminate {
                application: req!(
                    payload.and_then(|p| p.get("application")).and_then(Value::as_str),
                    "payload.application"
                ).to_string(),
            }),
        "deviceDidChange" =>
            Ok(DeviceDidChange {
                device: req!(device, "device").to_string(),
                device_info: serde_json
                    ::from_value(req!(m.get("deviceInfo").cloned(), "deviceInfo"))
                    .map_err(|e| format!("bad deviceInfo: {e}"))?,
            }),
        "deviceDidConnect" =>
            Ok(DeviceDidConnect {
                device: req!(device, "device").to_string(),
                device_info: serde_json
                    ::from_value(req!(m.get("deviceInfo").cloned(), "deviceInfo"))
                    .map_err(|e| format!("bad deviceInfo: {e}"))?,
            }),
        "deviceDidDisconnect" =>
            Ok(DeviceDidDisconnect {
                device: req!(device, "device").to_string(),
            }),
        "didReceiveDeepLink" =>
            Ok(DidReceiveDeepLink {
                url: req!(
                    payload.and_then(|p| p.get("url")).and_then(Value::as_str),
                    "payload.url"
                ).to_string(),
            }),
        "didReceiveGlobalSettings" =>
            Ok(DidReceiveGlobalSettings {
                settings: req!(
                    payload
                        .and_then(|p| p.get("settings"))
                        .and_then(Value::as_object)
                        .cloned(),
                    "payload.settings"
                ),
            }),
        "sendToPlugin" =>
            Ok(DidReceivePropertyInspectorMessage {
                action: req!(action, "action").to_string(),
                context: req!(context, "context").to_string(),
                message: payload
                    .and_then(|p| p.get("message"))
                    .cloned()
                    .unwrap_or(Value::Null),
            }),
        "systemDidWakeUp" => Ok(SystemDidWakeUp),
        other => Err(format!("unknown StreamDeck event: {}", other)),
    }
}

// --- Outgoing (typed) ---
#[derive(Debug, Clone, serde::Serialize)]
pub enum Outgoing {
    GetGlobalSettings {
        context: String,
    },
    GetSettings {
        context: String,
    },
    LogMessage {
        message: String,
    },
    OpenUrl {
        url: String,
    },
    SendToPropertyInspector {
        context: String,
        payload: Value,
    },
    SetFeedback {
        context: String,
        payload: Value,
    },
    SetFeedbackLayout {
        context: String,
        layout: String,
    },
    SetGlobalSettings {
        context: String,
        payload: Map<String, Value>,
    },
    SetImage {
        context: String,
        payload: SetImagePayload,
    },
    SetSettings {
        context: String,
        payload: Map<String, Value>,
    },
    SetState {
        context: String,
        state: SdState,
    },
    SetTitle {
        context: String,
        payload: SetTitlePayload,
    },
    SetTriggerDescription {
        context: String,
        payload: TriggerPayload,
    },
    ShowAlert {
        context: String,
    },
    ShowOk {
        context: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SetTitlePayload {
    /// Title to display; when no title is specified, the title will reset to the title set by the user.
    pub title: Option<String>,
    /// Action state the request applies to; when no state is supplied, the title is set for both states. Note, only applies to multi-state actions.
    pub state: Option<SdState>,
    /// Specifies which aspects of the Stream Deck should be updated, hardware, software, or both.
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetImagePayload {
    /// Image to display; this can be either a path to a local file within the plugin's folder, a base64 encoded string with the mime type declared (e.g. PNG, JPEG, etc.),
    pub image: Option<String>,
    /// Action state the command applies to; when no state is supplied, the image is set for both states. Note, only applies to multi-state actions.
    pub state: Option<SdState>,
    /// Specifies which aspects of the Stream Deck should be updated, hardware, software, or both.
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriggerPayload {
    /// Touchscreen "long-touch" interaction behavior description; when undefined, the description will not be shown.
    pub long_touch: Option<String>,
    /// Dial "push" (press) interaction behavior description; when undefined, the description will not be shown.
    pub push: Option<String>,
    /// Dial rotation interaction behavior description; when undefined, the description will not be shown.
    pub rotate: Option<String>,
    /// Touchscreen "touch" interaction behavior description; when undefined, the description will not be shown.
    pub touch: Option<String>,
}

pub fn serialize_outgoing(msg: &Outgoing) -> anyhow::Result<String> {
    use Outgoing::*;
    let json = match msg {
        GetGlobalSettings { context } =>
            serde_json::json!({
            "event": "getGlobalSettings",
            "context": context,
        }),
        SetTitle { context, payload } =>
            serde_json::json!({
            "event": "setTitle",
            "context": context,
            "payload": {
                "title": payload.title,
                "state": payload.state,
                "target": payload.target,
            }
        }),
        SetImage { context, payload } =>
            serde_json::json!({
            "event": "setImage",
            "context": context,
            "payload": {
                "image": payload.image,
                "state": payload.state,
                "target": payload.target,
            }
        }),
        SetState { context, state } =>
            serde_json::json!({
            "event": "setState",
            "context": context,
            "payload": {
                "state": state.as_u8(),
            }
        }),
        SetSettings { context, payload } =>
            serde_json::json!({
            "event": "setSettings",
            "context": context,
            "payload": payload,
        }),
        SetFeedback { context, payload } =>
            serde_json::json!({
            "event": "setFeedback",
            "context": context,
            "payload": payload,
        }),
        SetFeedbackLayout { context, layout } =>
            serde_json::json!({
            "event": "setFeedbackLayout",
            "context": context,
            "payload": {
                "layout": layout,
            }
        }),
        SetTriggerDescription { context, payload } =>
            serde_json::json!({
            "event": "setTriggerDescription",
            "context": context,
            "payload": {
                "long_touch": payload.long_touch,
                "push": payload.push,
                "rotate": payload.rotate,
                "touch": payload.touch,
            }
        }),
        SendToPropertyInspector { context, payload } =>
            serde_json::json!({
            "event": "sendToPropertyInspector",
            "context": context,
            "payload": payload,
        }),
        ShowAlert { context } =>
            serde_json::json!({
            "event": "showAlert",
            "context": context,
        }),
        ShowOk { context } =>
            serde_json::json!({
            "event": "showOk",
            "context": context,
        }),
        GetSettings { context } =>
            serde_json::json!({
            "event": "getSettings",
            "context": context,
        }),
        LogMessage { message } =>
            serde_json::json!({
            "event": "logMessage",
            "payload": {
                "message": message,
            }
        }),
        OpenUrl { url } =>
            serde_json::json!({
            "event": "openUrl",
            "payload": {
                "url": url,
            }
        }),
        SetGlobalSettings { context, payload } =>
            serde_json::json!({
            "event": "setGlobalSettings",
            "context": context,
            "payload": payload
        }),
    };
    Ok(json.to_string())
}
