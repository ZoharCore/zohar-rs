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

#[cfg(test)]
mod tests {
    use super::BehaviorFlags;

    #[test]
    fn can_wander_is_disabled_by_no_move() {
        let flags = BehaviorFlags::NO_MOVE | BehaviorFlags::AGGRESSIVE;
        assert!(!flags.can_wander());
        assert!(flags.contains(BehaviorFlags::AGGRESSIVE));
    }
}
