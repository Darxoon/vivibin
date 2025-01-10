use std::io::{Seek, SeekFrom};

use anyhow::Result;

pub struct SeekGuard<'a, R: Seek> {
    pub seek: &'a mut R,
    start_pos: u64,
}

impl<'a, R: Seek> SeekGuard<'a, R> {
    pub fn new(reader: &'a mut R) -> Result<Self> {
        let start_pos = reader.stream_position()?;
        
        Ok(Self {
            seek: reader,
            start_pos,
        })
    }
}

impl<'a, R: Seek> Drop for SeekGuard<'a, R> {
    fn drop(&mut self) {
        self.seek.seek(SeekFrom::Start(self.start_pos)).unwrap();
    }
}


#[macro_export]
macro_rules! scoped_reader_pos {
    ($reader:ident) => {
        let guard = ::vivibin::util::SeekGuard::new($reader)?;
        let $reader = &mut *guard.seek;
    };
}

#[macro_export]
macro_rules! scoped_writer_pos {
    ($writer:ident) => {
        let guard = ::vivibin::util::SeekGuard::new($writer)?;
        let $writer = &mut *guard.seek;
    };
}

