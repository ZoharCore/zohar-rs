use crate::adapters::content::{
    build_entity_motion_speeds, build_map_navigators, build_mob_chat_content, build_mob_proto,
    build_spawn_rules,
};
use crate::app::CoreRuntimeConfig;
use anyhow::anyhow;
use std::sync::Arc;
use zohar_content::ContentRuntimeBuilder;
use zohar_gamesrv::ContentCoords;
use zohar_sim::{MapConfig, MapInstanceKey, SharedConfig, WanderConfig};

pub(crate) struct LoadedContent {
    pub(crate) coords: Arc<ContentCoords>,
    pub(crate) map_key: MapInstanceKey,
    pub(crate) shared_config: SharedConfig,
    pub(crate) map_config: MapConfig,
}

pub(crate) fn load_content(
    config: &CoreRuntimeConfig,
    runtime: &tokio::runtime::Runtime,
) -> anyhow::Result<LoadedContent> {
    let content_runtime = runtime.block_on(async {
        ContentRuntimeBuilder::new()
            .db_path(config.content_db.clone())
            .run_read_only()
            .await
    })?;
    let catalog = content_runtime.catalog();
    let coords = Arc::new(ContentCoords::from_catalog(catalog)?);

    let map_id = require_map_id(coords.map_id_by_code(&config.map), &config.map)?;
    let map_key = MapInstanceKey::shared(config.channel, map_id);

    let entity_motion_speeds = Arc::new(build_entity_motion_speeds(catalog));
    let all_spawn_rules = build_spawn_rules(catalog);
    let all_mobs = Arc::new(build_mob_proto(catalog));
    let mob_chat = Arc::new(build_mob_chat_content(catalog));
    let all_navigators = build_map_navigators(catalog);
    let all_empires = coords.map_empires_by_id();

    let shared_config = SharedConfig {
        motion_speeds: entity_motion_speeds,
        mobs: all_mobs,
        wander: WanderConfig::default(),
        mob_chat,
    };
    let map_config = MapConfig {
        map_key,
        empire: all_empires.get(&map_id).copied().flatten(),
        local_size: coords
            .map_local_size(map_id)
            .ok_or_else(|| anyhow!("missing local bounds for map '{}'", config.map))?,
        navigator: all_navigators.get(&map_id).cloned(),
        spawn_rules: all_spawn_rules.get(&map_id).cloned().unwrap_or_default(),
    };

    Ok(LoadedContent {
        coords,
        map_key,
        shared_config,
        map_config,
    })
}

fn require_map_id(
    map_id: Option<zohar_domain::MapId>,
    map_code: &str,
) -> anyhow::Result<zohar_domain::MapId> {
    map_id.ok_or_else(|| anyhow!("unknown map code '{map_code}'"))
}

#[cfg(test)]
mod tests {
    use super::require_map_id;

    #[test]
    fn unknown_map_code_returns_error() {
        let err = require_map_id(None, "missing_map").expect_err("must fail");
        assert!(
            err.to_string().contains("unknown map code 'missing_map'"),
            "unexpected error: {err:#}"
        );
    }
}
