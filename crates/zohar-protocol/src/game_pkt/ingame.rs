pub mod chat;
pub mod fishing;
pub mod guild;
pub mod movement;
pub mod system;
pub mod trading;
pub mod world;

use crate::control_pkt::{ControlC2s, ControlS2c};

crate::route_packets! {
    /// Client-to-server packets for in-game phase.
    pub enum InGameC2s {
        Control(ControlC2s) from 0xFE | 0xFF | 0xFC,
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
        Move(movement::MovementS2c) from 0x03,
        Chat(chat::ChatS2c) from 0x04,
        System(system::SystemS2c) from 0x6A | 0x79,
        World(world::WorldS2c) from 0x01 | 0x88,
        Trading(trading::TradingS2c) from 0x51,
        Guild(guild::GuildS2c) from 0x61,
        Fishing(fishing::FishingS2c) from 0x71,
    }
}
