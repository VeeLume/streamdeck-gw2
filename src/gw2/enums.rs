use num_enum::TryFromPrimitive;
use serde::{Deserialize, Serialize};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive, Serialize, Deserialize)]
pub enum KeyControl {
    // Movement
    MovementMoveForward = 0,
    MovementMoveBackward = 1,
    MovementStrafeLeft = 2,
    MovementStrafeRight = 3,
    MovementTurnLeft = 4,
    MovementTurnRight = 5,
    MovementDodge = 6,
    MovementAutorun = 7,
    MovementWalk = 8,
    MovementJump = 9,
    MovementSwimUp = 10,
    MovementSwimDown = 11,
    MovementAboutFace = 12,

    // Skills
    SkillsSwapWeapons = 17,
    SkillsWeaponSkill1 = 18,
    SkillsWeaponSkill2 = 19,
    SkillsWeaponSkill3 = 20,
    SkillsWeaponSkill4 = 21,
    SkillsWeaponSkill5 = 22,
    SkillsHealingSkill = 23,
    SkillsUtilitySkill1 = 24,
    SkillsUtilitySkill2 = 25,
    SkillsUtilitySkill3 = 26,
    SkillsEliteSkill = 27,
    SkillsProfessionSkill1 = 28,
    SkillsProfessionSkill2 = 29,
    SkillsProfessionSkill3 = 30,
    SkillsProfessionSkill4 = 31,
    SkillsProfessionSkill5 = 79,
    SkillsProfessionSkill6 = 201,
    SkillsProfessionSkill7 = 202,
    SkillsSpecialAction = 82,

    // Targeting
    TargetingAlertTarget = 131,
    TargetingCallTarget = 32,
    TargetingTakeTarget = 33,
    TargetingSetPersonalTarget = 199,
    TargetingTakePersonalTarget = 200,
    TargetingNearestEnemy = 34,
    TargetingNextEnemy = 35,
    TargetingPreviousEnemy = 36,
    TargetingNearestAlly = 37,
    TargetingNextAlly = 38,
    TargetingPreviousAlly = 39,
    TargetingLockAutotarget = 40,
    TargetingSnapGroundTarget = 80,
    TargetingToggleSnapGroundTarget = 115,
    TargetingDisableAutotargeting = 116,
    TargetingToggleAutotargeting = 117,
    TargetingAllyTargetingMode = 197,
    TargetingToggleAllyTargetingMode = 198,

    // UI
    UiBlackLionTradingDialog = 41,
    UiContactsDialog = 42,
    UiGuildDialog = 43,
    UiHeroDialog = 44,
    UiInventoryDialog = 45,
    UiPetDialog = 46,
    UiLogOut = 47,
    UiMailDialog = 71,
    UiOptionsDialog = 48,
    UiPartyDialog = 49,
    UiPvPPanel = 73,
    UiPvPBuild = 75,
    UiScoreboard = 50,
    UiWizardsVaultDialog = 209,
    UiInformationDialog = 51,
    UiShowHideChat = 70,
    UiChatCommand = 52,
    UiChatMessage = 53,
    UiChatReply = 54,
    UiShowHideUi = 55,
    UiShowHideSquadBroadcastChat = 85,
    UiSquadBroadcastChatCommand = 83,
    UiSquadBroadcastMessage = 84,

    // Camera
    CameraFreeCamera = 13,
    CameraZoomIn = 14,
    CameraZoomOut = 15,
    CameraLookBehind = 16,
    CameraToggleActionCamera = 78,
    CameraDisableActionCamera = 114,

    // Screenshot
    ScreenshotNormal = 56,
    ScreenshotStereoscopic = 57,

    // Map
    MapOpenClose = 59,
    MapRecenter = 60,
    MapFloorDown = 61,
    MapFloorUp = 62,
    MapZoomIn = 63,
    MapZoomOut = 64,

    // Mounts
    MountsMountDismount = 152,
    MountsMountAbility1 = 130,
    MountsMountAbility2 = 153,
    MountsRaptor = 155,
    MountsSpringer = 156,
    MountsSkimmer = 157,
    MountsJackal = 158,
    MountsGriffon = 159,
    MountsRollerBeetle = 161,
    MountsWarclaw = 169,
    MountsSkyscale = 170,
    MountsTurtle = 203,

    // Spectators
    SpectatorsNearestFixedCamera = 102,
    SpectatorsNearestPlayer = 103,
    SpectatorsRedPlayer1 = 104,
    SpectatorsRedPlayer2 = 105,
    SpectatorsRedPlayer3 = 106,
    SpectatorsRedPlayer4 = 107,
    SpectatorsRedPlayer5 = 108,
    SpectatorsBluePlayer1 = 109,
    SpectatorsBluePlayer2 = 110,
    SpectatorsBluePlayer3 = 111,
    SpectatorsBluePlayer4 = 112,
    SpectatorsBluePlayer5 = 113,
    SpectatorsFreeCamera = 120,
    SpectatorsFreeCameraBoost = 127,
    SpectatorsFreeCameraForward = 121,
    SpectatorsFreeCameraBackward = 122,
    SpectatorsFreeCameraLeft = 123,
    SpectatorsFreeCameraRight = 124,
    SpectatorsFreeCameraUp = 125,
    SpectatorsFreeCameraDown = 126,

    // Squad
    SquadLocationArrow = 86,
    SquadLocationCircle = 87,
    SquadLocationHeart = 88,
    SquadLocationSquare = 89,
    SquadLocationStar = 90,
    SquadLocationSpiral = 91,
    SquadLocationTriangle = 92,
    SquadLocationX = 93,
    SquadClearAllLocationMarkers = 119,
    SquadObjectArrow = 94,
    SquadObjectCircle = 95,
    SquadObjectHeart = 96,
    SquadObjectSquare = 97,
    SquadObjectStar = 98,
    SquadObjectSpiral = 99,
    SquadObjectTriangle = 100,
    SquadObjectX = 101,
    SquadClearAllObjectMarkers = 118,

    // Mastery Skills
    MasterySkillsActivateMasterySkill = 196,
    MasterySkillsStartFishing = 204,
    MasterySkillsSummonSkiff = 205,
    MasterySkillsSetJadeBotWaypoint = 206,
    MasterySkillsScanForRift = 207,
    MasterySkillsSkyscaleLeap = 208,
    MasterySkillsConjuredDoorway = 211,

    // Misc
    MiscellaneousAoELoot = 74,
    MiscellaneousInteract = 65,
    MiscellaneousShowEnemyNames = 66,
    MiscellaneousShowAllyNames = 67,
    MiscellaneousStowDrawWeapon = 68,
    MiscellaneousToggleLanguage = 69,
    MiscellaneousRangerPetCombatToggle = 76,
    MiscellaneousToggleFullScreen = 160,
    MiscellaneousEquipUnequipNovelty = 162,
    MiscellaneousActivateChair = 163,
    MiscellaneousActivateMusicalInstrument = 164,
    MiscellaneousActivateHeldItem = 165,
    MiscellaneousActivateToy = 166,
    MiscellaneousActivateTonic = 167,
    MiscellaneousDecorateModeToggle = 210,

    // Templates
    TemplatesBuildTemplate1 = 171,
    TemplatesBuildTemplate2 = 172,
    TemplatesBuildTemplate3 = 173,
    TemplatesBuildTemplate4 = 174,
    TemplatesBuildTemplate5 = 175,
    TemplatesBuildTemplate6 = 176,
    TemplatesBuildTemplate7 = 177,
    TemplatesBuildTemplate8 = 178,
    TemplatesBuildTemplate9 = 179,
    TemplatesEquipmentTemplate1 = 182,
    TemplatesEquipmentTemplate2 = 183,
    TemplatesEquipmentTemplate3 = 184,
    TemplatesEquipmentTemplate4 = 185,
    TemplatesEquipmentTemplate5 = 186,
    TemplatesEquipmentTemplate6 = 187,
    TemplatesEquipmentTemplate7 = 188,
    TemplatesEquipmentTemplate8 = 189,
    TemplatesEquipmentTemplate9 = 190,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CharacterData {
    pub name: String,
    #[serde(default)]
    pub build_tabs: Vec<BuildTab>,
    #[serde(default)]
    pub equipment_tabs: Vec<EquipmentTab>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub is_active: bool,
    pub build: Build,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EquipmentTab {
    #[serde(rename = "tab")]
    pub tab_index: u8,
    pub name: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateNames {
    /// 1-based slots -> optional display names
    pub build: [Option<String>; 9],
    pub equipment: [Option<String>; 9],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterChange {
    /// Character was added
    Added,
    /// Character was removed
    Removed,
}
