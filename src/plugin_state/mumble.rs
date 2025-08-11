use std::{ slice, sync::Arc };
use windows::Win32::Foundation::*;
use windows::Win32::System::Memory::*;
use bytemuck::{ Pod, Zeroable };
use serde::Deserialize;
use bitflags::bitflags;

use crate::{ log, logger::{ self, ActionLog } };

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
    pub fn from_raw(raw: u32) -> Self {
        UiState::from_bits_truncate(raw)
    }

    pub fn is_in_combat(self) -> bool {
        self.contains(UiState::IN_COMBAT)
    }

    pub fn is_map_open(self) -> bool {
        self.contains(UiState::MAP_OPEN)
    }

    pub fn has_focus(self) -> bool {
        self.contains(UiState::GAME_HAS_FOCUS)
    }

    pub fn is_textbox_active(self) -> bool {
        self.contains(UiState::TEXTBOX_HAS_FOCUS)
    }

    pub fn is_competitive_mode(self) -> bool {
        self.contains(UiState::COMPETITIVE_MODE)
    }

    pub fn is_compass_top_right(self) -> bool {
        self.contains(UiState::COMPASS_TOP_RIGHT)
    }

    pub fn has_compass_rotation(self) -> bool {
        self.contains(UiState::COMPASS_ROTATION)
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
    pub _padding: [u8; 3], // for alignment
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
        ).map_err(|_| "Failed to open MumbleLink shared memory".to_string())?;

        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, SHARED_MEM_SIZE) };
        if ptr.Value.is_null() {
            unsafe {
                CloseHandle(handle);
            }
            return Err("Failed to map view of file".to_string());
        }

        Ok(MumbleLink { map_handle: handle, view_ptr: ptr })
    }

    pub fn read(
        &self,
        parse_identity: bool,
    ) -> Option<(LinkedMem, Option<MumbleContext>, Option<Identity>)> {
        unsafe {
            let bytes = slice::from_raw_parts(self.view_ptr.Value as *const u8, SHARED_MEM_SIZE);
            let lm: LinkedMem = *bytemuck::from_bytes::<LinkedMem>(bytes);

            let context = bytemuck
                ::try_from_bytes::<MumbleContext>(
                    &lm.context[..std::mem::size_of::<MumbleContext>()]
                )
                .ok()
                .copied();

            let identity = if parse_identity {
                let identity_str = String::from_utf16_lossy(&lm.identity)
                    .trim_end_matches('\0')
                    .to_string();

                // Strip trailing junk after a valid JSON object using a heuristic
                let identity_str = match identity_str.find("}") {
                    Some(pos) => &identity_str[..=pos],
                    None => &identity_str, // fallback
                };

                let identity_json = if identity_str.trim().starts_with('{') {
                    serde_json::from_str(&identity_str).ok()
                } else {
                    None
                };

                identity_json
            } else {
                None
            };

            Some((lm, context, identity))
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
