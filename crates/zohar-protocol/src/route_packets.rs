#[macro_export]
macro_rules! route_packets {
    (
        $(#[$outer:meta])*
        pub enum $EnumName:ident {
            $(
                $Variant:ident($SubEnum:ty) from $($opcode:pat_param)|+
            ),* $(,)?
        }
    ) => {
        $(#[$outer])*
        #[derive(Debug, Clone)]
        pub enum $EnumName {
            $(
                $Variant($SubEnum),
            )*
        }

        impl binrw::BinRead for $EnumName {
            type Args<'a> = ();

            fn read_options<R: std::io::Read + std::io::Seek>(
                reader: &mut R,
                endian: binrw::Endian,
                args: Self::Args<'_>,
            ) -> binrw::BinResult<Self> {
                use std::io::SeekFrom;

                let start_pos = reader.stream_position()?;
                let opcode = <u8 as binrw::BinRead>::read_options(reader, endian, ())?;
                reader.seek(SeekFrom::Start(start_pos))?;

                match opcode {
                    $(
                        $($opcode)|+ => {
                            let packet = <$SubEnum as binrw::BinRead>::read_options(
                                reader,
                                endian,
                                args,
                            )?;
                            Ok(Self::$Variant(packet))
                        }
                    )*
                    _ => Err(binrw::Error::AssertFail {
                        pos: start_pos,
                        message: format!(
                            "Unknown opcode: {:#02X} for {}",
                            opcode,
                            stringify!($EnumName)
                        )
                        .into(),
                    }),
                }
            }
        }

        impl binrw::BinWrite for $EnumName {
            type Args<'a> = ();

            fn write_options<W: std::io::Write + std::io::Seek>(
                &self,
                writer: &mut W,
                endian: binrw::Endian,
                args: Self::Args<'_>,
            ) -> binrw::BinResult<()> {
                match self {
                    $(
                        Self::$Variant(packet) => {
                            binrw::BinWrite::write_options(packet, writer, endian, args)
                        }
                    )*
                }
            }
        }

        $(
            impl From<$SubEnum> for $EnumName {
                fn from(v: $SubEnum) -> Self {
                    Self::$Variant(v)
                }
            }
        )*
    };
}
