#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarriorSkillBranch {
    Body,
    Mental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NinjaSkillBranch {
    BladeFight,
    Archery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuraSkillBranch {
    Weaponry,
    BlackMagic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShamanSkillBranch {
    Dragon,
    Healing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillBranch {
    Warrior(WarriorSkillBranch),
    Ninja(NinjaSkillBranch),
    Sura(SuraSkillBranch),
    Shaman(ShamanSkillBranch),
}
