use std::{io::{Read, Seek, SeekFrom, Write}, str::from_utf8};

use anyhow::Result;

pub mod default_impls;
pub mod pointers;
pub mod util;

pub trait Reader: Read + Seek {
    fn position(&mut self) -> Result<u64> {
        Ok(self.stream_position()?)
    }
    
    fn set_position(&mut self, position: impl Into<u64>) -> Result<()> {
        self.seek(SeekFrom::Start(position.into()))?;
        Ok(())
    }
    
    fn read_str(&mut self, size: usize) -> Result<String> {
        let mut bytes = Vec::new();
        
        for _ in 0..size {
            // TODO: consider using domain read method here
            let mut b: [u8; 1] = [0; 1];
            self.read_exact(&mut b)?;
            
            bytes.push(b[0]);
        }
        
        Ok(from_utf8(&bytes)?.to_owned())
    }
    
    fn read_c_str(&mut self) -> Result<String> {
        let mut bytes = Vec::new();
        
        loop {
            // TODO: consider using domain read method here
            let mut b: [u8; 1] = [0; 1];
            self.read_exact(&mut b)?;
            
            if b[0] == 0 {
                break;
            }
            
            bytes.push(b[0]);
        }
        
        Ok(from_utf8(&bytes)?.to_owned())
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
    
    fn write_str(&mut self, string: &str) -> Result<()> {
        self.write(string.as_bytes())?;
        Ok(())
    }
    
    fn write_c_str(&mut self, string: &str) -> Result<()> {
        self.write_str(string)?;
        self.write(&[0])?;
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
    // TODO: consider making an error type for 'no read implementation' rather than using Options (see silly return types below for why)
    fn read<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
    fn read_args<T: 'static, U>(self, reader: &mut impl Reader, args: U) -> Result<Option<T>>;
    
    // TODO: make these optional to implement?
    fn read_box<T, R: Reader>(self, reader: &mut R, parser: impl FnOnce(&mut R, Self) -> Result<T>) -> Result<Option<T>>;
    
    fn read_boxed<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<Option<T>>>;
    fn read_boxed_args<T: 'static, U>(self, reader: &mut impl Reader, args: U) -> Result<Option<Option<T>>>;
}

pub trait Readable: Sized {
    fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self>;
}

pub trait ReadableWithArgs<T>: Sized {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: T) -> Result<Self>;
}

