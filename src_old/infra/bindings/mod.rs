// src/infra/bindings/mod.rs
use std::collections::HashMap;
use std::fs;
use std::path::{ Path, PathBuf };
use std::sync::Arc;

use roxmltree::Document;
use serde::{ Deserialize, Serialize };

use crate::logger::ActionLog;
use crate::{ log };

pub mod key_control;
pub mod key_code;
pub mod send_keys;

// Re-export for convenience elsewhere:
pub use key_control::KeyControl;
pub use key_code::{ KeyCode, MouseCode, SendInputMouseButton };

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::TryFromPrimitive, Serialize, Deserialize)]
pub enum DeviceType {
    Unset = 0,
    Mouse = 1,
    Keyboard = 2,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Modifier: i32 {
        const SHIFT = 1;
        const CTRL  = 2;
        const ALT   = 4;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub device_type: DeviceType,
    pub code: i32,
    pub modifier: Modifier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBind {
    pub primary: Option<Key>,
    pub secondary: Option<Key>,
}

/// Public so other modules/tests can use it directly if needed.
pub fn load_input_bindings(
    path: &Path,
    logger: Arc<dyn ActionLog>
) -> HashMap<KeyControl, KeyBind> {
    let mut bindings = default_input_bindings();

    let xml_data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) => {
            log!(logger, "❌ failed to read bindings XML: {}", e);
            return bindings;
        }
    };

    let doc = match Document::parse(&xml_data) {
        Ok(doc) => doc,
        Err(e) => {
            log!(logger, "❌ failed to parse bindings XML: {}", e);
            return bindings;
        }
    };

    for node in doc.descendants().filter(|n| n.has_tag_name("action")) {
        let control = match
            node
                .attribute("id")
                .and_then(|id| id.parse::<i32>().ok())
                .and_then(|id| KeyControl::try_from(id).ok())
        {
            Some(c) => c,
            None => {
                log!(logger, "⚠️ invalid or missing action id");
                continue;
            }
        };

        let bind = bindings
            .entry(control)
            .or_insert_with(|| KeyBind { primary: None, secondary: None });

        // Primary
        if let Some(device_str) = node.attribute("device") {
            let Some(device_type) = parse_device(device_str, Arc::clone(&logger)) else {
                continue;
            };
            if device_type == DeviceType::Unset {
                bind.primary = None;
            } else {
                let code = parse_code(node.attribute("button"), Arc::clone(&logger));
                let modifier = parse_modifier(node.attribute("mod"));
                bind.primary = Some(Key { device_type, code, modifier });
            }
        }

        // Secondary
        if let Some(device_str) = node.attribute("device2") {
            let Some(device_type) = parse_device(device_str, Arc::clone(&logger)) else {
                continue;
            };
            if device_type == DeviceType::Unset {
                bind.secondary = None;
            } else {
                let code = parse_code(node.attribute("button2"), Arc::clone(&logger));
                let modifier = parse_modifier(node.attribute("mod2"));
                bind.secondary = Some(Key { device_type, code, modifier });
            }
        }
    }

    bindings
}

fn parse_device(s: &str, logger: Arc<dyn ActionLog>) -> Option<DeviceType> {
    match s {
        "Mouse" => Some(DeviceType::Mouse),
        "Keyboard" => Some(DeviceType::Keyboard),
        "None" => Some(DeviceType::Unset),
        other => {
            log!(logger, "⚠️ unknown device type: {}", other);
            None
        }
    }
}

fn parse_code(attr: Option<&str>, logger: Arc<dyn ActionLog>) -> i32 {
    match attr.and_then(|c| c.parse::<i32>().ok()) {
        Some(code) => code,
        None => {
            // Old note: LeftAlt had 0, which often shows up as “missing”
            log!(logger, "⚠️ missing/invalid key code, defaulting to LeftAlt (0)");
            key_code::KeyCode::LeftAlt as i32
        }
    }
}

fn parse_modifier(attr: Option<&str>) -> Modifier {
    attr.and_then(|m| m.parse::<i32>().ok())
        .and_then(Modifier::from_bits)
        .unwrap_or_else(Modifier::empty)
}

// ---------------- defaults ----------------

pub fn default_input_bindings() -> HashMap<KeyControl, KeyBind> {
    // moved from your old bindings/default.rs (unchanged)
    use crate::infra::bindings::{ key_code::KeyCode, DeviceType::Keyboard, Modifier };
    use key_control::KeyControl::*;
    let mut map = HashMap::new();

    macro_rules! bind {
        (
            $kc:ident,
            primary: ($code1:ident $(, $mod1:ident)*),
            secondary: ($code2:ident $(, $mod2:ident)*)
        ) => {
        {
            map.insert(
                $kc,
                KeyBind {
                    primary: Some(Key {
                        device_type: Keyboard,
                        code: KeyCode::$code1 as i32,
                        modifier: Modifier::empty() $(| Modifier::$mod1)*,
                    }),
                    secondary: Some(Key {
                        device_type: Keyboard,
                        code: KeyCode::$code2 as i32,
                        modifier: Modifier::empty() $(| Modifier::$mod2)*,
                    }),
                },
            );
        }
        };
        ($kc:ident, primary: ($code1:ident $(, $mod1:ident)*)) => {
        {
            map.insert(
                $kc,
                KeyBind {
                    primary: Some(Key {
                        device_type: Keyboard,
                        code: KeyCode::$code1 as i32,
                        modifier: Modifier::empty() $(| Modifier::$mod1)*,
                    }),
                    secondary: None,
                },
            );
        }
        };
    }

    bind!(MovementMoveForward, primary: (W), secondary: (ArrowUp));
    bind!(MovementMoveBackward, primary: (S), secondary: (ArrowDown));
    bind!(MovementStrafeLeft, primary: (A), secondary: (ArrowLeft));
    bind!(MovementStrafeRight, primary: (D), secondary: (ArrowRight));
    bind!(MovementTurnLeft, primary: (Q));
    bind!(MovementTurnRight, primary: (E));
    bind!(MovementDodge, primary: (V));
    bind!(MovementAutorun, primary: (R), secondary: (NumLock));
    bind!(MovementJump, primary: (Space));
    bind!(MovementSwimUp, primary: (Space));
    bind!(SkillsSwapWeapons, primary: (Tilde));
    bind!(SkillsWeaponSkill1, primary: (Key1));
    bind!(SkillsWeaponSkill2, primary: (Key2));
    bind!(SkillsWeaponSkill3, primary: (Key3));
    bind!(SkillsWeaponSkill4, primary: (Key4));
    bind!(SkillsWeaponSkill5, primary: (Key5));
    bind!(SkillsHealingSkill, primary: (Key6));
    bind!(SkillsUtilitySkill1, primary: (Key7));
    bind!(SkillsUtilitySkill2, primary: (Key8));
    bind!(SkillsUtilitySkill3, primary: (Key9));
    bind!(SkillsEliteSkill, primary: (Key0));
    bind!(SkillsProfessionSkill1, primary: (F1));
    bind!(SkillsProfessionSkill2, primary: (F2));
    bind!(SkillsProfessionSkill3, primary: (F3));
    bind!(SkillsProfessionSkill4, primary: (F4));
    bind!(SkillsProfessionSkill5, primary: (F5));
    bind!(SkillsProfessionSkill6, primary: (F6));
    bind!(SkillsProfessionSkill7, primary: (F7));
    bind!(SkillsSpecialAction, primary: (N));
    bind!(TargetingAlertTarget, primary: (T, SHIFT));
    bind!(TargetingCallTarget, primary: (T, CTRL));
    bind!(TargetingTakeTarget, primary: (T));
    bind!(TargetingNextEnemy, primary: (Tab));
    bind!(TargetingPreviousEnemy, primary: (Tab, SHIFT));
    bind!(UiBlackLionTradingDialog, primary: (O));
    bind!(UiContactsDialog, primary: (Y));
    bind!(UiGuildDialog, primary: (G));
    bind!(UiHeroDialog, primary: (H));
    bind!(UiInventoryDialog, primary: (I));
    bind!(UiPetDialog, primary: (K));
    bind!(UiLogOut, primary: (F12));
    bind!(UiOptionsDialog, primary: (F11));
    bind!(UiPartyDialog, primary: (P));
    bind!(UiScoreboard, primary: (B));
    bind!(UiWizardsVaultDialog, primary: (H, SHIFT));
    bind!(UiInformationDialog, primary: (Minus));
    bind!(UiShowHideChat, primary: (Backslash));
    bind!(UiChatCommand, primary: (Slash));
    bind!(UiChatMessage, primary: (Enter), secondary: (EnterNum));
    bind!(UiChatReply, primary: (Backspace));
    bind!(UiShowHideUi, primary: (H, CTRL, SHIFT));
    bind!(UiShowHideSquadBroadcastChat, primary: (Backslash, SHIFT));
    bind!(UiSquadBroadcastMessage, primary: (Slash, SHIFT));
    bind!(UiSquadBroadcastMessage, primary: (Enter, SHIFT), secondary: (EnterNum, SHIFT));
    bind!(CameraZoomIn, primary: (Prior));
    bind!(CameraZoomOut, primary: (Next));
    bind!(ScreenshotNormal, primary: (Print));
    bind!(MapOpenClose, primary: (M));
    bind!(MapRecenter, primary: (Space));
    bind!(MapFloorDown, primary: (Next));
    bind!(MapFloorUp, primary: (Prior));
    bind!(MapZoomIn, primary: (PlusNum), secondary: (Equals));
    bind!(MapZoomOut, primary: (MinusNum), secondary: (Minus));
    bind!(MountsMountDismount, primary: (X));
    bind!(MountsMountAbility1, primary: (V));
    bind!(MountsMountAbility2, primary: (C));
    bind!(SpectatorsNearestFixedCamera, primary: (Tab, SHIFT));
    bind!(SpectatorsNearestPlayer, primary: (Tab));
    bind!(SpectatorsRedPlayer1, primary: (Key1));
    bind!(SpectatorsRedPlayer2, primary: (Key2));
    bind!(SpectatorsRedPlayer3, primary: (Key3));
    bind!(SpectatorsRedPlayer4, primary: (Key4));
    bind!(SpectatorsRedPlayer5, primary: (Key5));
    bind!(SpectatorsBluePlayer1, primary: (Key6));
    bind!(SpectatorsBluePlayer2, primary: (Key7));
    bind!(SpectatorsBluePlayer3, primary: (Key8));
    bind!(SpectatorsBluePlayer4, primary: (Key9));
    bind!(SpectatorsBluePlayer5, primary: (Key0));
    bind!(SpectatorsFreeCamera, primary: (F, CTRL, SHIFT));
    bind!(SpectatorsFreeCameraBoost, primary: (E));
    bind!(SpectatorsFreeCameraForward, primary: (W));
    bind!(SpectatorsFreeCameraBackward, primary: (S));
    bind!(SpectatorsFreeCameraLeft, primary: (A));
    bind!(SpectatorsFreeCameraRight, primary: (D));
    bind!(SpectatorsFreeCameraUp, primary: (Space));
    bind!(SpectatorsFreeCameraDown, primary: (V));
    bind!(SquadLocationArrow, primary: (Key1, ALT));
    bind!(SquadLocationCircle, primary: (Key2, ALT));
    bind!(SquadLocationHeart, primary: (Key3, ALT));
    bind!(SquadLocationSquare, primary: (Key4, ALT));
    bind!(SquadLocationStar, primary: (Key5, ALT));
    bind!(SquadLocationSpiral, primary: (Key6, ALT));
    bind!(SquadLocationTriangle, primary: (Key7, ALT));
    bind!(SquadLocationX, primary: (Key8, ALT));
    bind!(SquadClearAllLocationMarkers, primary: (Key9, ALT));
    bind!(SquadObjectArrow, primary: (Key1, CTRL, ALT));
    bind!(SquadObjectCircle, primary: (Key2, CTRL, ALT));
    bind!(SquadObjectHeart, primary: (Key3, CTRL, ALT));
    bind!(SquadObjectSquare, primary: (Key4, CTRL, ALT));
    bind!(SquadObjectStar, primary: (Key5, CTRL, ALT));
    bind!(SquadObjectSpiral, primary: (Key6, CTRL, ALT));
    bind!(SquadObjectTriangle, primary: (Key7, CTRL, ALT));
    bind!(SquadObjectX, primary: (Key8, CTRL, ALT));
    bind!(SquadClearAllObjectMarkers, primary: (Key9, CTRL, ALT));
    bind!(MasterySkillsActivateMasterySkill, primary: (J));
    bind!(MiscellaneousInteract, primary: (F));
    bind!(MiscellaneousShowEnemyNames, primary: (LeftCtrl));
    bind!(MiscellaneousShowAllyNames, primary: (LeftAlt));
    bind!(MiscellaneousToggleLanguage, primary: (RightCtrl));
    bind!(MiscellaneousToggleFullScreen, primary: (Enter, CTRL));
    bind!(MiscellaneousEquipUnequipNovelty, primary: (U));
    bind!(MiscellaneousDecorateModeToggle, primary: (L));
    map
}
