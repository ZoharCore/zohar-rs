use binrw::{BinWrite, Endian};
use std::io::Cursor;

pub fn encoded_bytes<T>(pkt: &T) -> Vec<u8>
where
    for<'a> T: BinWrite<Args<'a> = ()>,
{
    let mut cursor = Cursor::new(Vec::new());
    BinWrite::write_options(pkt, &mut cursor, Endian::Little, ()).unwrap();
    cursor.into_inner()
}

pub fn assert_packet_frame<T>(pkt: &T, expected_opcode: u8, expected_len: usize)
where
    for<'a> T: BinWrite<Args<'a> = ()>,
{
    let raw = encoded_bytes(pkt);
    assert_eq!(raw[0], expected_opcode);
    assert_eq!(raw.len(), expected_len);
}
