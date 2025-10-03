use std::{fmt::Debug, io::{Read, Seek, Write}, ops::{Add, Sub}};

use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::{AnyReadable, ReadDomain, Reader, Writable, WriteCtx, WriteDomain};

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

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerZero32(u32);

impl PointerZero32 {
    pub fn new(value: u32) -> PointerZero32 {
        PointerZero32(value)
    }
    
    pub fn read(reader: &mut impl Read) -> Result<Option<PointerZero32>> {
        let value = reader.read_u32::<LittleEndian>()?;
        
        if value != 0 {
            Ok(Some(PointerZero32(value)))
        } else {
            Ok(None)
        }
    }
    
    pub fn read_relative<R: Read + Seek>(reader: &mut R) -> Result<Option<PointerZero32>> {
        let reader_pos = reader.stream_position()?;
        let value = reader.read_u32::<LittleEndian>()?;
        
        if value != 0 {
            Ok(Some(PointerZero32(value) + reader_pos))
        } else {
            Ok(None)
        }
    }
    
    pub fn write(&self, writer: &mut impl Write) -> Result<()> {
        writer.write_u32::<LittleEndian>(self.0)?;
        Ok(())
    }
    
    pub fn write_option(pointer: Option<Self>, writer: &mut impl Write) -> Result<()> {
        if let Some(pointer) = pointer {
            pointer.write(writer)?;
        }
        Ok(())
    }
    
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl AnyReadable for PointerZero32 {
    fn from_reader_any<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self> {
        Ok(PointerZero32(u32::from_reader_any(reader, domain)?))
    }
}

impl Writable for PointerZero32 {
    fn to_writer(&self, writer: &mut impl WriteCtx, domain: impl WriteDomain) -> Result<()> {
        self.0.to_writer(writer, domain)?;
        Ok(())
    }
}

impl Debug for PointerZero32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Pointer({:#x})", self.0))
    }
}

impl Add<Self> for PointerZero32 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        PointerZero32(self.0 + rhs.0)
    }
}

impl Sub<Self> for PointerZero32 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        PointerZero32(self.0 - rhs.0)
    }
}

impl<R: Reader> From<&mut R> for PointerZero32 {
    fn from(value: &mut R) -> Self {
        // not happy about these unwraps but Rust wouldn't let me implement TryFrom<>
        // because of hypothetical conflicting implementations
        // surprisingly From<> is okay though
        PointerZero32(value.position().unwrap().try_into().unwrap())
    }
}

from_type!(PointerZero32, u32);

from_type_unwrap!(PointerZero32, i32);
from_type_unwrap!(PointerZero32, u64);
from_type_unwrap!(PointerZero32, i64);
from_type_unwrap!(PointerZero32, usize);

into_type!(PointerZero32, u32);
into_type!(PointerZero32, u64);
into_type!(PointerZero32, i64);

into_type_unwrap!(PointerZero32, i32);
into_type_unwrap!(PointerZero32, usize);
