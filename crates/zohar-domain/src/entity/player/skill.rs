#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarriorSkillBranch {
    Body,
    Mental,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NinjaSkillBranch {
    BladeFight,
    Archery,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuraSkillBranch {
    Weaponry,
    BlackMagic,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShamanSkillBranch {
    Dragon,
    Healing,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillBranch {
    Warrior(WarriorSkillBranch),
    Ninja(NinjaSkillBranch),
    Sura(SuraSkillBranch),
    Shaman(ShamanSkillBranch),
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillId {
    ThreeWayCut,
    SwordSpin,
    Berserk,
    AuraOfTheSword,
    Dash,
    LifeForce,
    Shockwave,
    Bash,
    Stump,
    StrongBody,
    SwordStrike,
    SwordOrb,
    Ambush,
    FastAttack,
    RollingDagger,
    Stealth,
    PoisonousCloud,
    InsidiousPoison,
    RepetitiveShot,
    ArrowShower,
    FireArrow,
    FeatherWalk,
    PoisonArrow,
    Spark,
    FingerStrike,
    DragonSwirl,
    EnchantedBlade,
    Fear,
    EnchantedArmor,
    Dispel,
    DarkStrike,
    FlameStrike,
    FlameSpirit,
    DarkProtection,
    SpiritStrike,
    DarkOrb,
    FlyingTalisman,
    ShootingDragon,
    DragonRoar,
    Blessing,
    Reflect,
    DragonAid,
    LightningThrow,
    SummonLightning,
    LightningClaw,
    Cure,
    Swiftness,
    AttackUp,
    Leadership,
    Combo,
    Fishing,
    Mining,
    LanguageShinsoo,
    LanguageChunjo,
    LanguageJinno,
    Polymorph,
    HorseRiding,
    HorseSummon,
    HorseWildAttack,
    HorseCharge,
    HorseEscape,
    HorseWildAttackRange,
    AddHp,
    PenetrationResistance,
    GuildEye,
    GuildBlood,
    GuildBless,
    GuildSeonghwi,
    GuildAcceleration,
    GuildBunno,
    GuildJumun,
    GuildTeleport,
    GuildDoor,
}
