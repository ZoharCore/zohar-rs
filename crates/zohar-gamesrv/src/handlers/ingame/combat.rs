use super::super::types::PhaseResult;
use super::{InGameCtx, InGamePhaseEffects};
use crate::adapters::{ToDomain, ToProtocol};
use tracing::{debug, warn};
use zohar_domain::entity::EntityId;
use zohar_domain::util::FlagsMapper;
use zohar_map_port::{
    AttackIntent as PortAttackIntent, AttackTargetIntent, ClientIntent, ClientIntentMsg,
    DamageInfoFlags, ProjectileEffectKind, SpecialEffectType, TargetIntent,
};
use zohar_protocol::game_pkt::ZeroOpt;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::combat::{
    CombatC2s, CombatS2c, FloatingDamageFlags, ProjectileKind, SpecialEffectKind,
};

pub(super) async fn handle_packet(
    packet: CombatC2s,
    state: &mut InGameCtx<'_>,
) -> PhaseResult<InGamePhaseEffects> {
    match packet {
        CombatC2s::InputAttack {
            attack_type,
            target,
            _unknown,
        } => {
            let attack = match attack_type {
                ZeroOpt(None) => PortAttackIntent::Basic,
                ZeroOpt(Some(skill)) => PortAttackIntent::Skill(skill.to_domain()),
            };

            let intent_msg = ClientIntentMsg {
                player_id: state.player_id,
                intent: ClientIntent::Attack(AttackTargetIntent {
                    target: target.to_domain(),
                    attack,
                }),
            };
            if let Err(err) = state.ctx.map_events.try_send_client_intent(intent_msg) {
                warn!(
                    player_id = ?state.player_id,
                    map_id = %state.map_id,
                    target = u32::from(target),
                    error = ?err,
                    "Failed to enqueue attack intent to map runtime"
                );
            }
            Ok(InGamePhaseEffects::empty())
        }
        CombatC2s::SignalTargetSwitch { target } => {
            let intent_msg = ClientIntentMsg {
                player_id: state.player_id,
                intent: ClientIntent::Target(TargetIntent {
                    target: target.to_domain(),
                }),
            };
            if let Err(err) = state.ctx.map_events.try_send_client_intent(intent_msg) {
                warn!(
                    player_id = ?state.player_id,
                    map_id = %state.map_id,
                    target = u32::from(target),
                    error = ?err,
                    "Failed to enqueue target selection intent to map runtime"
                );
            }
            debug!(
                player_id = ?state.player_id,
                map_id = %state.map_id,
                target = u32::from(target),
                "Received client target selection"
            );
            Ok(InGamePhaseEffects::empty())
        }
    }
}

pub(super) fn encode_entity_health_bar(entity_id: EntityId, hp_pct: u8) -> Vec<InGameS2c> {
    vec![
        CombatS2c::SyncEntityHealthBar {
            target: entity_id.to_protocol(),
            hp_pct: hp_pct.min(100),
        }
        .into(),
    ]
}

pub(super) fn encode_damage_info(
    entity_id: EntityId,
    flags: DamageInfoFlags,
    damage: i32,
) -> Vec<InGameS2c> {
    vec![
        CombatS2c::TriggerFloatingDamage {
            target: entity_id.to_protocol(),
            flags: encode_floating_damage_flags(flags),
            damage,
        }
        .into(),
    ]
}

pub(super) fn encode_entity_stunned(entity_id: EntityId) -> Vec<InGameS2c> {
    vec![
        CombatS2c::SetEntityStunned {
            target: entity_id.to_protocol(),
        }
        .into(),
    ]
}

pub(super) fn encode_entity_dead(entity_id: EntityId) -> Vec<InGameS2c> {
    vec![
        CombatS2c::SetEntityDead {
            target: entity_id.to_protocol(),
        }
        .into(),
    ]
}

pub(super) fn encode_projectile(
    effect: ProjectileEffectKind,
    start_entity_id: EntityId,
    end_entity_id: EntityId,
) -> Vec<InGameS2c> {
    vec![
        CombatS2c::TriggerProjectileEffect {
            kind: encode_projectile_kind(effect),
            from_entity: start_entity_id.to_protocol(),
            to_entity: end_entity_id.to_protocol(),
        }
        .into(),
    ]
}

pub(super) fn encode_special_effect(
    effect: SpecialEffectType,
    entity_id: EntityId,
) -> Vec<InGameS2c> {
    vec![
        CombatS2c::TriggerSpecialEffect {
            kind: encode_special_effect_kind(effect),
            target: entity_id.to_protocol(),
        }
        .into(),
    ]
}

const fn encode_projectile_kind(effect: ProjectileEffectKind) -> ProjectileKind {
    match effect {
        ProjectileEffectKind::Exp => ProjectileKind::Experience,
    }
}

const fn encode_special_effect_kind(effect: SpecialEffectType) -> SpecialEffectKind {
    match effect {
        SpecialEffectType::Critical => SpecialEffectKind::CriticalStrike,
        SpecialEffectType::Penetrate => SpecialEffectKind::PiercingStrike,
    }
}

fn encode_floating_damage_flags(flags: DamageInfoFlags) -> FloatingDamageFlags {
    const MAPPER: FlagsMapper<DamageInfoFlags, FloatingDamageFlags> = FlagsMapper::new(&[
        (DamageInfoFlags::NORMAL, FloatingDamageFlags::NORMAL),
        (DamageInfoFlags::POISON, FloatingDamageFlags::POISON),
        (DamageInfoFlags::DODGE, FloatingDamageFlags::DODGE),
        (DamageInfoFlags::BLOCK, FloatingDamageFlags::BLOCK),
        (DamageInfoFlags::PENETRATE, FloatingDamageFlags::PENETRATE),
        (DamageInfoFlags::CRITICAL, FloatingDamageFlags::CRITICAL),
    ]);

    MAPPER.map(flags)
}
