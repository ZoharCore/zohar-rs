pub mod chat;
pub mod combat;
pub mod fishing;
pub mod guild;
pub mod movement;
pub mod system;
pub mod trading;
pub mod world;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{
    control_pkt::{ControlC2s, ControlS2c},
    game_pkt::impl_zero_fallback_num_enum,
};

crate::route_packets! {
    /// Client-to-server packets for in-game phase.
    pub enum InGameC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
        Combat(combat::CombatC2s) from 0x02 | 0x3D,
        Chat(chat::ChatC2s) from 0x03,
        Move(movement::MovementC2s) from 0x06 | 0x07,
        Trading(trading::TradingC2s) from 0x50,
        Guild(guild::GuildC2s) from 0x60,
        Fishing(fishing::FishingC2s) from 0x70,
    }
}

crate::route_packets! {
    /// Server-to-client packets for in-game phase.
    pub enum InGameS2c {
        Control(ControlS2c) from 0x2C | 0xFF | 0xFC | 0xFD,
        Move(movement::MovementS2c) from 0x03 | 0x6F,
        Chat(chat::ChatS2c) from 0x04,
        System(system::SystemS2c) from 0x6A | 0x79,
        World(world::WorldS2c) from 0x01 | 0x88,
        Trading(trading::TradingS2c) from 0x51,
        Guild(guild::GuildS2c) from 0x61,
        Fishing(fishing::FishingS2c) from 0x71,
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
