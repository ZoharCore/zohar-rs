bitflags::bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BehaviorFlags: u32 {
        const NO_MOVE = 1 << 0;
        const AGGRESSIVE = 1 << 1;
    }
}

impl BehaviorFlags {
    pub fn can_wander(self) -> bool {
        !self.contains(Self::NO_MOVE)
    }
}
