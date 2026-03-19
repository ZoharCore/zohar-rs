use bevy::prelude::*;
use zohar_domain::coords::LocalPos;

use crate::api::{ClientIntent, PlayerEvent};
use crate::bridge::{ClientIntentMsg, InboundEvent};

use super::players::{handle_player_enter, handle_player_leave, player_entities_on_map};
use super::state::{
    AttackIntent, AttackIntentQueue, ChatIntent, ChatIntentQueue, MAX_ATTACK_INTENTS_PER_TICK,
    MAX_CHAT_INTENTS_PER_TICK, MAX_MOVE_INTENTS_PER_TICK, MoveIntent, MoveIntentQueue,
    PlayerAppearanceComp, PlayerIndex, PlayerOutboxComp, RuntimeState,
};
use super::util::{format_global_shout, next_entity_id};

pub(super) fn drain_inbound(world: &mut World) {
    let mut events = Vec::new();
    {
        let bridge = world.resource::<super::state::NetworkBridgeRx>();
        while let Ok(event) = bridge.inbound_rx.try_recv() {
            events.push(event);
        }
    }

    if events.is_empty() {
        return;
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
            InboundEvent::PlayerEnter { msg } => handle_player_enter(world, msg),
            InboundEvent::PlayerLeave { msg } => handle_player_leave(world, msg),
            InboundEvent::ClientIntent { msg } => handle_client_intent(world, msg),
            InboundEvent::GlobalShout { msg } => handle_global_shout(world, msg),
        }
    }
}

fn handle_client_intent(world: &mut World, msg: ClientIntentMsg) {
    let Some(player_entity) = world
        .resource::<PlayerIndex>()
        .0
        .get(&msg.player_id)
        .copied()
    else {
        return;
    };

    match msg.intent {
        ClientIntent::Move {
            entity_id: _,
            kind,
            arg,
            rot,
            x,
            y,
            ts,
        } => {
            if let Some(mut queue) = world.entity_mut(player_entity).get_mut::<MoveIntentQueue>() {
                push_move_intent(
                    &mut queue.0,
                    MoveIntent {
                        kind,
                        arg,
                        rot,
                        target: LocalPos::new(x, y),
                        ts,
                    },
                );
            }
        }
        ClientIntent::Chat { message } => {
            if let Some(mut queue) = world.entity_mut(player_entity).get_mut::<ChatIntentQueue>() {
                queue.0.push(ChatIntent { message });
                if queue.0.len() > MAX_CHAT_INTENTS_PER_TICK {
                    let overflow = queue.0.len() - MAX_CHAT_INTENTS_PER_TICK;
                    queue.0.drain(0..overflow);
                }
            }
        }
        ClientIntent::Attack {
            target,
            attack_type,
        } => {
            if let Some(mut queue) = world
                .entity_mut(player_entity)
                .get_mut::<AttackIntentQueue>()
            {
                queue.0.push(AttackIntent {
                    target,
                    attack_type,
                });
                if queue.0.len() > MAX_ATTACK_INTENTS_PER_TICK {
                    let overflow = queue.0.len() - MAX_ATTACK_INTENTS_PER_TICK;
                    queue.0.drain(0..overflow);
                }
            }
        }
    }
}

fn push_move_intent(queue: &mut Vec<MoveIntent>, intent: MoveIntent) {
    if queue
        .last()
        .is_some_and(|last| move_intents_match(last, &intent))
    {
        return;
    }

    queue.push(intent);

    if queue.len() > MAX_MOVE_INTENTS_PER_TICK {
        let overflow = queue.len() - MAX_MOVE_INTENTS_PER_TICK;
        queue.drain(0..overflow);
    }
}

fn move_intents_match(lhs: &MoveIntent, rhs: &MoveIntent) -> bool {
    lhs.kind == rhs.kind
        && lhs.arg == rhs.arg
        && lhs.rot == rhs.rot
        && lhs.target == rhs.target
        && lhs.ts == rhs.ts
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::entity::MovementKind;

    fn intent(kind: MovementKind, ts: u32, x: f32, y: f32) -> MoveIntent {
        MoveIntent {
            kind,
            arg: 0,
            rot: 0,
            target: LocalPos::new(x, y),
            ts,
        }
    }

    #[test]
    fn move_intents_preserve_order() {
        let mut queue = Vec::new();
        push_move_intent(&mut queue, intent(MovementKind::Move, 100, 1.0, 1.0));
        push_move_intent(&mut queue, intent(MovementKind::Move, 120, 2.0, 2.0));

        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].kind, MovementKind::Move);
        assert_eq!(queue[0].ts, 100);
        assert_eq!(queue[1].kind, MovementKind::Move);
        assert_eq!(queue[1].ts, 120);
        assert_eq!(queue[1].target, LocalPos::new(2.0, 2.0));
    }

    #[test]
    fn exact_duplicate_move_intents_are_deduplicated() {
        let mut queue = Vec::new();
        push_move_intent(&mut queue, intent(MovementKind::Move, 100, 1.0, 1.0));
        push_move_intent(&mut queue, intent(MovementKind::Move, 100, 1.0, 1.0));

        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].ts, 100);
    }

    #[test]
    fn wait_intents_preserve_order() {
        let mut queue = Vec::new();
        push_move_intent(&mut queue, intent(MovementKind::Wait, 100, 1.0, 1.0));
        push_move_intent(&mut queue, intent(MovementKind::Wait, 120, 2.0, 2.0));

        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].kind, MovementKind::Wait);
        assert_eq!(queue[1].kind, MovementKind::Wait);
        assert_eq!(queue[1].ts, 120);
    }

    #[test]
    fn move_intent_overflow_keeps_latest_suffix_in_order() {
        let mut queue = Vec::new();
        for idx in 0..(MAX_MOVE_INTENTS_PER_TICK as u32 + 2) {
            push_move_intent(
                &mut queue,
                intent(MovementKind::Move, 100 + idx, idx as f32, 0.0),
            );
        }

        assert_eq!(queue.len(), MAX_MOVE_INTENTS_PER_TICK);
        assert_eq!(queue[0].ts, 102);
        assert_eq!(
            queue.last().expect("latest").ts,
            101 + MAX_MOVE_INTENTS_PER_TICK as u32
        );
    }
}

fn handle_global_shout(world: &mut World, msg: crate::bridge::GlobalShoutMsg) {
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
                kind: 6,
                sender_entity_id: None,
                empire: Some(msg.from_empire),
                message: payload.clone(),
            });
        }
    }
}
