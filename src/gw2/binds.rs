use std::collections::HashMap;
use std::sync::Arc;

use bitflags::bitflags;
use serde::{ Deserialize, Serialize };
use streamdeck_lib::{
    input::{ InputStep, Key, MouseButton },
    logger::ActionLog,
    prelude::{ chord, click, down, up },
    warn,
};

use super::enums::{ KeyControl };

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Device {
    Keyboard,
    Mouse,
    Unset,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Mods: u8 {
        const SHIFT = 1;
        const CTRL  = 2;
        const ALT   = 4;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub device: Device,
    pub mods: Mods,
    pub key: Option<Key>, // when device=Keyboard
    pub mouse: Option<MouseButton>, // when device=Mouse
}

impl Binding {
    pub fn to_steps(&self) -> Option<Vec<InputStep>> {
        let mut steps = Vec::new();

        let mut m: Vec<Key> = Vec::new();
        if self.mods.contains(Mods::SHIFT) {
            m.push(Key::LShift);
        }
        if self.mods.contains(Mods::CTRL) {
            m.push(Key::LCtrl);
        }
        if self.mods.contains(Mods::ALT) {
            m.push(Key::LAlt);
        }

        match self.device {
            Device::Unset => {
                return None; // no action
            }
            Device::Keyboard => {
                let Some(main) = self.key else {
                    return None; // no action
                };

                Some(chord(&m, main))
            }
            Device::Mouse => {
                let Some(btn) = self.mouse else {
                    return Some(steps);
                };
                // wrap mouse click with held modifiers
                for k in &m {
                    if let Some(s) = down(*k) {
                        steps.push(s);
                    }
                }
                steps.extend(click(btn));
                for k in m.iter().rev() {
                    if let Some(s) = up(*k) {
                        steps.push(s);
                    }
                }
                Some(steps)
            }
        }
    }
}

/// For each control, zero, one, or two bindings (primary/secondary). We’ll use the first.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct BindingSet {
    map: HashMap<KeyControl, Vec<Binding>>,
}

impl BindingSet {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub fn get(&self, kc: KeyControl) -> Option<&[Binding]> {
        self.map.get(&kc).map(|v| v.as_slice())
    }

    pub fn set(&mut self, kc: KeyControl, v: Vec<Binding>) {
        self.map.insert(kc, v);
    }

    pub fn reset_to_default(&mut self) {
        *self = Self::with_default();
    }

    pub fn with_default() -> Self {
        let mut s = Self::new();

        // Helpers: combine Mod flags, defaulting to empty when none given.
        macro_rules! mods {
            () => { Mods::empty() };
            ($($m:ident),+ $(,)?) => {
                {
                    let mut __m = Mods::empty();
                    $( __m |= Mods::$m; )*
                    __m
                }
            };
        }

        // bind!: turn a readable spec into (KeyControl, Vec<Binding>)
        macro_rules! bind {
            // primary only
            ($control:ident, primary: ($key:ident $(, $($mods:ident),*)?)) => {
                {
                    let mut __v = Vec::new();
                    __v.push(kb_key(Key::$key, mods!($( $( $mods ),* )? )));
                    (KeyControl::$control, __v)
                    }
            };
            // primary + secondary
            (
                $control:ident,
                primary: ($key:ident $(, $($mods:ident),*)?),
                secondary: ($key2:ident $(, $($mods2:ident),*)?)
            ) => {
                    {
                    let mut __v = Vec::new();
                    __v.push(kb_key(Key::$key, mods!($( $( $mods ),* )? )));
                    __v.push(kb_key(Key::$key2, mods!($( $( $mods2 ),* )? )));
                    (KeyControl::$control, __v)
                }
            };
        }

        // apply a block of bind! entries into a BindingSet via .set()
        macro_rules! apply_binds {
            ($set:expr, { $($b:expr;)* }) => {
                {
                    $(
                        {
                            let (__kc, __vec) = $b;
                            $set.set(__kc, __vec);
                        }
                    )*
                }
            };
        }

        apply_binds!(s, {
            bind!(MovementMoveForward, primary: (W), secondary: (ArrowUp));
            bind!(MovementMoveBackward, primary: (S), secondary: (ArrowDown));
            bind!(MovementStrafeLeft, primary: (A), secondary: (ArrowLeft));
            bind!(MovementStrafeRight, primary: (D), secondary: (ArrowRight));
            bind!(MovementTurnLeft, primary: (Q));
            bind!(MovementTurnRight, primary: (E));
            bind!(MovementDodge, primary: (V));
            bind!(MovementAutorun, primary: (R), secondary: (NpLock));
            bind!(MovementJump, primary: (Space));
            bind!(MovementSwimUp, primary: (Space));
            bind!(SkillsSwapWeapons, primary: (Grave));
            bind!(SkillsWeaponSkill1, primary: (D1));
            bind!(SkillsWeaponSkill2, primary: (D2));
            bind!(SkillsWeaponSkill3, primary: (D3));
            bind!(SkillsWeaponSkill4, primary: (D4));
            bind!(SkillsWeaponSkill5, primary: (D5));
            bind!(SkillsHealingSkill, primary: (D6));
            bind!(SkillsUtilitySkill1, primary: (D7));
            bind!(SkillsUtilitySkill2, primary: (D8));
            bind!(SkillsUtilitySkill3, primary: (D9));
            bind!(SkillsEliteSkill, primary: (D0));
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
            bind!(UiChatMessage, primary: (Enter), secondary: (NpEnter));
            bind!(UiChatReply, primary: (Backspace));
            bind!(UiShowHideUi, primary: (H, CTRL, SHIFT));
            bind!(UiShowHideSquadBroadcastChat, primary: (Backslash, SHIFT));
            bind!(UiSquadBroadcastMessage, primary: (Slash, SHIFT));
            bind!(UiSquadBroadcastMessage, primary: (Enter, SHIFT), secondary: (NpEnter, SHIFT));
            bind!(CameraZoomIn, primary: (PageUp));
            bind!(CameraZoomOut, primary: (PageDown));
            bind!(ScreenshotNormal, primary: (Print));
            bind!(MapOpenClose, primary: (M));
            bind!(MapRecenter, primary: (Space));
            bind!(MapFloorDown, primary: (PageDown));
            bind!(MapFloorUp, primary: (PageUp));
            bind!(MapZoomIn, primary: (NpAdd), secondary: (Equal));
            bind!(MapZoomOut, primary: (NpSubtract), secondary: (Minus));
            bind!(MountsMountDismount, primary: (X));
            bind!(MountsMountAbility1, primary: (V));
            bind!(MountsMountAbility2, primary: (C));
            bind!(SpectatorsNearestFixedCamera, primary: (Tab, SHIFT));
            bind!(SpectatorsNearestPlayer, primary: (Tab));
            bind!(SpectatorsRedPlayer1, primary: (D1));
            bind!(SpectatorsRedPlayer2, primary: (D2));
            bind!(SpectatorsRedPlayer3, primary: (D3));
            bind!(SpectatorsRedPlayer4, primary: (D4));
            bind!(SpectatorsRedPlayer5, primary: (D5));
            bind!(SpectatorsBluePlayer1, primary: (D6));
            bind!(SpectatorsBluePlayer2, primary: (D7));
            bind!(SpectatorsBluePlayer3, primary: (D8));
            bind!(SpectatorsBluePlayer4, primary: (D9));
            bind!(SpectatorsBluePlayer5, primary: (D0));
            bind!(SpectatorsFreeCamera, primary: (F, CTRL, SHIFT));
            bind!(SpectatorsFreeCameraBoost, primary: (E));
            bind!(SpectatorsFreeCameraForward, primary: (W));
            bind!(SpectatorsFreeCameraBackward, primary: (S));
            bind!(SpectatorsFreeCameraLeft, primary: (A));
            bind!(SpectatorsFreeCameraRight, primary: (D));
            bind!(SpectatorsFreeCameraUp, primary: (Space));
            bind!(SpectatorsFreeCameraDown, primary: (V));
            bind!(SquadLocationArrow, primary: (D1, ALT));
            bind!(SquadLocationCircle, primary: (D2, ALT));
            bind!(SquadLocationHeart, primary: (D3, ALT));
            bind!(SquadLocationSquare, primary: (D4, ALT));
            bind!(SquadLocationStar, primary: (D5, ALT));
            bind!(SquadLocationSpiral, primary: (D6, ALT));
            bind!(SquadLocationTriangle, primary: (D7, ALT));
            bind!(SquadLocationX, primary: (D8, ALT));
            bind!(SquadClearAllLocationMarkers, primary: (D9, ALT));
            bind!(SquadObjectArrow, primary: (D1, CTRL, ALT));
            bind!(SquadObjectCircle, primary: (D2, CTRL, ALT));
            bind!(SquadObjectHeart, primary: (D3, CTRL, ALT));
            bind!(SquadObjectSquare, primary: (D4, CTRL, ALT));
            bind!(SquadObjectStar, primary: (D5, CTRL, ALT));
            bind!(SquadObjectSpiral, primary: (D6, CTRL, ALT));
            bind!(SquadObjectTriangle, primary: (D7, CTRL, ALT));
            bind!(SquadObjectX, primary: (D9, CTRL, ALT));
            bind!(SquadClearAllObjectMarkers, primary: (D9, CTRL, ALT));
            bind!(MasterySkillsActivateMasterySkill, primary: (J));
            bind!(MiscellaneousInteract, primary: (F));
            bind!(MiscellaneousShowEnemyNames, primary: (LCtrl));
            bind!(MiscellaneousShowAllyNames, primary: (LAlt));
            bind!(MiscellaneousToggleLanguage, primary: (RCtrl));
            bind!(MiscellaneousToggleFullScreen, primary: (Enter, CTRL));
            bind!(MiscellaneousEquipUnequipNovelty, primary: (U));
            bind!(MiscellaneousDecorateModeToggle, primary: (L));
        });
        s
    }

    /// Patch with an exported GW2 bindings XML string.
    pub fn patch_from_xml(&mut self, xml: &str, logger: Arc<dyn ActionLog>) {
        let parsed: Result<InputBindings, _> = quick_xml::de::from_str(xml);
        let Ok(doc) = parsed else {
            warn!(logger, "GW2: failed to parse bindings XML (quick_xml)");
            return;
        };

        for a in doc.actions {
            let Ok(kc) = KeyControl::try_from(a.id) else {
                continue;
            };
            let mut v: Vec<Binding> = Vec::new();

            if let Some(b) = to_binding(a.device.as_deref(), a.button, a.mod_) {
                v.push(b);
            }
            if let Some(b) = to_binding(a.device2.as_deref(), a.button2, a.mod2_) {
                v.push(b);
            }

            if !v.is_empty() {
                self.set(kc, v);
            }
        }
    }
}

#[inline]
fn kb_key(k: Key, mods: Mods) -> Binding {
    Binding { device: Device::Keyboard, mods, key: Some(k), mouse: None }
}

#[inline]
fn ms_btn(b: MouseButton, mods: Mods) -> Binding {
    Binding { device: Device::Mouse, mods, key: None, mouse: Some(b) }
}

#[derive(Debug, Deserialize)]
struct InputBindings {
    #[serde(rename = "action")]
    actions: Vec<ActionEntry>,
}

#[derive(Debug, Deserialize)]
struct ActionEntry {
    #[serde(rename = "@id")]
    id: i32,
    #[serde(rename = "@device")]
    device: Option<String>,
    #[serde(rename = "@button")]
    button: Option<i32>,
    #[serde(rename = "@mod")]
    mod_: Option<i32>,

    #[serde(rename = "@device2")]
    device2: Option<String>,
    #[serde(rename = "@button2")]
    button2: Option<i32>,
    #[serde(rename = "@mod2")]
    mod2_: Option<i32>,
}

fn to_binding(dev: Option<&str>, btn: Option<i32>, mods: Option<i32>) -> Option<Binding> {
    let dev = dev?;
    let btn = btn?;
    let mods = mods.and_then(|m| Mods::from_bits(m as u8)).unwrap_or(Mods::empty());

    match dev {
        "Keyboard" => {
            let key = key_from_gw2_code(btn)?;
            Some(kb_key(key, mods))
        }
        "Mouse" => {
            let mouse = mouse_from_gw2_code(btn)?;
            Some(ms_btn(mouse, mods))
        }
        "None" =>
            Some(Binding {
                device: Device::Unset,
                mods,
                key: None,
                mouse: None,
            }),
        _ => None,
    }
}

fn mouse_from_gw2_code(code: i32) -> Option<MouseButton> {
    use MouseButton::*;
    match code {
        0 => Some(Left),
        1 => Some(Right),
        2 => Some(Middle),
        3 => Some(X(1)),
        4 => Some(X(2)),
        5..=20 => Some(X(code as u16)), // Mouse6–Mouse
        _ => None, // unsupported / exotic
    }
}

fn key_from_gw2_code(code: i32) -> Option<Key> {
    use Key::*;
    match code {
        0 => Some(LAlt),
        1 => Some(LCtrl),
        2 => Some(LShift),
        3 => Some(Apostrophe),
        4 => Some(Backslash),
        5 => Some(CapsLock),
        6 => Some(Comma),
        7 => Some(Minus),
        8 => Some(Equal),
        9 => Some(Escape),
        10 => Some(LBracket),
        11 => Some(NpLock),
        12 => Some(Period),
        13 => Some(RBracket),
        14 => Some(Semicolon),
        15 => Some(Slash),
        16 => Some(Print),
        17 => Some(Grave),
        18 => Some(Backspace),
        19 => Some(Delete),
        20 => Some(Enter),
        21 => Some(Space),
        22 => Some(Tab),
        23 => Some(End),
        24 => Some(Home),
        25 => Some(Insert),
        26 => Some(PageDown), // Next
        27 => Some(PageUp), // Prior
        28 => Some(ArrowDown),
        29 => Some(ArrowLeft),
        30 => Some(ArrowRight),
        31 => Some(ArrowUp),
        32 => Some(F1),
        33 => Some(F2),
        34 => Some(F3),
        35 => Some(F4),
        36 => Some(F5),
        37 => Some(F6),
        38 => Some(F7),
        39 => Some(F8),
        40 => Some(F9),
        41 => Some(F10),
        42 => Some(F11),
        43 => Some(F12),
        48 => Some(D0),
        49 => Some(D1),
        50 => Some(D2),
        51 => Some(D3),
        52 => Some(D4),
        53 => Some(D5),
        54 => Some(D6),
        55 => Some(D7),
        56 => Some(D8),
        57 => Some(D9),
        65 => Some(A),
        66 => Some(B),
        67 => Some(C),
        68 => Some(D),
        69 => Some(E),
        70 => Some(F),
        71 => Some(G),
        72 => Some(H),
        73 => Some(I),
        74 => Some(J),
        75 => Some(K),
        76 => Some(L),
        77 => Some(M),
        78 => Some(N),
        79 => Some(O),
        80 => Some(P),
        81 => Some(Q),
        82 => Some(R),
        83 => Some(S),
        84 => Some(T),
        85 => Some(U),
        86 => Some(V),
        87 => Some(W),
        88 => Some(X),
        89 => Some(Y),
        90 => Some(Z),
        91 => Some(NpAdd),
        92 => Some(NpDecimal),
        93 => Some(NpDivide),
        94 => Some(NpMultiply),
        95 => Some(Np0),
        96 => Some(Np1),
        97 => Some(Np2),
        98 => Some(Np3),
        99 => Some(Np4),
        100 => Some(Np5),
        101 => Some(Np6),
        102 => Some(Np7),
        103 => Some(Np8),
        104 => Some(Np9),
        105 => Some(NpEnter),
        106 => Some(NpSubtract),
        107 => None, // ImeKey1
        108 => None, // ImeKey2
        109 => Some(RAlt),
        110 => Some(RCtrl),
        111 => Some(Backslash),
        135 => Some(LShift),
        139 => Some(LCtrl),
        141 => Some(RCtrl),
        201 => Some(Pause),
        202 => Some(LWin),
        203 => Some(RWin),
        204 => Some(Menu),
        _ => None, // unsupported / exotic
    }
}
