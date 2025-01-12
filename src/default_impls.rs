use anyhow::Result;

use crate::{Endianness, ReadDomain, Readable, ReadableWithArgs, Reader};

// numbers
macro_rules! impl_rw_number {
    ($type:ident, $byte_size:expr) => {
        impl Readable for $type {
            fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self> {
                let mut buf = [0; $byte_size];
                reader.read_exact(&mut buf)?;
                let result = match domain.endianness() {
                    Endianness::Little => $type::from_le_bytes(buf),
                    Endianness::Big => $type::from_be_bytes(buf),
                };
                Ok(result)
            }
        }
    };
}

impl_rw_number!(u8, 1);
impl_rw_number!(u16, 2);
impl_rw_number!(u32, 4);
impl_rw_number!(u64, 8);

impl_rw_number!(i8, 1);
impl_rw_number!(i16, 2);
impl_rw_number!(i32, 4);
impl_rw_number!(i64, 8);

impl_rw_number!(f32, 4);
impl_rw_number!(f64, 8);

// booleans
pub enum BoolSize {
    U8,
    U16,
    U32,
    U64,
}

impl Readable for bool {
    fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self> {
        Self::from_reader_args(reader, domain, BoolSize::U32)
    }
}

impl ReadableWithArgs<BoolSize> for bool {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: BoolSize) -> Result<Self> {
        Ok(match args {
            BoolSize::U8 => u8::from_reader(reader, domain)? != 0,
            BoolSize::U16 => u16::from_reader(reader, domain)? != 0,
            BoolSize::U32 => u32::from_reader(reader, domain)? != 0,
            BoolSize::U64 => u64::from_reader(reader, domain)? != 0,
        })
    }
}


