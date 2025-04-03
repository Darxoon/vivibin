use std::{fmt::Debug, io::{Read, Seek, Write}, num::NonZeroU32, ops::{Add, Sub}};

use anyhow::{Error, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::{ReadDomain, Readable, Reader, Writable, WriteCtx, WriteDomain};

macro_rules! from_type {
    ($t:ident, $from:ty) => {
        impl From<$from> for $t {
            fn from(value: $from) -> Self {
                PointerZero32(value.into())
            }
        }
        
        impl Add<$from> for $t {
            type Output = Self;
        
            fn add(self, rhs: $from) -> Self {
                $t(self.0 + u32::from(rhs))
            }
        }
        
        impl Sub<$from> for $t {
            type Output = Self;
        
            fn sub(self, rhs: $from) -> Self {
                $t(self.0 - u32::from(rhs))
            }
        }
    };
}

macro_rules! from_type_unwrap {
    ($t:ident, $from:ty) => {
        impl From<$from> for $t {
            fn from(value: $from) -> Self {
                PointerZero32(value.try_into().unwrap())
            }
        }
        
        impl Add<$from> for $t {
            type Output = Self;
        
            fn add(self, rhs: $from) -> Self {
                // it's beautiful
                $t((i32::try_from(self.0).unwrap() + i32::try_from(rhs).unwrap()).try_into().unwrap())
            }
        }
        
        impl Sub<$from> for $t {
            type Output = Self;
        
            fn sub(self, rhs: $from) -> Self {
                $t((i32::try_from(self.0).unwrap() - i32::try_from(rhs).unwrap()).try_into().unwrap())
            }
        }
    };
}

macro_rules! into_type {
    ($t:ident, $into:ty) => {
        impl From<$t> for $into {
            fn from(value: $t) -> Self {
                value.0.into()
            }
        }
    };
}

macro_rules! into_type_unwrap {
    ($t:ident, $into:ty) => {
        impl From<$t> for $into {
            fn from(value: $t) -> Self {
                value.0.try_into().unwrap()
            }
        }
    };
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerNonZero32(NonZeroU32);

impl PointerNonZero32 {
    pub fn read(reader: &mut impl Read) -> Result<Option<PointerNonZero32>> {
        let value = reader.read_u32::<LittleEndian>()?;
        
        if let Some(value) = NonZeroU32::new(value) {
            Ok(Some(PointerNonZero32(value)))
        } else {
            Ok(None)
        }
    }
    
    // TODO: aaa
    // pub fn read_relative<R: Read + Seek>(reader: &mut R) -> Result<Option<Pointer32>> {
    //     let reader_pos = reader.stream_position()?;
    //     let value = reader.read_u32::<LittleEndian>()?;
        
    //     if let Some(value) = NonZeroU32::new(value) {
    //         Ok(Some(Pointer32(value) + reader_pos))
    //     } else {
    //         Ok(None)
    //     }
    // }
    
    pub fn write(&self, writer: &mut impl Write) -> Result<()> {
        writer.write_u32::<LittleEndian>(self.0.into())?;
        Ok(())
    }
    
    pub fn write_option(pointer: Option<Self>, writer: &mut impl Write) -> Result<()> {
        if let Some(pointer) = pointer {
            pointer.write(writer)?;
        }
        Ok(())
    }
    
    pub fn value(&self) -> u32 {
        self.0.into()
    }
    
    pub fn value_non_zero(&self) -> NonZeroU32 {
        self.0
    }
}

impl Readable for PointerNonZero32 {
    fn from_reader<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self> {
        let value = u32::from_reader(reader, domain)?;
        Ok(PointerNonZero32(NonZeroU32::new(value).ok_or(Error::msg("Tried to cast 0 into PointerNonZero32"))?))
    }
}

impl Writable for PointerNonZero32 {
    fn to_writer(&self, ctx: &mut impl WriteCtx, domain: impl WriteDomain) -> Result<()> {
        self.0.get().to_writer(ctx, domain)?;
        Ok(())
    }
}

// TODO: how do I allow user defined types to do the same
// this is only possible becasue Readable and Writable are defined in the same crate
impl Readable for Option<PointerNonZero32> {
    fn from_reader<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self> {
        let value = u32::from_reader(reader, domain)?;
        Ok(NonZeroU32::new(value).map(|value| PointerNonZero32(value)))
    }
}

impl Writable for Option<PointerNonZero32> {
    fn to_writer(&self, ctx: &mut impl WriteCtx, domain: impl WriteDomain) -> Result<()> {
        let value: u32 = self.map(|x| x.0.get()).unwrap_or(0);
        value.to_writer(ctx, domain)?;
        Ok(())
    }
}

impl Debug for PointerNonZero32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Pointer({:#x})", self.0))
    }
}

// impl Add<Self> for Pointer32 {
//     type Output = Self;

//     fn add(self, rhs: Self) -> Self {
//         Pointer32(self.0 + rhs.0)
//     }
// }

// impl Sub<Self> for Pointer32 {
//     type Output = Self;

//     fn sub(self, rhs: Self) -> Self {
//         Pointer32(self.0 - rhs.0)
//     }
// }

impl<R: Reader> From<&mut R> for PointerNonZero32 {
    fn from(value: &mut R) -> Self {
        // not happy about these unwraps but Rust wouldn't let me implement TryFrom<>
        // because of hypothetical conflicting implementations
        // surprisingly From<> is okay though
        let value = value.position().unwrap().try_into().unwrap();
        PointerNonZero32(NonZeroU32::new(value).unwrap())
    }
}

// TODO: aaaa
// from_type!(Pointer32, u32);

// from_type_unwrap!(Pointer32, i32);
// from_type_unwrap!(Pointer32, u64);
// from_type_unwrap!(Pointer32, i64);
// from_type_unwrap!(Pointer32, usize);

// into_type!(Pointer32, u32);
// into_type!(Pointer32, u64);
// into_type!(Pointer32, i64);

// into_type_unwrap!(Pointer32, i32);
// into_type_unwrap!(Pointer32, usize);
