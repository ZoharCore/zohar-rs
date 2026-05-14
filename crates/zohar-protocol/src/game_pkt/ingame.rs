pub mod chat;
pub mod combat;
pub mod item;
pub mod movement;
pub mod stats;
pub mod system;
pub mod world;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{control_pkt, game_pkt::impl_zero_fallback_num_enum};

crate::route_packets! {
    /// Client-to-server packets for in-game phase.
    pub enum InGameC2s {
        Control(control_pkt::ControlC2s) from 0xFE | 0xFF | 0xFC,
        Move(movement::MovementC2s) from 0x06 | 0x07,
        Chat(chat::ChatC2s) from 0x03,
        Combat(combat::CombatC2s) from 0x02 | 0x3D,
        // ItemDropped(item::dropped::DroppedItemC2s) from 0x0F,
        // ItemInventory(item::inventory::InventoryItemC2s) from 0x0B | 0x0C | 0x0D | 0x14 | 0x3C | 0x53,
        // ItemShop(item::shop::ShopC2s) from 0x32 | 0x37,
    }
}

crate::route_packets! {
    /// Server-to-client packets for in-game phase.
    pub enum InGameS2c {
        Control(control_pkt::ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        System(system::SystemS2c) from 0x41 | 0x6A | 0x79,
        Move(movement::MovementS2c) from 0x03 | 0x6F,
        Chat(chat::ChatS2c) from 0x04,
        World(world::WorldS2c) from 0x01 | 0x02 | 0x13 | 0x88,
        Stats(stats::StatsS2c) from 0x10 | 0x11,
        Combat(combat::CombatS2c) from 0x0D | 0x0E | 0x3F | 0x45 | 0x46 | 0x47 | 0x72 | 0x87,
        // ItemDropped(item::dropped::DroppedItemS2c) from 0x1A | 0x1B | 0x1F,
        // ItemInventory(item::inventory::InventoryItemS2c) from 0x14 | 0x15 | 0x16 | 0x17 | 0x19,
        // ItemShop(item::shop::ShopS2c) from 0x26 | 0x27,
    }
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum Skill {
    // Player Skills
    // Warrior Body
    ThreeWayCut = 1,
    SwordSpin = 2,
    Berserk = 3,
    AuraOfTheSword = 4,
    Dash = 5,
    LifeForce = 6,
    // Warrior Mental
    Shockwave = 16,
    Bash = 17,
    Stump = 18,
    StrongBody = 19,
    SwordStrike = 20,
    SwordOrb = 21,
    // Ninja Dagger
    Ambush = 31,
    FastAttack = 32,
    RollingDagger = 33,
    Stealth = 34,
    PoisonousCloud = 35,
    InsidiousPoison = 36,
    // Ninja Archer
    RepetitiveShot = 46,
    ArrowShower = 47,
    FireArrow = 48,
    FeatherWalk = 49,
    PoisonArrow = 50,
    Spark = 51,
    // Sura Weapons
    FingerStrike = 61,
    DragonSwirl = 62,
    EnchantedBlade = 63,
    Fear = 64,
    EnchantedArmor = 65,
    Dispel = 66,
    // Sura Black Magic
    DarkStrike = 76,
    FlameStrike = 77,
    FlameSpirit = 78,
    DarkProtection = 79,
    SpiritStrike = 80,
    DarkOrb = 81,
    // Shaman Dragon
    FlyingTalisman = 91,
    ShootingDragon = 92,
    DragonRoar = 93,
    Blessing = 94,
    Reflect = 95,
    DragonAid = 96,
    // Shaman Healing
    LightningThrow = 106,
    SummonLightning = 107,
    LightningClaw = 108,
    Cure = 109,
    Swiftness = 110,
    AttackUp = 111,

    // Passive Skills
    Leadership = 121,
    Combo = 122,
    Fishing = 123,
    Mining = 124,
    LanguageShinsoo = 126,
    LanguageChunjo = 127,
    LanguageJinno = 128,
    Polymorph = 129,

    // Horse Skills
    HorseRiding = 130,
    HorseSummon = 131,
    HorseWildAttack = 137,
    HorseCharge = 138,
    HorseEscape = 139,
    HorseWildAttackRange = 140,

    AddHp = 141,
    PenetrationResistance = 142,

    GuildEye = 151,
    GuildBlood = 152,
    GuildBless = 153,
    GuildSeonghwi = 154,
    GuildAcceleration = 155,
    GuildBunno = 156,
    GuildJumun = 157,
    GuildTeleport = 158,
    GuildDoor = 159,
}
impl_zero_fallback_num_enum!(Skill, u8);
