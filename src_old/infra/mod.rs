pub mod sd_protocol;
pub mod bindings;
pub mod gw2;
#[cfg(windows)]
pub mod mumble;
#[cfg(not(windows))]
pub use mumble_stub as mumble;
pub mod bindings_manager;

// Optional: re-exports to make call-sites nicer
pub use sd_protocol::{ Outgoing, StreamDeckEvent, serialize_outgoing, SdState };
pub use bindings::{
    KeyControl,
    KeyCode,
    MouseCode,
    SendInputMouseButton,
    DeviceType,
    Modifier,
    Key,
    KeyBind,
    send_keys,
};
