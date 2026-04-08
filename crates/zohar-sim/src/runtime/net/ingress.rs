use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;
use zohar_map_port::{ChatChannel, ClientIntent, ClientIntentMsg, GlobalShoutMsg, PlayerEvent};

use crate::bridge::InboundEvent;

use super::super::player::persistence::leave_player_and_snapshot;
use super::players::{handle_player_enter, handle_player_leave, player_entities_on_map};
use super::state::{
    ChatIntent, ChatIntentQueue, MAX_ATTACK_INTENTS_PER_TICK, MAX_CHAT_INTENTS_PER_TICK,
    MAX_MOVE_INTENTS_PER_TICK, PlayerAppearanceComp, PlayerCommand, PlayerCommandQueue,
    PlayerIndex, PlayerOutboxComp, RuntimeState,
};
use super::util::{format_global_shout, next_entity_id};

pub(crate) fn drain_inbound(world: &mut World) {
    let mut events = Vec::new();
    {
        let bridge = world.resource::<super::state::NetworkBridgeRx>();
        while let Ok(event) = bridge.inbound_rx.try_recv() {
            events.push(event);
        }
    }

    for event in events {
        match event {
            InboundEvent::ReserveNetId { reply } => {
                let net_id = {
                    let mut state = world.resource_mut::<RuntimeState>();
                    next_entity_id(&mut state)
                };
                let _ = reply.send(net_id);
            }
            InboundEvent::PlayerEnter { msg, outbox } => handle_player_enter(world, msg, outbox),
            InboundEvent::PlayerLeave { msg } => handle_player_leave(world, msg),
            InboundEvent::PlayerLeaveAndSnapshot { msg, reply } => {
                let _ = reply.send(leave_player_and_snapshot(world, msg));
            }
            InboundEvent::ClientIntent { msg } => {
                handle_client_intent(DeferredWorld::from(&mut *world), msg)
            }
            InboundEvent::GlobalShout { msg } => handle_global_shout(world, msg),
        }
    }
}

pub(crate) fn handle_client_intent(mut world: DeferredWorld, msg: ClientIntentMsg) {
    let Some(player_entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    else {
        return;
    };

    match msg.intent {
        ClientIntent::Move(intent) => {
            if let Some(mut queue) = world
                .entity_mut(player_entity)
                .get_mut::<PlayerCommandQueue>()
            {
                push_player_command(
                    &mut queue.0,
                    PlayerCommand::Move {
                        kind: intent.kind,
                        arg: intent.arg,
                        rot: intent.facing,
                        target: intent.target,
                        ts: intent.client_ts,
                    },
                );
            }
        }
        ClientIntent::SetMovementAnimation(animation) => {
            if let Some(mut queue) = world
                .entity_mut(player_entity)
                .get_mut::<PlayerCommandQueue>()
            {
                push_player_command(&mut queue.0, PlayerCommand::SetMovementAnimation(animation));
            }
        }
        ClientIntent::Chat(intent) => {
            if let Some(mut queue) = world.entity_mut(player_entity).get_mut::<ChatIntentQueue>() {
                queue.0.push(ChatIntent {
                    // TODO: only broadcast local speaking packets
                    channel: intent.channel,
                    message: intent.message,
                });
                if queue.0.len() > MAX_CHAT_INTENTS_PER_TICK {
                    let overflow = queue.0.len() - MAX_CHAT_INTENTS_PER_TICK;
                    queue.0.drain(0..overflow);
                }
            }
        }
        ClientIntent::Attack(intent) => {
            if let Some(mut queue) = world
                .entity_mut(player_entity)
                .get_mut::<PlayerCommandQueue>()
            {
                push_player_command(
                    &mut queue.0,
                    PlayerCommand::Attack {
                        target: intent.target,
                        attack: intent.attack,
                    },
                );
            }
        }
    }
}

fn push_player_command(queue: &mut Vec<PlayerCommand>, command: PlayerCommand) {
    if matches!(command, PlayerCommand::Move { .. })
        && queue
            .last()
            .is_some_and(|last| player_move_commands_match(last, &command))
    {
        return;
    }
    if matches!(command, PlayerCommand::SetMovementAnimation(_))
        && queue
            .last()
            .is_some_and(|last| player_animation_commands_match(last, &command))
    {
        return;
    }

    queue.push(command);
    match command {
        PlayerCommand::Move { .. } => trim_player_commands(queue, MAX_MOVE_INTENTS_PER_TICK, true),
        PlayerCommand::Attack { .. } => {
            trim_player_commands(queue, MAX_ATTACK_INTENTS_PER_TICK, false)
        }
        PlayerCommand::SetMovementAnimation(_) => {}
    }
}

fn trim_player_commands(queue: &mut Vec<PlayerCommand>, max_len: usize, keep_moves: bool) {
    let mut matching = queue
        .iter()
        .filter(|command| matches!(command, PlayerCommand::Move { .. }) == keep_moves)
        .count();
    if matching <= max_len {
        return;
    }

    queue.retain(|command| {
        let is_matching = matches!(command, PlayerCommand::Move { .. }) == keep_moves;
        if !is_matching {
            return true;
        }
        if matching > max_len {
            matching -= 1;
            false
        } else {
            true
        }
    });
}

fn player_move_commands_match(lhs: &PlayerCommand, rhs: &PlayerCommand) -> bool {
    match (lhs, rhs) {
        (
            PlayerCommand::Move {
                kind: lhs_kind,
                arg: lhs_arg,
                rot: lhs_rot,
                target: lhs_target,
                ts: lhs_ts,
            },
            PlayerCommand::Move {
                kind: rhs_kind,
                arg: rhs_arg,
                rot: rhs_rot,
                target: rhs_target,
                ts: rhs_ts,
            },
        ) => {
            lhs_kind == rhs_kind
                && lhs_arg == rhs_arg
                && lhs_rot == rhs_rot
                && lhs_target == rhs_target
                && lhs_ts == rhs_ts
        }
        _ => false,
    }
}

fn player_animation_commands_match(lhs: &PlayerCommand, rhs: &PlayerCommand) -> bool {
    matches!(
        (lhs, rhs),
        (
            PlayerCommand::SetMovementAnimation(lhs_animation),
            PlayerCommand::SetMovementAnimation(rhs_animation),
        ) if lhs_animation == rhs_animation
    )
}

fn handle_global_shout(world: &mut World, msg: GlobalShoutMsg) {
    let payload = format_global_shout(&msg.from_player_name, &msg.message_bytes).into_bytes();
    let player_entities = player_entities_on_map(world);

    for entity in player_entities {
        let Some(appearance) = world
            .entity(entity)
            .get::<PlayerAppearanceComp>()
            .map(|a| a.0.clone())
        else {
            continue;
        };

        if appearance.empire != msg.from_empire {
            continue;
        }

        if let Some(mut outbox) = world.entity_mut(entity).get_mut::<PlayerOutboxComp>() {
            outbox.0.push_reliable(PlayerEvent::Chat {
                channel: ChatChannel::Shout,
                sender_entity_id: None,
                empire: Some(msg.from_empire),
                message: payload.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::coords::LocalPos;
    use zohar_domain::entity::EntityId;
    use zohar_domain::entity::MovementKind;
    use zohar_map_port::{AttackIntent, ClientTimestamp, Facing72, MovementArg};

    fn move_command(kind: MovementKind, ts: u32, x: f32, y: f32) -> PlayerCommand {
        PlayerCommand::Move {
            kind,
            arg: MovementArg::ZERO,
            rot: Facing72::from_wrapped(0),
            target: LocalPos::new(x, y),
            ts: ClientTimestamp::new(ts),
        }
    }

    #[test]
    fn move_command_overflow_keeps_latest_suffix_in_order() {
        let mut queue = Vec::new();
        for idx in 0..(MAX_MOVE_INTENTS_PER_TICK as u32 + 2) {
            push_player_command(
                &mut queue,
                move_command(MovementKind::Move, 100 + idx, idx as f32, 0.0),
            );
        }

        assert_eq!(
            queue
                .iter()
                .filter(|command| matches!(command, PlayerCommand::Move { .. }))
                .count(),
            MAX_MOVE_INTENTS_PER_TICK
        );
        assert!(matches!(
            queue[0],
            PlayerCommand::Move { ts, .. } if ts == ClientTimestamp::new(102)
        ));
        assert!(matches!(
            queue.last().expect("latest"),
            PlayerCommand::Move {
                ts,
                ..
            } if *ts == ClientTimestamp::new(101 + MAX_MOVE_INTENTS_PER_TICK as u32)
        ));
    }

    #[test]
    fn attack_command_overflow_only_trims_attack_backlog() {
        let mut queue = vec![move_command(MovementKind::Move, 100, 1.0, 1.0)];
        for idx in 0..(MAX_ATTACK_INTENTS_PER_TICK as u32 + 2) {
            push_player_command(
                &mut queue,
                PlayerCommand::Attack {
                    target: EntityId(idx + 1),
                    attack: AttackIntent::Skill(
                        zohar_domain::entity::player::skill::SkillId::ThreeWayCut,
                    ),
                },
            );
        }

        assert!(matches!(
            queue[0],
            PlayerCommand::Move { ts, .. } if ts == ClientTimestamp::new(100)
        ));
        assert_eq!(
            queue
                .iter()
                .filter(|command| matches!(command, PlayerCommand::Attack { .. }))
                .count(),
            MAX_ATTACK_INTENTS_PER_TICK
        );
    }
}
