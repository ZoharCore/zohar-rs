use anyhow::{Result, bail};
use zohar_content::types::player::{
    PlayerClass as ContentPlayerClass, PlayerClassBaseStats as ContentPlayerClassBaseStats,
};
use zohar_db::PlayerCoreStatAllocationRow;
use zohar_domain::Empire;
use zohar_domain::entity::player::PlayerClass as DomainPlayerClass;
use zohar_domain::entity::player::PlayerStats;

impl Default for EmpireStartMaps {
    fn default() -> Self {
        Self {
            red: "metin2_map_a1".into(),
            yellow: "metin2_map_b1".into(),
            blue: "metin2_map_c1".into(),
        }
    }
}

impl Default for PlayerCreateBaseStatTable {
    fn default() -> Self {
        Self(vec![
            (
                DomainPlayerClass::Warrior,
                PlayerCreateBaseStats {
                    stat_str: 6,
                    stat_vit: 4,
                    stat_dex: 3,
                    stat_int: 3,
                },
            ),
            (
                DomainPlayerClass::Ninja,
                PlayerCreateBaseStats {
                    stat_str: 4,
                    stat_vit: 3,
                    stat_dex: 6,
                    stat_int: 3,
                },
            ),
            (
                DomainPlayerClass::Sura,
                PlayerCreateBaseStats {
                    stat_str: 5,
                    stat_vit: 3,
                    stat_dex: 3,
                    stat_int: 5,
                },
            ),
            (
                DomainPlayerClass::Shaman,
                PlayerCreateBaseStats {
                    stat_str: 3,
                    stat_vit: 4,
                    stat_dex: 3,
                    stat_int: 6,
                },
            ),
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerCreateBaseStats {
    pub stat_str: u8,
    pub stat_vit: u8,
    pub stat_dex: u8,
    pub stat_int: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCreateBaseStatTable(Vec<(DomainPlayerClass, PlayerCreateBaseStats)>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmpireStartMaps {
    red: String,
    yellow: String,
    blue: String,
}

impl EmpireStartMaps {
    pub fn from_options(red: Option<String>, yellow: Option<String>, blue: Option<String>) -> Self {
        let defaults = Self::default();

        Self {
            red: red.unwrap_or(defaults.red),
            yellow: yellow.unwrap_or(defaults.yellow),
            blue: blue.unwrap_or(defaults.blue),
        }
    }

    pub fn validate(self) -> Result<Self> {
        if self.red.trim().is_empty() {
            bail!("red start map code cannot be empty");
        }
        if self.yellow.trim().is_empty() {
            bail!("yellow start map code cannot be empty");
        }
        if self.blue.trim().is_empty() {
            bail!("blue start map code cannot be empty");
        }
        Ok(self)
    }

    pub fn map_code_for_empire(&self, empire: Empire) -> &str {
        match empire {
            Empire::Red => &self.red,
            Empire::Yellow => &self.yellow,
            Empire::Blue => &self.blue,
        }
    }
}

impl PlayerCreateBaseStatTable {
    pub fn from_content_rows(content_rows: &[ContentPlayerClassBaseStats]) -> Self {
        Self(
            content_rows
                .iter()
                .filter_map(|row| {
                    Some((
                        match row.player_class {
                            ContentPlayerClass::Warrior => DomainPlayerClass::Warrior,
                            ContentPlayerClass::Ninja => DomainPlayerClass::Ninja,
                            ContentPlayerClass::Sura => DomainPlayerClass::Sura,
                            ContentPlayerClass::Shaman => DomainPlayerClass::Shaman,
                        },
                        PlayerCreateBaseStats {
                            stat_str: u8::try_from(row.base_strength).ok()?,
                            stat_vit: u8::try_from(row.base_vitality).ok()?,
                            stat_dex: u8::try_from(row.base_dexterity).ok()?,
                            stat_int: u8::try_from(row.base_intelligence).ok()?,
                        },
                    ))
                })
                .collect(),
        )
    }

    pub fn get(&self, class: DomainPlayerClass) -> Option<PlayerCreateBaseStats> {
        self.0
            .iter()
            .find_map(|(candidate, stats)| (*candidate == class).then_some(*stats))
    }

    pub fn resolve_player_stats(
        &self,
        class: DomainPlayerClass,
        allocations: PlayerCoreStatAllocationRow,
    ) -> Option<PlayerStats> {
        let base = self.get(class)?;

        Some(PlayerStats {
            stat_str: i32::from(base.stat_str) + allocations.allocated_str,
            stat_vit: i32::from(base.stat_vit) + allocations.allocated_vit,
            stat_dex: i32::from(base.stat_dex) + allocations.allocated_dex,
            stat_int: i32::from(base.stat_int) + allocations.allocated_int,
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &(DomainPlayerClass, PlayerCreateBaseStats)> {
        self.0.iter()
    }
}
