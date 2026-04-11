use crate::adapters::ToProtocol;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use tracing::warn;
use zohar_content::types::ContentCatalog;
use zohar_content::types::empires::Empire as ContentEmpire;
use zohar_db::PlayerRuntimeStateRow;
use zohar_domain::coords::{LocalPos, LocalSize, WorldPos};
use zohar_domain::{Empire as DomainEmpire, Empire, MapId};
use zohar_protocol::game_pkt::WireWorldPos;

const CM_PER_METER: f32 = 100.0;

pub(crate) fn wire_cm_to_meters(coord_cm: i32) -> f32 {
    coord_cm as f32 / CM_PER_METER
}

pub(crate) fn meters_to_wire_cm(coord_m: f32) -> i32 {
    (coord_m * CM_PER_METER).trunc() as i32
}

impl ToProtocol<WireWorldPos> for WorldPos {
    fn to_protocol(self) -> WireWorldPos {
        WireWorldPos {
            x_cm: meters_to_wire_cm(self.x),
            y_cm: meters_to_wire_cm(self.y),
        }
    }
}

#[derive(Debug, Clone)]
struct MapCoordMeta {
    map_id: MapId,
    map_code: String,
    base_x: f32,
    base_y: f32,
    map_width: f32,
    map_height: f32,
}

impl MapCoordMeta {
    fn contains_local(&self, local: LocalPos) -> bool {
        if !local.x.is_finite() || !local.y.is_finite() {
            return false;
        }
        local.x >= 0.0 && local.x < self.map_width && local.y >= 0.0 && local.y < self.map_height
    }

    fn to_world(&self, local: LocalPos) -> WorldPos {
        WorldPos::new(self.base_x + local.x, self.base_y + local.y)
    }

    fn to_local(&self, world: WorldPos) -> Option<LocalPos> {
        let local_x = world.x - self.base_x;
        let local_y = world.y - self.base_y;
        let local = LocalPos::new(local_x, local_y);
        if self.contains_local(local) {
            Some(local)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EmpireStart {
    map_id: MapId,
    local_x: f32,
    local_y: f32,
}

#[derive(Debug, Clone, Copy)]
struct EmpireStarts {
    red: EmpireStart,
    yellow: EmpireStart,
    blue: EmpireStart,
}

impl EmpireStarts {
    fn get(self, empire: DomainEmpire) -> EmpireStart {
        match empire {
            DomainEmpire::Red => self.red,
            DomainEmpire::Yellow => self.yellow,
            DomainEmpire::Blue => self.blue,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PersistedPlayerPos {
    pub map_key: String,
    pub local_x: f32,
    pub local_y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedSpawn {
    pub map_id: MapId,
    pub local_pos: LocalPos,
    pub used_fallback: bool,
}

#[derive(Debug, Clone)]
pub struct ContentCoords {
    maps_by_code: HashMap<String, MapCoordMeta>,
    maps_by_id: HashMap<MapId, MapCoordMeta>,
    maps_empires: HashMap<MapId, Option<Empire>>,
    town_spawns: HashMap<(MapId, DomainEmpire), LocalPos>,
    empire_starts: EmpireStarts,
}

impl ContentCoords {
    pub fn persisted_from_runtime_state(
        player: &PlayerRuntimeStateRow,
    ) -> Option<PersistedPlayerPos> {
        match (&player.map_key, player.local_x, player.local_y) {
            (Some(map_key), Some(local_x), Some(local_y)) => Some(PersistedPlayerPos {
                map_key: map_key.clone(),
                local_x,
                local_y,
            }),
            _ => None,
        }
    }

    pub fn resolve_spawn_for_player(
        &self,
        player: Option<&PlayerRuntimeStateRow>,
        empire: DomainEmpire,
    ) -> ResolvedSpawn {
        let persisted = player.and_then(Self::persisted_from_runtime_state);
        self.resolve_spawn(persisted, empire)
    }

    pub fn spawnable_shared_map_ids(&self) -> Vec<MapId> {
        self.maps_by_id.keys().copied().collect()
    }

    pub fn map_names_by_id(&self) -> HashMap<MapId, String> {
        self.maps_by_id
            .iter()
            .map(|(map_id, meta)| (*map_id, meta.map_code.clone()))
            .collect()
    }

    pub fn map_id_by_code(&self, code: &str) -> Option<MapId> {
        self.maps_by_code.get(code).map(|meta| meta.map_id)
    }

    pub fn map_code_by_id(&self, map_id: MapId) -> Option<&str> {
        self.maps_by_id
            .get(&map_id)
            .map(|meta| meta.map_code.as_str())
    }

    pub fn map_empires_by_id(&self) -> HashMap<MapId, Option<Empire>> {
        self.maps_empires
            .iter()
            .map(|(map_id, empire_opt)| (*map_id, *empire_opt))
            .collect()
    }

    pub fn map_local_size(&self, map_id: MapId) -> Option<LocalSize> {
        self.maps_by_id
            .get(&map_id)
            .map(|meta| LocalSize::new(meta.map_width, meta.map_height))
    }

    pub fn local_to_world(&self, map_id: MapId, local_pos: LocalPos) -> Option<WorldPos> {
        self.maps_by_id.get(&map_id).and_then(|map| {
            if map.contains_local(local_pos) {
                Some(map.to_world(local_pos))
            } else {
                None
            }
        })
    }

    pub fn world_to_local(&self, map_id: MapId, world_pos: WorldPos) -> Option<LocalPos> {
        self.maps_by_id
            .get(&map_id)
            .and_then(|map| map.to_local(world_pos))
    }

    pub fn world_wire_to_local(&self, map_id: MapId, world_pos: WireWorldPos) -> Option<LocalPos> {
        let world = WorldPos::new(
            wire_cm_to_meters(world_pos.x_cm),
            wire_cm_to_meters(world_pos.y_cm),
        );
        self.world_to_local(map_id, world)
    }

    pub fn resolve_world_destination(&self, world_pos: WorldPos) -> Option<(MapId, LocalPos)> {
        let mut resolved = None;

        for (map_id, map) in &self.maps_by_id {
            let Some(local_pos) = map.to_local(world_pos) else {
                continue;
            };

            if resolved.is_some() {
                return None;
            }

            resolved = Some((*map_id, local_pos));
        }

        resolved
    }

    pub fn resolve_town_spawn(
        &self,
        map_id: MapId,
        preferred_empire: DomainEmpire,
    ) -> Option<LocalPos> {
        if let Some(pos) = self.town_spawns.get(&(map_id, preferred_empire)).copied() {
            return Some(pos);
        }

        let mut fallback = None;
        for ((spawn_map_id, _), pos) in &self.town_spawns {
            if *spawn_map_id != map_id {
                continue;
            }
            if fallback.replace(*pos).is_some() {
                return None;
            }
        }

        fallback
    }

    pub fn from_catalog(catalog: &ContentCatalog) -> Result<Self> {
        let mut maps_by_code = HashMap::with_capacity(catalog.maps.len());
        let mut maps_by_id = HashMap::with_capacity(catalog.maps.len());
        let mut maps_empires = HashMap::with_capacity(catalog.maps.len());
        let mut town_spawns = HashMap::with_capacity(catalog.town_spawns.len());

        for map in &catalog.maps {
            let Some(map_id) = map_id_from_i64(map.map_id, "maps.map_id") else {
                continue;
            };

            let raw_base_x = map.base_x.with_context(|| {
                format!(
                    "map {} ({}) is missing map_placement.base_x",
                    map.map_id, map.code
                )
            })?;
            let raw_base_y = map.base_y.with_context(|| {
                format!(
                    "map {} ({}) is missing map_placement.base_y",
                    map.map_id, map.code
                )
            })?;

            let base_x = raw_base_x;
            let base_y = raw_base_y;
            let map_width = map.map_width;
            let map_height = map.map_height;

            if !base_x.is_finite() || !base_y.is_finite() {
                bail!(
                    "map {} ({}) has non-finite placement origin",
                    map.map_id,
                    map.code
                );
            }
            if !map_width.is_finite() || !map_height.is_finite() {
                bail!(
                    "map {} ({}) has non-finite dimensions",
                    map.map_id,
                    map.code
                );
            }
            if map_width <= 0.0 || map_height <= 0.0 {
                bail!(
                    "map {} ({}) has invalid dimensions {}x{}",
                    map.map_id,
                    map.code,
                    map_width,
                    map_height
                );
            }

            // Ensure in-bounds projection remains finite.
            let max_local_x = map_width - 1.0;
            let max_local_y = map_height - 1.0;
            if !(base_x + max_local_x).is_finite() || !(base_y + max_local_y).is_finite() {
                bail!(
                    "map {} ({}) world projection produced non-finite coordinates",
                    map.map_id,
                    map.code
                );
            }

            let meta = MapCoordMeta {
                map_id,
                map_code: map.code.clone(),
                base_x,
                base_y,
                map_width,
                map_height,
            };

            if maps_by_id.insert(map_id, meta.clone()).is_some() {
                bail!("duplicate map_id {} in content catalog", map.map_id);
            }
            if maps_by_code.insert(map.code.clone(), meta).is_some() {
                bail!("duplicate map code '{}' in content catalog", map.code);
            }
            if maps_empires
                .insert(map_id, map.empire.map(map_content_empire))
                .is_some()
            {
                bail!("duplicate map_id {} in content catalog", map.map_id);
            }
        }

        if maps_by_code.is_empty() {
            bail!("content catalog does not define any maps");
        }

        for spawn in &catalog.town_spawns {
            let Some(map_id) = map_id_from_i64(spawn.map_id, "town_spawns.map_id") else {
                continue;
            };
            let empire = map_content_empire(spawn.empire);
            let local_pos = LocalPos::new(spawn.x, spawn.y);

            let map = maps_by_id.get(&map_id).with_context(|| {
                format!(
                    "town spawn for empire {:?} references unknown map_id {}",
                    empire, spawn.map_id
                )
            })?;

            if !map.contains_local(local_pos) {
                bail!(
                    "town spawn for empire {:?} on map_id {} is out of bounds at ({}, {})",
                    empire,
                    spawn.map_id,
                    local_pos.x,
                    local_pos.y
                );
            }

            if town_spawns.insert((map_id, empire), local_pos).is_some() {
                bail!(
                    "duplicate town spawn for empire {:?} on map_id {}",
                    empire,
                    spawn.map_id
                );
            }
        }

        let mut red: Option<EmpireStart> = None;
        let mut yellow: Option<EmpireStart> = None;
        let mut blue: Option<EmpireStart> = None;

        for start in &catalog.empire_start_configs {
            let domain_empire = map_content_empire(start.empire);
            let Some(map_id) =
                map_id_from_i64(start.start_map_id, "empire_start_configs.start_map_id")
            else {
                continue;
            };
            let raw_map_id = map_id.get();

            let local_x = start.start_x;
            let local_y = start.start_y;
            if !local_x.is_finite() || !local_y.is_finite() {
                bail!(
                    "empire {:?} start coordinates are non-finite",
                    domain_empire
                );
            }

            let map = maps_by_id.get(&map_id).with_context(|| {
                format!(
                    "empire {:?} start map_id {} does not exist in maps",
                    domain_empire, raw_map_id
                )
            })?;

            if !map.contains_local(LocalPos::new(local_x, local_y)) {
                bail!(
                    "empire {:?} start ({}, {}) is out of bounds for map_id {}",
                    domain_empire,
                    local_x,
                    local_y,
                    raw_map_id
                );
            }

            let start = EmpireStart {
                map_id,
                local_x,
                local_y,
            };

            match domain_empire {
                DomainEmpire::Red => {
                    if red.replace(start).is_some() {
                        bail!("duplicate start config for empire Red");
                    }
                }
                DomainEmpire::Yellow => {
                    if yellow.replace(start).is_some() {
                        bail!("duplicate start config for empire Yellow");
                    }
                }
                DomainEmpire::Blue => {
                    if blue.replace(start).is_some() {
                        bail!("duplicate start config for empire Blue");
                    }
                }
            }
        }

        let empire_starts = EmpireStarts {
            red: red.context("missing start config for empire Red")?,
            yellow: yellow.context("missing start config for empire Yellow")?,
            blue: blue.context("missing start config for empire Blue")?,
        };

        Ok(Self {
            maps_by_code,
            maps_by_id,
            maps_empires,
            town_spawns,
            empire_starts,
        })
    }

    pub fn resolve_spawn(
        &self,
        persisted: Option<PersistedPlayerPos>,
        empire: DomainEmpire,
    ) -> ResolvedSpawn {
        if let Some(saved) = persisted
            && let Some(map) = self.maps_by_code.get(saved.map_key.as_str())
        {
            let local_pos = LocalPos::new(saved.local_x, saved.local_y);
            if map.contains_local(local_pos) {
                return ResolvedSpawn {
                    map_id: map.map_id,
                    local_pos,
                    used_fallback: false,
                };
            }
        }

        // fallback to default empire start if position uninitialized or wiped
        let start = self.empire_starts.get(empire);
        self.maps_by_id
            .get(&start.map_id)
            .expect("empire start maps must be valid after startup validation");

        ResolvedSpawn {
            map_id: start.map_id,
            local_pos: LocalPos::new(start.local_x, start.local_y),
            used_fallback: true,
        }
    }
}

fn map_content_empire(empire: ContentEmpire) -> DomainEmpire {
    match empire {
        ContentEmpire::Red => DomainEmpire::Red,
        ContentEmpire::Yellow => DomainEmpire::Yellow,
        ContentEmpire::Blue => DomainEmpire::Blue,
    }
}

fn map_id_from_i64(raw: i64, field: &'static str) -> Option<MapId> {
    match u32::try_from(raw) {
        Ok(value) => Some(MapId::new(value)),
        Err(error) => {
            warn!(
                %error,
                %field,
                raw,
                "Invalid map id in content; skipping record"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_content::types::ContentCatalog;
    use zohar_content::types::empires::{Empire as ContentEmpire, EmpireStartConfig};
    use zohar_content::types::maps::{ContentMap, MapTownSpawn};

    fn base_catalog() -> ContentCatalog {
        ContentCatalog {
            maps: vec![
                ContentMap {
                    map_id: 1,
                    code: "zohar_map_a1".to_string(),
                    name: "a1".to_string(),
                    map_width: 1024.0,
                    map_height: 1280.0,
                    empire: Some(ContentEmpire::Red),
                    base_x: Some(4096.0),
                    base_y: Some(8960.0),
                },
                ContentMap {
                    map_id: 21,
                    code: "zohar_map_b1".to_string(),
                    name: "b1".to_string(),
                    map_width: 1024.0,
                    map_height: 1280.0,
                    empire: Some(ContentEmpire::Yellow),
                    base_x: Some(0.0),
                    base_y: Some(1024.0),
                },
                ContentMap {
                    map_id: 41,
                    code: "zohar_map_c1".to_string(),
                    name: "c1".to_string(),
                    map_width: 1024.0,
                    map_height: 1280.0,
                    empire: Some(ContentEmpire::Blue),
                    base_x: Some(9216.0),
                    base_y: Some(2048.0),
                },
            ],
            town_spawns: vec![
                MapTownSpawn {
                    map_id: 1,
                    empire: ContentEmpire::Red,
                    x: 620.0,
                    y: 700.0,
                },
                MapTownSpawn {
                    map_id: 21,
                    empire: ContentEmpire::Yellow,
                    x: 560.0,
                    y: 560.0,
                },
                MapTownSpawn {
                    map_id: 41,
                    empire: ContentEmpire::Blue,
                    x: 490.0,
                    y: 740.0,
                },
            ],
            empire_start_configs: vec![
                EmpireStartConfig {
                    empire: ContentEmpire::Red,
                    start_map_id: 1,
                    start_x: 597.0,
                    start_y: 682.0,
                },
                EmpireStartConfig {
                    empire: ContentEmpire::Yellow,
                    start_map_id: 21,
                    start_x: 557.0,
                    start_y: 555.0,
                },
                EmpireStartConfig {
                    empire: ContentEmpire::Blue,
                    start_map_id: 41,
                    start_x: 480.0,
                    start_y: 736.0,
                },
            ],
            ..ContentCatalog::default()
        }
    }

    #[test]
    fn fallback_when_map_key_is_unknown() {
        let coords = ContentCoords::from_catalog(&base_catalog()).expect("coords");
        let spawn = coords.resolve_spawn(
            Some(PersistedPlayerPos {
                map_key: "unknown".to_string(),
                local_x: 1.0,
                local_y: 1.0,
            }),
            DomainEmpire::Red,
        );

        assert!(spawn.used_fallback);
        assert_eq!(spawn.map_id, MapId::new(1));
        assert_eq!(spawn.local_pos.x, 597.0);
        assert_eq!(spawn.local_pos.y, 682.0);
    }

    #[test]
    fn fallback_when_saved_position_out_of_bounds() {
        let coords = ContentCoords::from_catalog(&base_catalog()).expect("coords");
        let spawn = coords.resolve_spawn(
            Some(PersistedPlayerPos {
                map_key: "zohar_map_a1".to_string(),
                local_x: 5000.0,
                local_y: 1.0,
            }),
            DomainEmpire::Blue,
        );

        assert!(spawn.used_fallback);
        assert_eq!(spawn.map_id, MapId::new(41));
        assert_eq!(spawn.local_pos.x, 480.0);
        assert_eq!(spawn.local_pos.y, 736.0);
    }

    #[test]
    fn constructor_fails_when_map_placement_missing() {
        let mut catalog = base_catalog();
        catalog.maps[0].base_x = None;

        let err = ContentCoords::from_catalog(&catalog).expect_err("must fail");
        assert!(
            err.to_string().contains("missing map_placement"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn constructor_fails_when_one_empire_start_is_missing() {
        let mut catalog = base_catalog();
        catalog
            .empire_start_configs
            .retain(|entry| entry.empire != ContentEmpire::Blue);

        let err = ContentCoords::from_catalog(&catalog).expect_err("must fail");
        assert!(
            err.to_string()
                .contains("missing start config for empire Blue"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn constructor_fails_when_required_start_map_was_skipped() {
        let mut catalog = base_catalog();
        catalog.maps[0].map_id = i64::from(u32::MAX) + 1;
        catalog.town_spawns.retain(|spawn| spawn.map_id != 1);

        let err = ContentCoords::from_catalog(&catalog).expect_err("must fail");
        assert!(
            err.to_string().contains("does not exist in maps"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn edge_cm_to_m_roundtrip_preserves_exact_cm() {
        for cm in [-12345, -1, 0, 1, 12345, 1_000_000] {
            let meters = wire_cm_to_meters(cm);
            let back = meters_to_wire_cm(meters);
            assert_eq!(back, cm);
        }
    }

    #[test]
    fn resolves_map_ids_from_exact_map_codes() {
        let coords = ContentCoords::from_catalog(&base_catalog()).expect("coords");

        assert_eq!(coords.map_id_by_code("zohar_map_a1"), Some(MapId::new(1)));
        assert_eq!(coords.map_id_by_code("A1"), None);
        assert_eq!(coords.map_id_by_code("#41"), None);
    }

    #[test]
    fn resolves_world_destination_into_map_local_coordinates() {
        let coords = ContentCoords::from_catalog(&base_catalog()).expect("coords");

        let (map_id, local_pos) = coords
            .resolve_world_destination(WorldPos::new(4096.0 + 12.5, 8960.0 + 34.25))
            .expect("world destination");

        assert_eq!(map_id, MapId::new(1));
        assert_eq!(local_pos, LocalPos::new(12.5, 34.25));
    }

    #[test]
    fn resolve_town_spawn_prefers_empire_specific_spawn() {
        let coords = ContentCoords::from_catalog(&base_catalog()).expect("coords");

        assert_eq!(
            coords.resolve_town_spawn(MapId::new(21), DomainEmpire::Yellow),
            Some(LocalPos::new(560.0, 560.0))
        );
        assert_eq!(
            coords.resolve_town_spawn(MapId::new(21), DomainEmpire::Red),
            Some(LocalPos::new(560.0, 560.0))
        );
    }
}
