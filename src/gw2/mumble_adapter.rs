#![cfg(windows)]

use std::{ slice, thread, time::Duration };
use std::sync::Arc;

use bitflags::bitflags;
use bytemuck::{ Pod, Zeroable };
use crossbeam_channel::{ bounded, select, tick, Receiver as CbReceiver };
use serde::Deserialize;
use serde_json::json;
use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;

use streamdeck_lib::{
    adapters::{ Adapter, AdapterHandle, AdapterNotify, StartPolicy },
    bus::Bus,
    prelude::Context,
    debug,
    warn,
    info,
};

use crate::gw2::shared::{ ActiveChar, InCombat };

/// Publishes:
/// - "mumble.combat"           -> bool
/// - "mumble.active-character" -> String  (only emitted in SLOW mode)
///
/// Listens:
/// - "mumble.fast"             -> ~16ms polling, combat only
/// - "mumble.slow"             -> ~10s polling, parses identity too
pub struct MumbleAdapter;

impl MumbleAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for MumbleAdapter {
    fn name(&self) -> &'static str {
        "gw2.mumble_adapter"
    }

    fn policy(&self) -> StartPolicy {
        StartPolicy::OnAppLaunch
    }

    fn topics(&self) -> &'static [&'static str] {
        &["mumble.fast", "mumble.slow"]
    }

    fn start(
        &self,
        cx: &Context,
        bus: Arc<dyn Bus>,
        inbox: CbReceiver<AdapterNotify>
    ) -> Result<AdapterHandle, String> {
        let (stop_tx, stop_rx) = bounded::<()>(1);
        let logger = cx.log().clone();
        let in_combat_ext = cx.try_ext::<InCombat>().expect("InCombat extension not found").clone();
        let active_char_ext = cx
            .try_ext::<ActiveChar>()
            .expect("ActiveChar extension not found")
            .clone();

        let join = thread::spawn(move || {
            const FAST: Duration = Duration::from_millis(16);
            const SLOW: Duration = Duration::from_secs(10);

            // default: slow
            let mut fast = false;
            let mut ticker = tick(SLOW);

            // mapping
            let mut link: Option<MumbleLink> = None;

            // de-dupe
            let mut last_in_combat: Option<bool> = None;
            let mut last_name: Option<String> = None;

            info!(logger, "🎧 Mumble adapter started (slow)");

            loop {
                select! {
                    recv(inbox) -> msg => {
                        match msg {
                            Ok(AdapterNotify { topic, .. }) => match topic.as_str() {
                                "mumble.fast" => {
                                    fast = true;
                                    ticker = tick(FAST);
                                    debug!(logger, "🎚️ mumble mode -> FAST (combat only)");
                                    // keep last_name as-is; we aren't emitting names in fast
                                }
                                "mumble.slow" => {
                                    fast = false;
                                    ticker = tick(SLOW);
                                    debug!(logger, "🎚️ mumble mode -> SLOW (combat + identity)");
                                    // force a fresh name emit next slow tick
                                    last_name = None;
                                }
                                _ => {}
                            },
                            Err(_) => break, // inbox closed
                        }
                    }

                    recv(stop_rx) -> _ => {
                        debug!(logger, "Stopping Mumble adapter...");
                        break;
                    }

                    recv(ticker) -> _ => {
                        // Ensure mapping
                        if link.is_none() {
                            match MumbleLink::new() {
                                Ok(l) => {
                                    info!(logger, "✅ MumbleLink mapped");
                                    link = Some(l);
                                    // on (re)map, refresh dedupe so we re-emit state
                                    last_in_combat = None;
                                    if !fast { last_name = None; }
                                }
                                Err(e) => {
                                    if !fast {
                                        warn!(logger, "⚠️ MumbleLink open failed: {} (retrying)", e);
                                    }
                                    continue;
                                }
                            }
                        }

                        // Read: identity only in slow mode
                        if let Some(l) = link.as_ref() {
                            let parse_identity = !fast;
                            if let Some((ui, ident)) = l.read(parse_identity) {
                                // combat always processed
                                let in_combat = ui.is_in_combat();
                                if last_in_combat != Some(in_combat) {
                                    last_in_combat = Some(in_combat);
                                    in_combat_ext.set(in_combat);
                                    bus.action_notify_all("mumble.combat".to_string(), Some(json!(in_combat)));
                                }

                                // identity only when parsed (slow mode)
                                if let Some(id) = ident {
                                    let name = id.name; // empty string is valid
                                    if last_name.as_deref() != Some(name.as_str()) {
                                        last_name = Some(name.clone());
                                        active_char_ext.set(Some(name.clone()));
                                        bus.action_notify_all("mumble.active-character".to_string(), Some(json!(name)));
                                    }
                                }
                            } else {
                                // read failed -> drop mapping and retry next tick
                                warn!(logger, "⚠️ MumbleLink read failed; remapping");
                                link = None;
                            }
                        }
                    }
                }
            }

            if link.take().is_some() {
                debug!(logger, "Unmapping MumbleLink on shutdown");
            }
            info!(logger, "🛑 Mumble adapter stopped");
        });

        Ok(AdapterHandle {
            join: Some(join),
            shutdown: Box::new(move || {
                let _ = stop_tx.send(());
            }),
        })
    }
}

// ---- Mumble types + mapping -------------------------------------------------

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UiState: u32 {
        const MAP_OPEN               = 1 << 0;
        const COMPASS_TOP_RIGHT      = 1 << 1;
        const COMPASS_ROTATION       = 1 << 2;
        const GAME_HAS_FOCUS         = 1 << 3;
        const COMPETITIVE_MODE       = 1 << 4;
        const TEXTBOX_HAS_FOCUS      = 1 << 5;
        const IN_COMBAT              = 1 << 6;
    }
}
impl UiState {
    pub fn is_in_combat(self) -> bool {
        self.contains(UiState::IN_COMBAT)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Identity {
    pub name: String,
    pub profession: Option<u8>,
    pub spec: Option<u16>,
    pub race: Option<u8>,
    pub map_id: Option<u32>,
    pub world_id: Option<u32>,
    pub team_color_id: Option<u8>,
    pub commander: Option<bool>,
    pub fov: Option<f32>,
    pub uisz: Option<u8>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct LinkedMem {
    pub ui_version: u32,
    pub ui_tick: u32,
    pub f_avatar_position: [f32; 3],
    pub f_avatar_front: [f32; 3],
    pub f_avatar_top: [f32; 3],
    pub name: [u16; 256],
    pub f_camera_position: [f32; 3],
    pub f_camera_front: [f32; 3],
    pub f_camera_top: [f32; 3],
    pub identity: [u16; 256],
    pub context_len: u32,
    pub context: [u8; 256],
    pub description: [u16; 2048],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct MumbleContext {
    pub server_address: [u8; 28],
    pub map_id: u32,
    pub map_type: u32,
    pub shard_id: u32,
    pub instance: u32,
    pub build_id: u32,
    pub ui_state: u32,
    pub compass_width: u16,
    pub compass_height: u16,
    pub compass_rotation: f32,
    pub player_x: f32,
    pub player_y: f32,
    pub map_center_x: f32,
    pub map_center_y: f32,
    pub map_scale: f32,
    pub process_id: u32,
    pub mount_index: u8,
    pub _padding: [u8; 3],
}

const SHARED_MEM_SIZE: usize = std::mem::size_of::<LinkedMem>();

pub struct MumbleLink {
    map_handle: HANDLE,
    view_ptr: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl MumbleLink {
    pub fn new() -> Result<Self, String> {
        let handle = (
            unsafe {
                OpenFileMappingW(FILE_MAP_READ.0, false, windows_core::w!("MumbleLink"))
            }
        ).map_err(|_| "OpenFileMappingW(MumbleLink) failed".to_string())?;

        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, SHARED_MEM_SIZE) };
        if ptr.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("MapViewOfFile failed".to_string());
        }

        Ok(MumbleLink { map_handle: handle, view_ptr: ptr })
    }

    /// Returns (UiState, Identity) if read succeeded.
    /// `parse_identity` should be `false` in FAST mode.
    pub fn read(&self, parse_identity: bool) -> Option<(UiState, Option<Identity>)> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            let lm: LinkedMem = *bytemuck::from_bytes::<LinkedMem>(bytes);

            let ctx = bytemuck
                ::try_from_bytes::<MumbleContext>(
                    &lm.context[..std::mem::size_of::<MumbleContext>()]
                )
                .ok()?
                .to_owned();
            let ui = UiState::from_bits_truncate(ctx.ui_state);

            let ident = if parse_identity {
                let s = String::from_utf16_lossy(&lm.identity).trim_end_matches('\0').to_string();
                let upto = s
                    .find('}')
                    .map(|i| i + 1)
                    .unwrap_or(s.len());
                let s = &s[..upto];
                if s.trim_start().starts_with('{') {
                    serde_json::from_str::<Identity>(s).ok()
                } else {
                    None
                }
            } else {
                None
            };

            Some((ui, ident))
        }
    }
}

impl Drop for MumbleLink {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view_ptr);
            let _ = CloseHandle(self.map_handle);
        }
    }
}
