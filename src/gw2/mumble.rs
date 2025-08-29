#![cfg(windows)]

use bytemuck::{Pod, Zeroable};
use std::slice;
use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UiState: u32 {
        const MAP_OPEN          = 1 << 0;
        const COMPASS_TOP_RIGHT = 1 << 1;
        const COMPASS_ROTATION  = 1 << 2;
        const GAME_HAS_FOCUS    = 1 << 3;
        const COMPETITIVE_MODE  = 1 << 4;
        const TEXTBOX_HAS_FOCUS = 1 << 5;
        const IN_COMBAT         = 1 << 6;
    }
}
impl UiState {
    #[inline]
    pub fn is_in_combat(self) -> bool {
        self.contains(Self::IN_COMBAT)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
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
struct LinkedMem {
    ui_version: u32,
    ui_tick: u32,
    f_avatar_position: [f32; 3],
    f_avatar_front: [f32; 3],
    f_avatar_top: [f32; 3],
    name: [u16; 256],
    f_camera_position: [f32; 3],
    f_camera_front: [f32; 3],
    f_camera_top: [f32; 3],
    identity: [u16; 256],
    context_len: u32,
    context: [u8; 256],
    description: [u16; 2048],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
struct MumbleContext {
    server_address: [u8; 28],
    map_id: u32,
    map_type: u32,
    shard_id: u32,
    instance: u32,
    build_id: u32,
    ui_state: u32,
    compass_width: u16,
    compass_height: u16,
    compass_rotation: f32,
    player_x: f32,
    player_y: f32,
    map_center_x: f32,
    map_center_y: f32,
    map_scale: f32,
    process_id: u32,
    mount_index: u8,
    _padding: [u8; 3],
}

const SHARED_MEM_SIZE: usize = std::mem::size_of::<LinkedMem>();

pub struct MumbleLink {
    map_handle: HANDLE,
    view_ptr: MEMORY_MAPPED_VIEW_ADDRESS,
}
impl MumbleLink {
    pub fn new() -> Result<Self, String> {
        let handle =
            (unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, windows_core::w!("MumbleLink")) })
                .map_err(|_| "OpenFileMappingW(MumbleLink) failed".to_string())?;

        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, SHARED_MEM_SIZE) };
        if ptr.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("MapViewOfFile failed".to_string());
        }
        Ok(Self {
            map_handle: handle,
            view_ptr: ptr,
        })
    }

    /// Ultra-cheap: read only UiState + ui_tick. Use for exec-time combat decisions.
    pub fn read_ui(&self) -> Option<(UiState, u32)> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            let lm: LinkedMem = *bytemuck::from_bytes::<LinkedMem>(bytes);
            let ctx = bytemuck::try_from_bytes::<MumbleContext>(
                &lm.context[..std::mem::size_of::<MumbleContext>()],
            )
            .ok()?
            .to_owned();
            Some((UiState::from_bits_truncate(ctx.ui_state), lm.ui_tick))
        }
    }

    pub fn read_linked_mem(&self) -> Option<LinkedMem> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            Some(*bytemuck::from_bytes::<LinkedMem>(bytes))
        }
    }

    /// Full read with optional identity parse (use in slow adapter).
    pub fn read_full(&self, parse_identity: bool) -> Option<(UiState, Option<Identity>)> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            let lm: LinkedMem = *bytemuck::from_bytes::<LinkedMem>(bytes);
            let ctx = bytemuck::try_from_bytes::<MumbleContext>(
                &lm.context[..std::mem::size_of::<MumbleContext>()],
            )
            .ok()?
            .to_owned();
            let ui = UiState::from_bits_truncate(ctx.ui_state);

            let ident = if parse_identity {
                let s = String::from_utf16_lossy(&lm.identity)
                    .trim_end_matches('\0')
                    .to_string();
                let upto = s.find('}').map(|i| i + 1).unwrap_or(s.len());
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

impl crate::gw2::airborne::MotionSource for MumbleLink {
    fn read_motion(&self) -> Option<super::airborne::MotionSample> {
        self.read_linked_mem()
            .map(|lm| (lm.f_avatar_position, lm.f_avatar_front, lm.ui_tick))
    }
}
