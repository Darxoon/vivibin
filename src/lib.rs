use std::{io::{Read, Seek, SeekFrom, Write}, process::Output};

use anyhow::Result;

pub mod default_impls;
pub mod util;

pub trait Reader: Read + Seek {
    fn position(&mut self) -> Result<u64> {
        Ok(self.stream_position()?)
    }
    
    fn set_position(&mut self, position: impl Into<u64>) -> Result<()> {
        self.seek(SeekFrom::Start(position.into()))?;
        Ok(())
    }
}

impl<T: Read + Seek> Reader for T {}

pub trait Writer: Write + Seek {
    fn position(&mut self) -> Result<u64> {
        Ok(self.stream_position()?)
    }
    
    fn set_position(&mut self, position: impl Into<u64>) -> Result<()> {
        self.seek(SeekFrom::Start(position.into()))?;
        Ok(())
    }
}

impl<T: Write + Seek> Writer for T {}

pub enum Endianness {
    Little,
    Big,
    // Native?
}

pub trait ReadDomain: Copy {
    type Flags;
    type Pointer;
    
    fn endianness(self) -> Endianness;
    fn read<T: Clone + 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
}

pub trait Readable: Sized {
    fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self>;
}

