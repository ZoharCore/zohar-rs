use binrw::binrw;

use crate::game_pkt;

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub enum TradingC2s {
    #[brw(magic = 0x1B_u8)]
    SubmitExchangeRequest {
        #[br(try_map = |wire: ExchangeRequestWire| wire.try_into())]
        #[bw(map = |request: &PlayerExchangeRequest| ExchangeRequestWire::from(*request))]
        #[brw(pad_size_to = 9)]
        request: PlayerExchangeRequest,
    },
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub enum TradingS2c {
    #[brw(magic = 0x2A_u8)]
    NotifyExchangeEvent {
        #[br(try_map = |wire: ExchangeEventWire| wire.try_into())]
        #[bw(map = |event: &ExchangeEvent| ExchangeEventWire::from(*event))]
        #[brw(pad_size_to = 46)]
        event: ExchangeEvent,
    },
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeRequestKind {
    Initiate = 0,
    AddItem = 1,
    RemoveItem = 2,
    SetGold = 3,
    Accept = 4,
    Cancel = 5,
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeEventKind {
    Initiated = 0,
    AddedItem = 1,
    RemovedItem = 2,
    SetGold = 3,
    Decisioned = 4,
    Ended = 5,
    AlreadyTrading = 6,
    InsufficientGold = 7,
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelativeTransactionParty {
    #[default]
    Remote = 0,
    Local = 1,
}

#[binrw]
#[brw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransactionDecision {
    #[default]
    Rejected = 0,
    Accepted = 1,
}

impl TransactionDecision {
    fn from_arg1(arg1: u32) -> Result<Self, binrw::Error> {
        match arg1 {
            0 => Ok(Self::Rejected),
            1 => Ok(Self::Accepted),
            other => Err(binrw::Error::AssertFail {
                pos: 0,
                message: format!("invalid exchange decision in arg1: {other}"),
            }),
        }
    }

    fn into_arg1(self) -> u32 {
        self as u8 as u32
    }
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub struct ExchangePositionArg {
    pub window: u8,
    pub cell: super::ItemCell,
}

impl Default for ExchangePositionArg {
    fn default() -> Self {
        Self {
            window: 0,
            cell: super::ItemCell::from_raw(0),
        }
    }
}

impl ExchangePositionArg {
    pub fn item_position(self) -> Option<super::ItemPosition> {
        let window = super::WindowKind::try_from(self.window).ok()?;
        Some(super::ItemPosition::new(window, self.cell))
    }

    pub fn display_cell(self) -> super::WireItemCell8 {
        super::WireItemCell8::from_raw(self.cell.raw() as u8)
    }
}

impl From<super::ItemPosition> for ExchangePositionArg {
    fn from(pos: super::ItemPosition) -> Self {
        Self {
            window: pos.window.into(),
            cell: pos.cell,
        }
    }
}

impl From<super::WireItemCell8> for ExchangePositionArg {
    fn from(cell: super::WireItemCell8) -> Self {
        Self {
            window: 0,
            cell: cell.cell(),
        }
    }
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub struct ExchangeRequestWire {
    pub kind: ExchangeRequestKind,
    pub arg1: u32,
    pub display_cell: super::WireItemCell8,
    pub source: ExchangePositionArg,
}

impl Default for ExchangeRequestWire {
    fn default() -> Self {
        Self {
            kind: ExchangeRequestKind::Cancel,
            arg1: 0,
            display_cell: super::WireItemCell8::from_raw(0),
            source: ExchangePositionArg::default(),
        }
    }
}

/// Semantic C2S exchange request.
#[derive(Debug, Clone, Copy)]
pub enum PlayerExchangeRequest {
    Initiate {
        target: game_pkt::NetId,
    },

    AddItem {
        display_cell: super::WireItemCell8,
        source: ExchangePositionArg,
    },

    RemoveItem {
        display_cell: super::WireItemCell8,
    },

    SetGold {
        amount: u32,
    },

    Accept,

    Cancel,
}

impl TryFrom<ExchangeRequestWire> for PlayerExchangeRequest {
    type Error = binrw::Error;

    fn try_from(w: ExchangeRequestWire) -> Result<Self, Self::Error> {
        Ok(match w.kind {
            ExchangeRequestKind::Initiate => Self::Initiate {
                target: game_pkt::NetId::from(w.arg1),
            },

            ExchangeRequestKind::AddItem => Self::AddItem {
                display_cell: w.display_cell,
                source: w.source,
            },

            ExchangeRequestKind::RemoveItem => Self::RemoveItem {
                display_cell: w.display_cell,
            },

            ExchangeRequestKind::SetGold => Self::SetGold { amount: w.arg1 },

            ExchangeRequestKind::Accept => Self::Accept,

            ExchangeRequestKind::Cancel => Self::Cancel,
        })
    }
}

impl From<PlayerExchangeRequest> for ExchangeRequestWire {
    fn from(request: PlayerExchangeRequest) -> Self {
        match request {
            PlayerExchangeRequest::Initiate { target } => Self {
                kind: ExchangeRequestKind::Initiate,
                arg1: target.into(),
                ..Default::default()
            },

            PlayerExchangeRequest::AddItem {
                display_cell,
                source,
            } => Self {
                kind: ExchangeRequestKind::AddItem,
                display_cell,
                source,
                ..Default::default()
            },

            PlayerExchangeRequest::RemoveItem { display_cell } => Self {
                kind: ExchangeRequestKind::RemoveItem,
                display_cell,
                ..Default::default()
            },

            PlayerExchangeRequest::SetGold { amount } => Self {
                kind: ExchangeRequestKind::SetGold,
                arg1: amount,
                ..Default::default()
            },

            PlayerExchangeRequest::Accept => Self {
                kind: ExchangeRequestKind::Accept,
                ..Default::default()
            },

            PlayerExchangeRequest::Cancel => Self {
                kind: ExchangeRequestKind::Cancel,
                ..Default::default()
            },
        }
    }
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy)]
pub struct ExchangeEventWire {
    pub kind: ExchangeEventKind,
    pub who: RelativeTransactionParty,
    pub arg1: u32,
    pub arg2: ExchangePositionArg,
    pub arg3: u32,
    pub details: super::ItemInstanceDetails,
}

impl Default for ExchangeEventWire {
    fn default() -> Self {
        Self {
            kind: ExchangeEventKind::Ended,
            who: RelativeTransactionParty::default(),
            arg1: 0,
            arg2: ExchangePositionArg::default(),
            arg3: 0,
            details: super::ItemInstanceDetails::default(),
        }
    }
}

/// Semantic S2C exchange event.
#[derive(Debug, Clone, Copy)]
pub enum ExchangeEvent {
    Initiated {
        who: RelativeTransactionParty,
        partner: game_pkt::NetId,
    },

    AddedItem {
        who: RelativeTransactionParty,
        item_id: super::ItemTemplateId,
        display_cell: ExchangePositionArg,
        qty: u32,
        details: super::ItemInstanceDetails,
    },

    RemovedItem {
        who: RelativeTransactionParty,
        display_cell: super::WireItemCell8,
    },

    SetGold {
        who: RelativeTransactionParty,
        amount: u32,
    },

    Decisioned {
        who: RelativeTransactionParty,
        decision: TransactionDecision,
    },

    Ended,

    AlreadyTrading,

    InsufficientGold,
}

impl TryFrom<ExchangeEventWire> for ExchangeEvent {
    type Error = binrw::Error;

    fn try_from(raw: ExchangeEventWire) -> Result<Self, Self::Error> {
        Ok(match raw.kind {
            ExchangeEventKind::Initiated => Self::Initiated {
                who: raw.who,
                partner: game_pkt::NetId::from(raw.arg1),
            },

            ExchangeEventKind::AddedItem => Self::AddedItem {
                who: raw.who,
                item_id: super::ItemTemplateId::from(raw.arg1),
                display_cell: raw.arg2,
                qty: raw.arg3,
                details: raw.details,
            },

            ExchangeEventKind::RemovedItem => Self::RemovedItem {
                who: raw.who,
                display_cell: super::WireItemCell8::from_raw(raw.arg1 as u8),
            },

            ExchangeEventKind::SetGold => Self::SetGold {
                who: raw.who,
                amount: raw.arg1,
            },

            ExchangeEventKind::Decisioned => Self::Decisioned {
                who: raw.who,
                decision: TransactionDecision::from_arg1(raw.arg1)?,
            },

            ExchangeEventKind::Ended => Self::Ended,

            ExchangeEventKind::AlreadyTrading => Self::AlreadyTrading,

            ExchangeEventKind::InsufficientGold => Self::InsufficientGold,
        })
    }
}

impl From<ExchangeEvent> for ExchangeEventWire {
    fn from(event: ExchangeEvent) -> Self {
        match event {
            ExchangeEvent::Initiated { who, partner } => Self {
                kind: ExchangeEventKind::Initiated,
                who,
                arg1: partner.into(),
                ..Default::default()
            },

            ExchangeEvent::AddedItem {
                who,
                item_id,
                display_cell,
                qty,
                details,
            } => Self {
                kind: ExchangeEventKind::AddedItem,
                who,
                arg1: item_id.into(),
                arg2: display_cell,
                arg3: qty,
                details,
            },

            ExchangeEvent::RemovedItem { who, display_cell } => Self {
                kind: ExchangeEventKind::RemovedItem,
                who,
                arg1: display_cell.cell().raw() as u32,
                ..Default::default()
            },

            ExchangeEvent::SetGold { who, amount } => Self {
                kind: ExchangeEventKind::SetGold,
                who,
                arg1: amount,
                ..Default::default()
            },

            ExchangeEvent::Decisioned { who, decision } => Self {
                kind: ExchangeEventKind::Decisioned,
                who,
                arg1: decision.into_arg1(),
                ..Default::default()
            },

            ExchangeEvent::Ended => Self {
                kind: ExchangeEventKind::Ended,
                ..Default::default()
            },

            ExchangeEvent::AlreadyTrading => Self {
                kind: ExchangeEventKind::AlreadyTrading,
                ..Default::default()
            },

            ExchangeEvent::InsufficientGold => Self {
                kind: ExchangeEventKind::InsufficientGold,
                ..Default::default()
            },
        }
    }
}
