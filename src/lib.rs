use std::{
    cmp::Eq,
    collections::HashMap,
    default::Default,
    hash::Hash,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    mem,
    ops::{Deref, DerefMut},
    str::from_utf8,
};

use anyhow::Result;
use array_init::try_array_init;

pub mod default_impls;
pub mod pointers;
pub mod util;

pub use vivibin_derive::*;

const ZEROES: &[u8] = &[0; 128];

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
    
    fn read_unk<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
    
    // "optional" to implement, return Ok(None) if not
    // TODO: implement more of these/make this more generic for all container types
    fn read_std_box_of<T, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Option<Box<T>>>;
    
    // TODO: make these optional to implement? i. e. split them into another Trait
    fn read_box<T, R: Reader>(self, reader: &mut R, parser: impl FnOnce(&mut R, Self) -> Result<T>) -> Result<Option<T>>;
}

pub trait ReadDomainExt: ReadDomain {
    fn read_fallback<T: Readable<Self> + 'static>(self, reader: &mut impl Reader) -> Result<T> {
        Ok(match self.read_unk::<T>(reader)? {
            Some(x) => x,
            None => T::from_reader(reader, self)?,
        })
    }
    
    fn read_std_box_fallback<T: Readable<Self> + 'static, R: Reader>(self, reader: &mut R) -> Result<Option<Box<T>>> {
        self.read_std_box_of(reader, |reader| self.read_fallback::<T>(reader))
    }
    
    fn read_unk_array<T, R: Reader, const N: usize>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<[T; N]> {
        try_array_init(|_| read_content(reader))
    }
    
    fn read_array<T: Readable<Self>, R: Reader, const N: usize>(self, reader: &mut R) -> Result<[T; N]> {
        try_array_init(|_| T::from_reader(reader, self))
    }
}

impl<T: ReadDomain> ReadDomainExt for T {}

// TODO: make this more generic across more container types?
pub trait CanReadVec: ReadDomain {
    fn read_std_vec_of<T: 'static, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Vec<T>>;
}

pub trait ReadVecFallbackExt: CanReadVec {
    fn read_std_vec_fallback<T: Readable<Self> + 'static, R: Reader>(self, reader: &mut R) -> Result<Vec<T>> {
        self.read_std_vec_of(reader, |reader| self.read_fallback::<T>(reader))
    }
}

impl<D: CanReadVec> ReadVecFallbackExt for D {}

pub trait ReadVecExt: CanReadVec {
    fn read_std_vec<T: 'static, R: Reader>(self, reader: &mut R) -> Result<Vec<T>>
    where
        Self: CanRead<T>
    {
        self.read_std_vec_of(reader, |reader| self.read(reader))
    }
}

impl<D: CanReadVec> ReadVecExt for D {}

pub trait CanRead<T: 'static>: ReadDomain {
    fn read(self, reader: &mut impl Reader) -> Result<T>;
}

pub trait Readable<D: ReadDomain>: Sized {
    fn from_reader<R: Reader>(reader: &mut R, domain: D) -> Result<Self>;
}

pub trait ReadableWithArgs<T>: Sized {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: T) -> Result<Self>;
}

// Convenience trait for manual impls of types that are readable by all domains
pub trait AnyReadable: Sized {
    fn from_reader_any<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self>;
}

impl<A: AnyReadable, D: ReadDomain> Readable<D> for A {
    fn from_reader<R: Reader>(reader: &mut R, domain: D) -> Result<Self> {
        A::from_reader_any(reader, domain)
    }
}

// writing / serializing
pub trait HeapCategory: Eq + Hash + Ord + Default + Clone {}

pub trait WriteDomain: Copy + EndianSpecific {
    // TODO: split these into another trait
    type Pointer;
    type Cat: HeapCategory;
    
    fn write_unk<T: 'static>(self, ctx: &mut impl WriteCtx, value: &T) -> Result<Option<()>>;
    fn apply_reference(self, writer: &mut impl Writer, heap_offset: usize) -> Result<()>;
    
    // TODO: writing with args
    // TODO: boxed serializing
}

pub trait WriteDomainExt: WriteDomain {
    fn new_ctx() -> WriteCtxImpl<Self> {
        WriteCtxImpl::new()
    }
    
    fn write_fallback<T: Writable<Self> + 'static>(self, ctx: &mut impl WriteCtx, value: &T) -> Result<()> {
        if self.write_unk::<T>(ctx, &value)?.is_none() {
            value.to_writer(ctx, self)?;
        }
        Ok(())
    }
}

impl<T: WriteDomain> WriteDomainExt for T {}

pub trait CanWrite<T: 'static + ?Sized>: WriteDomain  {
    fn write(self, ctx: &mut impl WriteCtx, value: &T) -> Result<()>;
}

pub trait CanWriteWithArgs<T: 'static, A: Default>: CanWrite<T> {
    fn write_args(self, ctx: &mut impl WriteCtx, value: &T, args: A) -> Result<()>;
}

pub trait Writable<D: WriteDomain>: Sized {
    fn to_writer(&self, ctx: &mut impl WriteCtx, domain: D) -> Result<()>;
}

pub trait SimpleWritable<D: WriteDomain>: Sized {
    fn to_writer_simple(&self, writer: &mut impl Writer, domain: D) -> Result<()>;
}

#[macro_export]
macro_rules! impl_writable_from_simple {
    ($type:ty) => {
        impl<D: $crate::WriteDomain> $crate::Writable<D> for $type {
            fn to_writer(&self, ctx: &mut impl $crate::WriteCtx, domain: D) -> Result<()> {
                self.to_writer_simple(ctx.cur_writer(), domain)
            }
        }
    };
}

// boxed serialization stuff
pub trait WriteCtx: Deref<Target = WriteHeap<Self::Writer>> + DerefMut {
    type Category: Eq + Hash + Default;
    type Writer: Writer;
    
    fn allocate_next_block(&mut self, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken>;
    fn allocate_next_block_aligned(&mut self, alignment: usize, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken>;
    
    fn heap(&self, category: &Self::Category) -> Option<&WriteHeap<Self::Writer>>;
    fn heap_mut(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer>;
    
    // useful for child ctx's
    fn set_heap(&mut self, category: Self::Category, heap: WriteHeap<Self::Writer>);
    fn remove_heap(&mut self, category: &Self::Category) -> WriteHeap<Self::Writer>;
}

pub type WriteCtxWriter = Cursor<Vec<u8>>;

pub struct WriteCtxImpl<T: WriteDomain> {
    default_heap: WriteHeap<WriteCtxWriter>,
    heaps: HashMap<T::Cat, WriteHeap<WriteCtxWriter>>,
}

impl<D: WriteDomain> WriteCtxImpl<D> {
    pub fn new() -> Self {
        WriteCtxImpl {
            default_heap: WriteHeap::new(),
            heaps: HashMap::new(),
        }
    }
    
    pub fn to_buffer(mut self, domain: D, mut block_offsets: Option<&mut Vec<usize>>) -> Result<Vec<u8>> {
        let mut writer = WriteCtxWriter::default();
        
        self.heaps.insert(D::Cat::default(), self.default_heap);
        
        let mut heaps = self.heaps.iter().collect::<Vec<_>>();
        heaps.sort_by_key(|(category, _)| *category);
        
        let mut all_relocations: Vec<(usize, HeapToken)> = Vec::new();
        
        for (_, heap) in heaps {
            all_relocations.clear();
            
            for (block_id, block) in heap.blocks.iter().enumerate() {
                let block_start = writer.position() as usize;
                if let Some(block_offsets) = block_offsets.as_mut() {
                    block_offsets.push(block_start);
                }
                writer.write_all(block.writer.get_ref())?;
                
                // apply previous relocations
                let all_relocations_to_current: _ = all_relocations.extract_if(.., |(_, token)|
                    token.block_id as usize == block_id);
                
                for (offset, token) in all_relocations_to_current {
                    let writer_pos = writer.position();
                    writer.set_position(offset as u64);
                    
                    domain.apply_reference(&mut writer, block_start + token.offset as usize)?;
                    
                    writer.set_position(writer_pos);
                }
                
                // push new relocations
                all_relocations.extend(block.relocations.iter().copied()
                    .map(|(local_offset, token)| (block_start + local_offset, token)));
            }
        }
        
        Ok(writer.into_inner())
    }
}

impl<T: WriteDomain> WriteCtx for WriteCtxImpl<T> {
    type Category = T::Cat;
    type Writer = WriteCtxWriter;

    fn allocate_next_block(&mut self, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken> {
        let prev_current_block = self.default_heap.current_block;
        let new_block_token = self.default_heap.seek_to_new_block(0)?;
        
        content_callback(self)?;
        
        self.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    fn allocate_next_block_aligned(&mut self, alignment: usize, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken> {
        let prev_current_block = self.default_heap.current_block;
        let new_block_token = self.default_heap.seek_to_new_block(alignment)?;
        
        content_callback(self)?;
        
        self.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    // I'm honestly not sure anymore where these are useful
    fn heap(&self, category: &Self::Category) -> Option<&WriteHeap<Self::Writer>> {
        if *category == T::Cat::default() {
            Some(&self.default_heap)
        } else {
            self.heaps.get(category)
        }
    }
    
    fn heap_mut(&mut self, category: Self::Category) -> &mut WriteHeap<Self::Writer> {
        if category == T::Cat::default() {
            &mut self.default_heap
        } else {
            // wow HashMap::entry is such a cool api actually
            self.heaps.entry(category).or_insert_with(WriteHeap::new)
        }
    }
    
    fn set_heap(&mut self, category: Self::Category, heap: WriteHeap<Self::Writer>) {
        if category == T::Cat::default() {
            self.default_heap = heap;
        } else {
            self.heaps.insert(category, heap);
        }
    }
    
    fn remove_heap(&mut self, category: &Self::Category) -> WriteHeap<Self::Writer> {
        if *category == T::Cat::default() {
            mem::take(&mut self.default_heap)
        } else {
            self.heaps
                .remove(category)
                .unwrap_or_else(|| WriteHeap::new())
        }
    }
}

impl<T: WriteDomain> Deref for WriteCtxImpl<T> {
    type Target = WriteHeap<WriteCtxWriter>;

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
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    default_category: T::Cat,
    default_heap: WriteHeap<WriteCtxWriter>,
    ctx: &'a mut C,
}

impl<'a, T, C> InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    pub fn new(ctx: &'a mut C, default_category: T::Cat) -> Self {
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
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    type Category = T::Cat;
    type Writer = WriteCtxWriter;

    fn allocate_next_block(&mut self, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken> {
        let prev_current_block = self.default_heap.current_block;
        let new_block_token = self.default_heap.seek_to_new_block(0)?;
        
        content_callback(self)?;
        
        self.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    fn allocate_next_block_aligned(&mut self, alignment: usize, content_callback: impl FnOnce(&mut Self) -> Result<()>) -> Result<HeapToken> {
        let prev_current_block = self.default_heap.current_block;
        let new_block_token = self.default_heap.seek_to_new_block(alignment)?;
        
        content_callback(self)?;
        
        self.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
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
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    type Target = WriteHeap<WriteCtxWriter>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<'a, T, C> DerefMut for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

impl<'a, T, C> Drop for InnerWriteCtx<'a, T, C>
where
    T: WriteDomain,
    C: WriteCtx<Writer = WriteCtxWriter, Category = T::Cat>,
{
    fn drop(&mut self) {
        let default_category = mem::take(&mut self.default_category);
        let default_heap = mem::take(&mut self.default_heap);
        self.ctx.set_heap(default_category, default_heap);
    }
}

pub fn align_to(writer: &mut impl Writer, alignment: usize) -> Result<()> {
    if alignment == 0 {
        return Ok(());
    }
    
    let alignment = alignment as isize;
    let pos = writer.position()? as isize;
    
    // bonkers alignment calculation
    let padding_size = ((alignment - pos) % alignment + alignment) % alignment;
    
    writer.write(&ZEROES[..padding_size as usize])?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, Hash)]
pub struct HeapToken {
    block_id: u32,
    offset: u32,
}

impl HeapToken {
    pub fn resolve(self, block_offsets: &[usize]) -> usize {
        block_offsets[self.block_id as usize] + self.offset as usize
    }
}

impl PartialOrd for HeapToken {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.block_id.partial_cmp(&other.block_id) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        self.offset.partial_cmp(&other.offset)
    }
}

#[derive(Clone, Debug)]
pub struct HeapBlock<W: Writer> {
    relocations: Vec<(usize, HeapToken)>,
    writer: W,
}

impl<W: Writer> HeapBlock<W> {
    pub fn new() -> Self {
        HeapBlock {
            relocations: Vec::new(),
            writer: W::default(),
        }
    }
}

#[derive(Default)]
pub struct WriteHeap<W: Writer> {
    current_block: usize,
    blocks: Vec<HeapBlock<W>>,
}

impl<W: Writer> WriteHeap<W> {
    pub fn new() -> Self {
        WriteHeap {
            current_block: 0,
            blocks: vec![HeapBlock::new()],
        }
    }
    
    pub fn cur_writer(&mut self) -> &mut W {
        &mut self.blocks[self.current_block].writer
    }
    
    pub fn write_token<const BYTE_SIZE: usize>(&mut self, token: HeapToken) -> Result<()> {
        let block = &mut self.blocks[self.current_block];
        block.relocations.push((block.writer.position()? as usize, token));
        
        self.cur_writer().write_all(&const { [0; BYTE_SIZE] })?;
        Ok(())
    }
    
    pub fn align_to(&mut self, alignment: usize) -> Result<()> {
        align_to(self.cur_writer(), alignment)
    }
    
    pub fn heap_token_at_current_pos(&mut self) -> Result<HeapToken> {
        Ok(HeapToken {
            block_id: self.current_block as u32,
            offset: self.cur_writer().position()? as u32,
        })
    }
    
    fn seek_to_new_block(&mut self, alignment: usize) -> Result<HeapToken> {
        if self.current_block == self.blocks.len() - 1 {
            // allocate new block
            self.current_block = self.blocks.len();
            self.blocks.push(HeapBlock::new());
            // TODO: add alignment to HeapBlock
        } else {
            self.current_block += 1;
            self.align_to(alignment)?;
        }
        
        self.heap_token_at_current_pos()
    }
}

impl<W: Writer> Deref for WriteHeap<W> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        // I don't really see the point of creating an explicit named function for this
        // unlike cur_writer/deref_mut
        // because what do you want to do with an immutable writer
        &self.blocks[self.current_block].writer
    }
}

impl<W: Writer> DerefMut for WriteHeap<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.cur_writer()
    }
}
