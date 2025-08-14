#![cfg(windows)]

use std::panic::{ catch_unwind, AssertUnwindSafe };
use std::slice;
use std::thread::{ self, JoinHandle };
use std::time::Duration;
use std::sync::Arc;

use bitflags::bitflags;
use bytemuck::{ Pod, Zeroable };
use crossbeam_channel::{ unbounded, select, tick, Receiver, Sender };
use serde::Deserialize;
use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;

use crate::{ log };
use crate::core::events::AppEvent;
use crate::logger::ActionLog;

// ---- public handle ----------------------------------------------------------

enum Cmd {
    Fast(bool),
    Shutdown,
}

pub struct Mumble {
    tx: Sender<Cmd>,
    join: Option<JoinHandle<()>>,
}

impl Mumble {
    pub fn spawn(logger: Arc<dyn ActionLog>, app_tx: Sender<AppEvent>) -> Self {
        let (tx, rx) = unbounded::<Cmd>();
        let join = thread::spawn(move || {
            if let Err(p) = catch_unwind(AssertUnwindSafe(|| { run(logger.clone(), app_tx, rx) })) {
                log!(logger, "❌ reader thread panicked: {:?}", p);
            }
        });
        Self { tx, join: Some(join) }
    }

    pub fn set_fast(&self, fast: bool) {
        let _ = self.tx.send(Cmd::Fast(fast));
    }
    pub fn shutdown(mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for Mumble {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// ---- reader loop ------------------------------------------------------------

fn run(logger: Arc<dyn ActionLog>, app_tx: Sender<AppEvent>, rx: Receiver<Cmd>) {
    const FAST: Duration = Duration::from_millis(16);
    const SLOW: Duration = Duration::from_secs(10);

    let mut fast = false;
    let mut ticker = tick(SLOW);

    log!(logger, "🎧 Mumble reader started (slow)");

    // last-knowns to de-duplicate events
    let mut last_in_combat: Option<bool> = None;
    let mut last_name: Option<String> = None;

    // open/reopen loop
    'outer: loop {
        // try to map shared memory; if not available, wait and retry / handle cmds
        let link = match MumbleLink::new() {
            Ok(l) => l,
            Err(e) => {
                log!(logger, "⚠️ MumbleLink open failed: {} (retrying)", e);
                // wait a bit or until a command arrives
                select! {
                    recv(rx) -> msg => match msg {
                        Ok(Cmd::Fast(f)) => {
                            fast = f;
                            ticker = tick(if fast { FAST } else { SLOW });
                            log!(logger, "🎚️ mumble mode -> {}", if fast { "FAST" } else { "SLOW" });
                        }
                        Ok(Cmd::Shutdown) | Err(_) => break 'outer,
                    },
                    default(SLOW) => {}
                }
                continue;
            }
        };

        log!(logger, "✅ MumbleLink mapped");

        // inner read loop
        loop {
            select! {
                recv(rx) -> msg => match msg {
                    Ok(Cmd::Fast(f)) => {
                        fast = f;
                        ticker = tick(if fast { FAST } else { SLOW });
                        log!(logger, "🎚️ mumble mode -> {}", if fast { "FAST" } else { "SLOW" });
                    }
                    Ok(Cmd::Shutdown) | Err(_) => break 'outer,
                },
                recv(ticker) -> _ => {
                    // parse identity every tick; if you want to micro-opt, sample every N ticks
                    if let Some((ui, ident)) = link.read(true) {
                        // combat flag from ui_state
                        let in_combat = ui.is_in_combat();
                        if last_in_combat != Some(in_combat) {
                            last_in_combat = Some(in_combat);
                            let _ = app_tx.send(AppEvent::MumbleCombat(in_combat));
                        }

                        // active char name
                        if let Some(id) = ident {
                            if let Some(name) = Some(id.name).filter(|s| !s.is_empty()) {
                                if last_name.as_deref() != Some(&name) {
                                    last_name = Some(name.clone());
                                    let _ = app_tx.send(AppEvent::MumbleActiveCharacter(name));
                                }
                            }
                        }
                    } else {
                        // read failed (likely unmapped); reopen
                        log!(logger, "⚠️ MumbleLink read failed; remapping");
                        break;
                    }
                }
            }
        }
    }

    log!(logger, "🛑 Mumble reader stopped");
}

// ---- types + mapping --------------------------------------------------------

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
                CloseHandle(handle);
            }
            return Err("MapViewOfFile failed".to_string());
        }

        Ok(MumbleLink { map_handle: handle, view_ptr: ptr })
    }

    /// Returns (UiState, Identity) if read succeeded.
    pub fn read(&self, parse_identity: bool) -> Option<(UiState, Option<Identity>)> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            let lm: LinkedMem = *bytemuck::from_bytes::<LinkedMem>(bytes);

            // parse context for ui_state
            let ctx = bytemuck
                ::try_from_bytes::<MumbleContext>(
                    &lm.context[..std::mem::size_of::<MumbleContext>()]
                )
                .ok()?
                .to_owned();
            let ui = UiState::from_bits_truncate(ctx.ui_state);

            // optional identity JSON
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
            UnmapViewOfFile(self.view_ptr);
            CloseHandle(self.map_handle);
        }
    }
}
