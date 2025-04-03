use std::{cmp::Eq, collections::HashMap, default::Default, hash::Hash, io::{Read, Seek, SeekFrom, Write}, mem, ops::{Deref, DerefMut}, str::from_utf8, sync::atomic::{AtomicU64, Ordering}};

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
// kind of misusing unsafe traits here but this 
// *is* supposed to be generated via proc macro
pub unsafe trait ReadDomain: Copy + EndianSpecific {
    type Pointer;
    
    fn read<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
    
    // "optional" to implement, return Ok(None) if not
    // TODO: implement more of these/make this more generic for all container types
    fn read_std_vec<T, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Option<Vec<T>>>;
    fn read_std_box<T, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Option<Box<T>>>;
    
    // TODO: make these optional to implement? i. e. split them into another Trait
    fn read_box<T, R: Reader>(self, reader: &mut R, parser: impl FnOnce(&mut R, Self) -> Result<T>) -> Result<Option<T>>;
}

pub trait Readable: Sized {
    fn from_reader<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self>;
}

pub trait ReadableWithArgs<T>: Sized {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: T) -> Result<Self>;
}

// writing / serializing
pub unsafe trait WriteDomain: Copy + EndianSpecific {
    type CanonicalWriter: Writer + Default;
    
    // TODO: split these into another trait
    type Pointer;
    type HeapCategory: Eq + Hash + Ord + Default;
    
    fn write<T: 'static>(self, ctx: &mut impl WriteCtx, value: &T) -> Result<Option<()>>;
    
    // TODO: writing with args
    // TODO: boxed serializing
}

pub trait WriteDomainExt: WriteDomain {
    fn new_writer() -> Self::CanonicalWriter {
        Self::CanonicalWriter::default()
    }
    
    fn new_ctx() -> WriteCtxImpl<Self> {
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
    
    fn heap(&self, category: &Self::Category) -> Option<&WriteHeap<Self::Writer>>;
    fn heap_mut(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer>;
    
    // useful for child ctx's
    fn set_heap(&mut self, category: Self::Category, heap: WriteHeap<Self::Writer>);
    fn remove_heap(&mut self, category: &Self::Category) -> WriteHeap<Self::Writer>;
}

pub struct WriteCtxImpl<T: WriteDomain> {
    default_heap: WriteHeap<T::CanonicalWriter>,
    heaps: HashMap<T::HeapCategory, WriteHeap<T::CanonicalWriter>>,
}

impl<T: WriteDomain> WriteCtxImpl<T> {
    pub fn new() -> Self {
        WriteCtxImpl {
            default_heap: WriteHeap::new(),
            heaps: HashMap::new(),
        }
    }
    
    pub fn to_buffer(&self) -> Vec<u8> {
        let writer = T::CanonicalWriter::default();
        
        let mut categories: Vec<&T::HeapCategory> = self.heaps.keys().collect();
        let default_category = T::HeapCategory::default();
        categories.push(&default_category);
        categories.sort();
        
        for category in categories {
            let heap = self.heap(category).unwrap();
        }
        
        Vec::new()
    }
}

impl<T: WriteDomain> WriteCtx for WriteCtxImpl<T> {
    type Category = T::HeapCategory;
    type Writer = T::CanonicalWriter;

    fn heap(&self, category: &Self::Category) -> Option<&WriteHeap<Self::Writer>> {
        if *category == T::HeapCategory::default() {
            Some(&self.default_heap)
        } else {
            self.heaps.get(category)
        }
    }
    
    fn heap_mut(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer> {
        if category == T::HeapCategory::default() {
            &mut self.default_heap
        } else {
            // wow HashMap::entry is such a cool api actually
            self.heaps.entry(category).or_insert_with(WriteHeap::new)
        }
    }
    
    fn set_heap(&mut self, category: Self::Category, heap: WriteHeap<Self::Writer>) {
        if category == T::HeapCategory::default() {
            self.default_heap = heap;
        } else {
            self.heaps.insert(category, heap);
        }
    }
    
    fn remove_heap(&mut self, category: &Self::Category) -> WriteHeap<Self::Writer> {
        if *category == T::HeapCategory::default() {
            mem::take(&mut self.default_heap)
        } else {
            self.heaps.remove(category).unwrap_or_else(|| WriteHeap::new())
        }
    }
}

impl<T: WriteDomain> Deref for WriteCtxImpl<T> {
    type Target = WriteHeap<T::CanonicalWriter>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<T: WriteDomain> DerefMut for WriteCtxImpl<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

pub struct InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    default_category: T::HeapCategory,
    default_heap: WriteHeap<T::CanonicalWriter>,
    ctx: &'a mut C,
}

impl<'a, T, C> InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    pub fn new(ctx: &'a mut C, default_category: T::HeapCategory) -> Self {
        let default_heap = ctx.remove_heap(&default_category);
        
        // SAFETY: 
        Self {
            default_category,
            default_heap,
            ctx,
        }
    }
}

impl<'a, T, C> WriteCtx for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    type Category = T::HeapCategory;
    type Writer = T::CanonicalWriter;

    fn heap(&self, category: &Self::Category) -> Option<&WriteHeap<Self::Writer>> {
        if *category == self.default_category {
            Some(&self.default_heap)
        } else {
            self.ctx.heap(category)
        }
    }
    
    fn heap_mut(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer> {
        if category == self.default_category {
            &mut self.default_heap
        } else {
            self.ctx.heap_mut(category)
        }
    }

    fn set_heap(&mut self, category: Self::Category, heap: WriteHeap<Self::Writer>) {
        if category == self.default_category {
            self.default_heap = heap;
        } else {
            self.ctx.set_heap(category, heap);
        }
    }

    fn remove_heap(&mut self, category: &Self::Category) -> WriteHeap<Self::Writer> {
        if *category == self.default_category {
            mem::take(&mut self.default_heap)
        } else {
            self.ctx.remove_heap(category)
        }
    }
}

impl<'a, T, C> Deref for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    type Target = WriteHeap<T::CanonicalWriter>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<'a, T, C> DerefMut for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

impl<'a, T, C> Drop for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = T::CanonicalWriter, Category = T::HeapCategory>
{
    fn drop(&mut self) {
        let default_category = mem::take(&mut self.default_category);
        let default_heap = mem::take(&mut self.default_heap);
        self.ctx.set_heap(default_category, default_heap);
    }
}

#[derive(Default)]
pub struct WriteHeap<W: Writer> {
    pub writer: W,
    queued_blocks: Vec<(u64, W, Option<Box<dyn Fn(&mut W) -> Result<()>>>)>,
}

static BLOCK_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

impl<W: Writer> WriteHeap<W> {
    pub fn new() -> Self {
        WriteHeap {
            writer: W::default(),
            queued_blocks: Vec::new(),
        }
    }
    
    // TODO: use InnerWriteCtx here
    pub fn enqueue_block(&mut self, content_callback: impl Fn(&mut W) -> Result<()> + 'static) -> Result<u64> {
        let id = BLOCK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.queued_blocks.insert(0, (id, W::default(), Some(Box::new(content_callback))));
        
        Ok(id)
    }
    
    pub fn enqueue_block_end(&mut self, content_callback: impl Fn(&mut W) -> Result<()> + 'static) -> Result<u64> {
        let id = BLOCK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.queued_blocks.push((id, W::default(), Some(Box::new(content_callback))));
        
        Ok(id)
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

