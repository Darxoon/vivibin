use anyhow::Result;

use crate::{impl_writable_from_simple, AnyReadable, Endianness, HeapCategory, ReadDomain, ReadableWithArgs, Reader, SimpleWritable, WriteDomain, Writer};

impl HeapCategory for () {}

// numbers
macro_rules! impl_rw_number {
    ($type:ident, $byte_size:expr) => {
        impl AnyReadable for $type {
            fn from_reader_any<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self> {
                let mut buf = [0; $byte_size];
                reader.read_exact(&mut buf)?;
                let result = match domain.endianness() {
                    Endianness::Little => $type::from_le_bytes(buf),
                    Endianness::Big => $type::from_be_bytes(buf),
                };
                Ok(result)
            }
        }
        
        impl<D: WriteDomain> SimpleWritable<D> for $type {
            fn to_writer_simple(&self, ctx: &mut impl Writer, domain: &mut D) -> Result<()> {
                let bytes = match domain.endianness() {
                    Endianness::Little => self.to_le_bytes(),
                    Endianness::Big => self.to_be_bytes(),
                };
                
                ctx.write_all(&bytes)?;
                Ok(())
            }
        }
        
        impl_writable_from_simple!($type);
    };
}

macro_rules! impl_traits_number {
    ($type:ident, $byte_size:expr) => {
        impl_rw_number!($type, $byte_size);
        
        impl HeapCategory for $type {}
    };
}

impl_traits_number!(u8, 1);
impl_traits_number!(u16, 2);
impl_traits_number!(u32, 4);
impl_traits_number!(u64, 8);

impl_traits_number!(i8, 1);
impl_traits_number!(i16, 2);
impl_traits_number!(i32, 4);
impl_traits_number!(i64, 8);

impl_rw_number!(f32, 4);
impl_rw_number!(f64, 8);

// booleans
pub enum BoolSize {
    U8,
    U16,
    U32,
    U64,
}

impl AnyReadable for bool {
    fn from_reader_any<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self> {
        Self::from_reader_args(reader, domain, BoolSize::U32)
    }
}

impl ReadableWithArgs<BoolSize> for bool {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: BoolSize) -> Result<Self> {
        Ok(match args {
            BoolSize::U8 => u8::from_reader_any(reader, domain)? != 0,
            BoolSize::U16 => u16::from_reader_any(reader, domain)? != 0,
            BoolSize::U32 => u32::from_reader_any(reader, domain)? != 0,
            BoolSize::U64 => u64::from_reader_any(reader, domain)? != 0,
        })
    }
}

// TODO: allow specifying size
impl<D: WriteDomain> SimpleWritable<D> for bool {
    fn to_writer_simple(&self, ctx: &mut impl Writer, domain: &mut D) -> Result<()> {
        (*self as u32).to_writer_simple(ctx, domain)?;
        Ok(())
    }
}

impl_writable_from_simple!(bool);

