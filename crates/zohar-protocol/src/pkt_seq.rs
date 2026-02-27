use std::fmt;

static SEQ_BYTES_RAW: &[u8] = include_bytes!("pkt_seq_default.bin");

pub struct PacketSequencer {
    seq_bytes: &'static [u8],
    pos: usize,
    last_mismatch: Option<SequenceMismatchError>,
}

impl PacketSequencer {
    pub fn next(&mut self) -> u8 {
        let next_byte = self.seq_bytes[self.pos];
        self.pos += 1;
        if self.pos == self.seq_bytes.len() {
            self.pos = 0;
        }
        next_byte
    }

    /// Get the next expected sequence byte without consuming it.
    pub fn expected(&self) -> u8 {
        self.seq_bytes[self.pos]
    }

    pub fn check(&mut self, got: u8) -> bool {
        let expected = self.next();
        let matches = got == expected;
        self.last_mismatch = (!matches).then(|| SequenceMismatchError { expected, got });
        matches
    }

    pub fn last_mismatch(&self) -> Option<SequenceMismatchError> {
        self.last_mismatch
    }
}

impl Default for PacketSequencer {
    fn default() -> Self {
        Self {
            seq_bytes: SEQ_BYTES_RAW,
            pos: 0,
            last_mismatch: None,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SequenceMismatchError {
    pub expected: u8,
    pub got: u8,
}

impl SequenceMismatchError {
    pub fn from(sequence: &PacketSequencer, got: u8) -> Self {
        sequence
            .last_mismatch
            .unwrap_or(Self { expected: got, got })
    }
}

impl fmt::Display for SequenceMismatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Sequence mismatch: expected 0x{:02X}, got 0x{:02X}",
            self.expected, self.got
        )
    }
}

impl std::error::Error for SequenceMismatchError {}
