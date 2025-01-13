use std::{cmp::Eq, collections::HashMap, default::Default, hash::Hash, io::{Read, Seek, SeekFrom, Write}, ops::{Deref, DerefMut}, str::from_utf8, sync::atomic::{AtomicU64, Ordering}};

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

pub trait Writer: Write + Seek + Default {
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

impl<T: Write + Seek + Default> Writer for T {}

pub enum Endianness {
    Little,
    Big,
    // Native?
}

pub trait EndianSpecific {
    fn endianness(self) -> Endianness;
}

// reading / parsing
pub trait ReadDomain: Copy + EndianSpecific {
    type Pointer;
    
    // TODO: consider making an error type for 'no read implementation' rather than using Options (see silly return types below for why)
    fn read<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
    fn read_args<T: 'static, U>(self, reader: &mut impl Reader, args: U) -> Result<Option<T>>;
    
    // TODO: make these optional to implement? i. e. split them into another Trait
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

// writing / serializing
pub trait WriteDomain: Copy + EndianSpecific {
    type CanonicalWriter: Writer + Default;
    
    // TODO: split these into another trait
    type Pointer;
    type HeapCategory: Eq + Hash + Default;
    
    fn write<T: 'static>(self, ctx: &mut impl WriteCtx, value: &T) -> Result<Option<()>>;
    
    // TODO: writing with args
    // TODO: boxed serializing
}

pub trait WriteDomainExt: WriteDomain {
    fn new_writer() -> Self::CanonicalWriter {
        Self::CanonicalWriter::default()
    }
    
    fn new_ctx() -> WriteCtxImpl<Self::CanonicalWriter, Self::HeapCategory> {
        WriteCtxImpl::new()
    }
}

impl<T: WriteDomain> WriteDomainExt for T {}

pub trait Writable: Sized {
    fn to_writer(&self, ctx: &mut impl WriteCtx, domain: impl WriteDomain) -> Result<()>;
}

// boxed serialization stuff
pub trait WriteCtx: Deref<Target = WriteHeap<Self::Writer>> + DerefMut {
    type Category: Eq + Hash + Default;
    type Writer: Writer;
    
    fn get_heap(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer>;
}

pub struct WriteCtxImpl<W: Writer, C: Eq + Hash + Default = ()> {
    default_heap: WriteHeap<W>,
    heaps: HashMap<C, WriteHeap<W>>,
}

impl<W: Writer, C: Eq + Hash + Default> WriteCtxImpl<W, C> {
    pub fn new() -> Self {
        WriteCtxImpl {
            default_heap: WriteHeap::new(),
            heaps: HashMap::new(),
        }
    }
}

impl<W: Writer, C: Eq + Hash + Default> WriteCtx for WriteCtxImpl<W, C> {
    type Category = C;
    type Writer = W;

    fn get_heap(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer> {
        if category == C::default() {
            &mut self.default_heap
        } else {
            // wow HashMap::entry is such a cool api actually
            self.heaps.entry(category).or_insert_with(WriteHeap::new)
        }
    }
}

impl<W: Writer, C: Eq + Hash + Default> Deref for WriteCtxImpl<W, C> {
    type Target = WriteHeap<W>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<W: Writer, C: Eq + Hash + Default> DerefMut for WriteCtxImpl<W, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

pub struct WriteHeap<W: Writer> {
    pub writer: W,
    queued_blocks: Vec<(u64, W)>,
}

static BLOCK_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

impl<W: Writer> WriteHeap<W> {
    pub fn new() -> Self {
        WriteHeap {
            writer: W::default(),
            queued_blocks: Vec::new(),
        }
    }
    
    pub fn enqueue_block_start(&mut self) -> (u64, &mut W) {
        let id = BLOCK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.queued_blocks.insert(0, (id, W::default()));
        let (_, writer) = self.queued_blocks.first_mut().unwrap();
        (id, writer)
    }
    
    pub fn enqueue_block_end(&mut self) -> (u64, &mut W) {
        let id = BLOCK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.queued_blocks.push((id, W::default()));
        let (_, writer) = self.queued_blocks.last_mut().unwrap();
        (id, writer)
    }
}

impl<W: Writer> Deref for WriteHeap<W> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.writer
    }
}

impl<W: Writer> DerefMut for WriteHeap<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.writer
    }
}

