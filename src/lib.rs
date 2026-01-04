extern crate alloc;

use core::{
    cmp::{Eq, Ordering},
    default::Default,
    hash::Hash,
    mem,
    ops::{Deref, DerefMut},
    str::from_utf8,
};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use anyhow::{anyhow, Result};
use array_init::try_array_init;
use indexmap::IndexMap;

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
        self.write_all(string.as_bytes())?;
        Ok(())
    }
    
    fn write_c_str(&mut self, string: &str) -> Result<()> {
        self.write_str(string)?;
        self.write_all(&[0])?;
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
    fn endianness(&self) -> Endianness;
}

// reading / parsing
pub trait ReadDomain: Copy + EndianSpecific {
    type Pointer;
    
    fn read_unk<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>>;
    
    // TODO: make this optional to implement? i. e. split them into another Trait
    fn read_box_nullable<T, R: Reader>(self, reader: &mut R, read_content: impl FnOnce(&mut R) -> Result<T>) -> Result<Option<T>>;
}

pub trait ReadDomainExt: ReadDomain {
    fn read_fallback<T: Readable<Self> + 'static>(self, reader: &mut impl Reader) -> Result<T> {
        Ok(match self.read_unk::<T>(reader)? {
            Some(x) => x,
            None => T::from_reader(reader, self)?,
        })
    }
    
    fn read_box<T, R: Reader>(self, reader: &mut R, read_content: impl FnOnce(&mut R) -> Result<T>) -> Result<T> {
        let offset = reader.position()?;
        let value = self.read_box_nullable(reader, read_content)?
            .ok_or_else(|| anyhow!("Expected value, got null (at offset 0x{offset:x})"))?;
        Ok(value)
    }
    
    fn read_std_box_of<T, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Box<T>> {
        self.read_box(reader, read_content).map(Box::new)
    }
    
    fn read_std_box_fallback<T: Readable<Self> + 'static, R: Reader>(self, reader: &mut R) -> Result<Box<T>> {
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
    fn from_reader_unboxed<R: Reader>(reader: &mut R, domain: D) -> Result<Self>;
    
    /// Override this with a read_box if this type should be boxed by default
    fn from_reader<R: Reader>(reader: &mut R, domain: D) -> Result<Self> {
        Self::from_reader_unboxed(reader, domain)
    }
}

pub trait ReadableWithArgs<T>: Sized {
    fn from_reader_args(reader: &mut impl Reader, domain: impl ReadDomain, args: T) -> Result<Self>;
}

// Convenience trait for manual impls of types that are readable by all domains
pub trait AnyReadable: Sized {
    fn from_reader_any<R: Reader>(reader: &mut R, domain: impl ReadDomain) -> Result<Self>;
}

impl<A: AnyReadable, D: ReadDomain> Readable<D> for A {
    fn from_reader_unboxed<R: Reader>(reader: &mut R, domain: D) -> Result<Self> {
        A::from_reader_any(reader, domain)
    }
}

// writing / serializing
pub trait HeapCategory: Eq + Hash + Ord + Default + Clone {}

// TODO: does this have to be sized?
pub trait WriteDomain: Sized + EndianSpecific {
    // TODO: split these into another trait
    type Pointer;
    type Cat: HeapCategory;
    
    fn write_unk<T: 'static>(&mut self, ctx: &mut impl WriteCtx, value: &T) -> Result<Option<()>>;
    fn apply_reference(&mut self, writer: &mut impl Writer, heap_offset: usize) -> Result<()>;
    
    // TODO: writing with args
    // TODO: boxed serializing
}

pub trait WriteDomainExt: WriteDomain {
    fn new_ctx() -> WriteCtxImpl<Self::Cat> {
        WriteCtxImpl::new()
    }
    
    fn write_fallback<T: Writable<Self> + 'static>(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, value: &T) -> Result<()> {
        if self.write_unk::<T>(ctx, value)?.is_none() {
            value.to_writer(ctx, self)?;
        }
        Ok(())
    }
}

impl<T: WriteDomain> WriteDomainExt for T {}

pub trait CanWriteBox: WriteDomain {
    fn write_box_of<W: WriteCtx>(
        &mut self,
        ctx: &mut W,
        write_content: impl FnOnce(&mut Self, &mut W::InnerCtx<'_>) -> Result<()>,
    ) -> Result<()>;
}

pub trait WriteBoxFallbackExt: CanWriteBox {
    fn write_box_fallback<T: Writable<Self> + 'static>(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, value: &T) -> Result<()> {
        self.write_box_of(ctx, |domain, ctx| {
            domain.write_fallback::<T>(ctx, value)
        })
    }
}

impl<D: CanWriteBox> WriteBoxFallbackExt for D {}

pub trait WriteBoxExt: CanWriteBox {
    fn write_box<T: 'static>(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, value: &T) -> Result<()>
    where
        Self: CanWrite<T>
    {
        self.write_box_of(ctx, |domain, ctx| {
            domain.write(ctx, value)
        })
    }
}

impl<D: CanWriteBox> WriteBoxExt for D {}

pub trait CanWriteSlice: WriteDomain {
    fn write_slice_of<T: 'static, W: WriteCtx<Cat = Self::Cat>>(
        &mut self,
        ctx: &mut W,
        values: &[T],
        write_content: impl Fn(&mut Self, &mut W::InnerCtx<'_>, &T) -> Result<()>,
    ) -> Result<()>;
}

pub trait WriteSliceFallbackExt: CanWriteSlice {
    fn write_slice_fallback<T: Writable<Self> + 'static>(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, values: &[T]) -> Result<()> {
        self.write_slice_of(ctx, values, |domain, ctx, value| {
            domain.write_fallback::<T>(ctx, value)
        })
    }
}

impl<D: CanWriteSlice> WriteSliceFallbackExt for D {}

pub trait WriteSliceExt: CanWriteSlice {
    fn write_slice<T: 'static>(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, values: &[T]) -> Result<()>
    where
        Self: CanWrite<T>
    {
        self.write_slice_of(ctx, values, |domain, ctx, value| {
            domain.write(ctx, value)
        })
    }
}

impl<D: CanWriteSlice> WriteSliceExt for D {}

pub trait CanWriteSliceWithArgs<T: 'static, A>: WriteDomain {
    fn write_slice_args_of<W: WriteCtx<Cat = Self::Cat>>(
        &mut self,
        ctx: &mut W,
        values: &[T],
        args: A,
        write_content: impl Fn(&mut Self, &mut W::InnerCtx<'_>, &T) -> Result<()>,
    ) -> Result<()>;
}

pub trait WriteSliceWithArgsFallbackExt<T: Writable<Self> + 'static, A>: CanWriteSliceWithArgs<T, A> {
    fn write_slice_args_fallback(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, values: &[T], args: A) -> Result<()> {
        self.write_slice_args_of(ctx, values, args, |domain, ctx, value| {
            domain.write_fallback::<T>(ctx, value)
        })
    }
}

impl<T: Writable<Self> + 'static, A, D: CanWriteSliceWithArgs<T, A>> WriteSliceWithArgsFallbackExt<T, A> for D {}

pub trait WriteSliceWithArgsExt<T: 'static, A>: CanWriteSliceWithArgs<T, A> + CanWrite<T> {
    fn write_slice_args(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, values: &[T], args: A) -> Result<()> {
        self.write_slice_args_of(ctx, values, args, |domain, ctx, value| {
            domain.write(ctx, value)
        })
    }
}

impl<T: 'static, A, D: CanWrite<T> + CanWriteSliceWithArgs<T, A>> WriteSliceWithArgsExt<T, A> for D {}

pub trait CanWrite<T: 'static + ?Sized>: WriteDomain  {
    fn write(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, value: &T) -> Result<()>;
}

pub trait CanWriteWithArgs<T: 'static, A: Default>: CanWrite<T> {
    fn write_args(&mut self, ctx: &mut impl WriteCtx<Cat = Self::Cat>, value: &T, args: A) -> Result<()>;
}

pub trait Writable<D: WriteDomain>: Sized {
    fn to_writer_unboxed(&self, ctx: &mut impl WriteCtx<Cat = D::Cat>, domain: &mut D) -> Result<()>;
    
    /// Override this with a write_box if this type should be boxed by default
    fn to_writer(&self, ctx: &mut impl WriteCtx<Cat = D::Cat>, domain: &mut D) -> Result<()> {
        self.to_writer_unboxed(ctx, domain)
    }
}

pub trait SimpleWritable<D: WriteDomain>: Sized {
    fn to_writer_simple(&self, writer: &mut impl Writer, domain: &mut D) -> Result<()>;
}

#[macro_export]
macro_rules! impl_writable_from_simple {
    ($type:ty) => {
        impl<D: $crate::WriteDomain> $crate::Writable<D> for $type {
            fn to_writer_unboxed(&self, ctx: &mut impl $crate::WriteCtx, domain: &mut D) -> Result<()> {
                self.to_writer_simple(ctx.cur_writer(), domain)
            }
        }
    };
}

// boxed serialization stuff
pub trait WriteCtx: Deref<Target = WriteHeap<Self::Writer>> + DerefMut {
    // TODO: should this even still be an associated type or make it into a type parameter?
    type Cat: Eq + Hash + Default + Clone;
    type Writer: Writer;
    type InnerCtx<'a>: WriteCtx<Cat = Self::Cat, Writer = Self::Writer> where Self: 'a;
    
    fn allocate_next_block<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>,
    ) -> Result<HeapToken>
    where
        Self::Cat: 'a;
    
    fn allocate_next_block_aligned<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        alignment: usize,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>
    ) -> Result<HeapToken>
    where
        Self::Cat: 'a;
    
    fn heap(&self, category: &Self::Cat) -> Option<&WriteHeap<Self::Writer>>;
    fn heap_mut(&mut self, category: Self::Cat) -> &mut WriteHeap<Self::Writer>;
    
    fn heap_id_of(&mut self, category: Self::Cat) -> u32;
    fn heap_token_at_current_pos(&mut self) -> Result<HeapToken>;
    
    // useful for child ctx's
    fn set_heap(&mut self, category: Self::Cat, heap: WriteHeap<Self::Writer>);
    fn remove_heap(&mut self, category: &Self::Cat) -> WriteHeap<Self::Writer>;
}

pub type WriteCtxWriter = Cursor<Vec<u8>>;

pub struct WriteCtxImpl<C: HeapCategory> {
    default_heap: WriteHeap<WriteCtxWriter>,
    heaps: IndexMap<C, WriteHeap<WriteCtxWriter>>,
}

impl<C: HeapCategory> WriteCtxImpl<C> {
    pub fn new() -> Self {
        WriteCtxImpl {
            default_heap: WriteHeap::new(),
            heaps: IndexMap::new(),
        }
    }
    
    pub fn to_buffer<D: WriteDomain<Cat = C>>(mut self, domain: &mut D, mut block_offsets: Option<&mut Vec<usize>>) -> Result<Vec<u8>> {
        let mut writer = WriteCtxWriter::default();
        
        self.heaps.insert(C::default(), self.default_heap);
        
        let mut heaps = self.heaps.iter().collect::<Vec<_>>();
        heaps.sort_by_key(|(category, _)| *category);
        
        for (_, heap) in heaps {
            heap.to_writer(&mut writer, domain, block_offsets.as_deref_mut())?;
        }
        
        Ok(writer.into_inner())
    }
}

impl<C: HeapCategory> Default for WriteCtxImpl<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: HeapCategory> WriteCtx for WriteCtxImpl<C> {
    type Cat = C;
    type Writer = WriteCtxWriter;
    type InnerCtx<'a> = InnerWriteCtx<'a, C, WriteCtxImpl<C>> where Self: 'a;

    fn allocate_next_block<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>,
    ) -> Result<HeapToken>
    where
        C: 'a,
    {
        let heap_id = self.heap_id_of(category.clone().unwrap_or_default());
        
        let mut ctx: InnerWriteCtx<'_, C, WriteCtxImpl<C>> = InnerWriteCtx::new(self, category.unwrap_or_default());
        
        let prev_current_block = ctx.default_heap.current_block;
        let new_block_token = ctx.default_heap.seek_to_new_block(0, heap_id)?;
        
        content_callback(&mut ctx)?;
        
        ctx.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    fn allocate_next_block_aligned<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        alignment: usize,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>
    ) -> Result<HeapToken>
    where
        C: 'a
    {
        let heap_id = self.heap_id_of(category.clone().unwrap_or_default());
        
        let mut ctx: InnerWriteCtx<'_, C, WriteCtxImpl<C>> = InnerWriteCtx::new(self, category.unwrap_or_default());
        let prev_current_block = ctx.default_heap.current_block;
        let new_block_token = ctx.default_heap.seek_to_new_block(alignment, heap_id)?;
        
        content_callback(&mut ctx)?;
        
        ctx.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    // I'm honestly not sure anymore where these are useful
    fn heap(&self, category: &Self::Cat) -> Option<&WriteHeap<Self::Writer>> {
        if *category == C::default() {
            Some(&self.default_heap)
        } else {
            self.heaps.get(category)
        }
    }
    
    fn heap_mut(&mut self, category: Self::Cat) -> &mut WriteHeap<Self::Writer> {
        if category == C::default() {
            &mut self.default_heap
        } else {
            // wow HashMap::entry is such a cool api actually
            self.heaps.entry(category).or_default()
        }
    }
    
    fn heap_id_of(&mut self, category: Self::Cat) -> u32 {
        self.heaps.entry(category.clone()).or_default();
        self.heaps.get_index_of(&category).unwrap() as u32
    }
    
    fn heap_token_at_current_pos(&mut self) -> Result<HeapToken> {
        let heap_id = self.heap_id_of(C::default());
        self.default_heap.heap_token_at_current_pos_inner(heap_id)
    }
    
    fn set_heap(&mut self, category: Self::Cat, heap: WriteHeap<Self::Writer>) {
        if category == C::default() {
            self.default_heap = heap;
        } else {
            self.heaps.insert(category, heap);
        }
    }
    
    fn remove_heap(&mut self, category: &Self::Cat) -> WriteHeap<Self::Writer> {
        if *category == C::default() {
            mem::take(&mut self.default_heap)
        } else {
            self.heaps
                .remove(category)
                .unwrap_or_default()
        }
    }
}

impl<C: HeapCategory> Deref for WriteCtxImpl<C> {
    type Target = WriteHeap<WriteCtxWriter>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<C: HeapCategory> DerefMut for WriteCtxImpl<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

pub struct InnerWriteCtx<'a, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
{
    default_category: C,
    default_heap: WriteHeap<WriteCtxWriter>,
    ctx: &'a mut W,
}

impl<'a, C, W> InnerWriteCtx<'a, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
{
    pub fn new(ctx: &'a mut W, default_category: C) -> Self {
        let default_heap = ctx.remove_heap(&default_category);
        
        Self {
            default_category,
            default_heap,
            ctx,
        }
    }
}

impl<C, W> WriteCtx for InnerWriteCtx<'_, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
{
    type Cat = C;
    type Writer = WriteCtxWriter;
    type InnerCtx<'a> = InnerWriteCtx<'a, C, Self> where Self: 'a;

    fn allocate_next_block<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>,
    ) -> Result<HeapToken> where C: 'a {
        let heap_id = self.ctx.heap_id_of(category.clone().unwrap_or_default());
        
        let mut ctx: InnerWriteCtx<'_, C, Self> = InnerWriteCtx::new(self, category.unwrap_or_default());
        
        let prev_current_block = ctx.default_heap.current_block;
        let new_block_token = ctx.default_heap.seek_to_new_block(0, heap_id)?;
        
        content_callback(&mut ctx)?;
        
        ctx.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    fn allocate_next_block_aligned<'a>(
        &'a mut self,
        category: Option<Self::Cat>,
        alignment: usize,
        content_callback: impl FnOnce(&mut Self::InnerCtx<'a>) -> Result<()>,
    ) -> Result<HeapToken> {
        let heap_id = self.ctx.heap_id_of(category.clone().unwrap_or_default());
        
        let mut ctx: InnerWriteCtx<'_, C, Self> = InnerWriteCtx::new(self, category.unwrap_or_default());
        
        let prev_current_block = ctx.default_heap.current_block;
        let new_block_token = ctx.default_heap.seek_to_new_block(alignment, heap_id)?;
        
        content_callback(&mut ctx)?;
        
        ctx.default_heap.current_block = prev_current_block;
        Ok(new_block_token)
    }
    
    fn heap(&self, category: &Self::Cat) -> Option<&WriteHeap<Self::Writer>> {
        if *category == self.default_category {
            Some(&self.default_heap)
        } else {
            self.ctx.heap(category)
        }
    }
    
    fn heap_mut(&mut self, category: Self::Cat) -> &mut WriteHeap<Self::Writer> {
        if category == self.default_category {
            &mut self.default_heap
        } else {
            self.ctx.heap_mut(category)
        }
    }

    fn heap_id_of(&mut self, category: Self::Cat) -> u32 {
        self.ctx.heap_id_of(category)
    }
    
    fn heap_token_at_current_pos(&mut self) -> Result<HeapToken> {
        let heap_id = self.ctx.heap_id_of(self.default_category.clone());
        self.default_heap.heap_token_at_current_pos_inner(heap_id)
    }
    
    fn set_heap(&mut self, category: Self::Cat, heap: WriteHeap<Self::Writer>) {
        if category == self.default_category {
            self.default_heap = heap;
        } else {
            self.ctx.set_heap(category, heap);
        }
    }

    fn remove_heap(&mut self, category: &Self::Cat) -> WriteHeap<Self::Writer> {
        if *category == self.default_category {
            mem::take(&mut self.default_heap)
        } else {
            self.ctx.remove_heap(category)
        }
    }
}

impl<C, W> Deref for InnerWriteCtx<'_, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
{
    type Target = WriteHeap<WriteCtxWriter>;

    fn deref(&self) -> &Self::Target {
        &self.default_heap
    }
}

impl<C, W> DerefMut for InnerWriteCtx<'_, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.default_heap
    }
}

impl<C, W> Drop for InnerWriteCtx<'_, C, W>
where
    C: HeapCategory,
    W: WriteCtx<Writer = WriteCtxWriter, Cat = C>,
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
    
    writer.write_all(&ZEROES[..padding_size as usize])?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HeapToken {
    heap_id: u32,
    block_id: u32,
    offset: usize,
}

impl HeapToken {
    pub fn resolve(self, block_offsets: &[usize]) -> usize {
        block_offsets[self.block_id as usize] + self.offset as usize
    }
}

impl PartialOrd for HeapToken {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapToken {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.block_id.cmp(&other.block_id) {
            Ordering::Equal => {}
            ord => return ord,
        }
        self.offset.cmp(&other.offset)
    }
}

#[derive(Clone, Debug, Default)]
pub struct HeapBlock<W: Writer> {
    relocations: Vec<(usize, HeapToken)>,
    writer: W,
}

impl<W: Writer> HeapBlock<W> {
    pub fn new() -> Self {
        Self::default()
    }
}

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
    
    fn heap_token_at_current_pos_inner(&mut self, heap_id: u32) -> Result<HeapToken> {
        Ok(HeapToken {
            heap_id,
            block_id: self.current_block as u32,
            offset: self.cur_writer().position()? as usize,
        })
    }
    
    fn seek_to_new_block(&mut self, alignment: usize, heap_id: u32) -> Result<HeapToken> {
        if self.current_block == self.blocks.len() - 1 {
            // allocate new block
            self.current_block = self.blocks.len();
            self.blocks.push(HeapBlock::new());
            // TODO: add alignment to HeapBlock
        } else {
            self.current_block += 1;
            self.align_to(alignment)?;
        }
        
        self.heap_token_at_current_pos_inner(heap_id)
    }
}

impl WriteHeap<WriteCtxWriter> {
    pub fn to_buffer(&self, domain: &mut impl WriteDomain, block_offsets: Option<&mut Vec<usize>>) -> Result<Vec<u8>> {
        let mut writer = WriteCtxWriter::default();
        self.to_writer(&mut writer, domain, block_offsets)?;
        Ok(writer.into_inner())
    }
    
    pub fn to_writer(&self, writer: &mut WriteCtxWriter, domain: &mut impl WriteDomain, mut block_offsets: Option<&mut Vec<usize>>) -> Result<()> {
        let mut all_relocations: Vec<(usize, HeapToken)> = Vec::new();
        
        for (block_id, block) in self.blocks.iter().enumerate() {
            let block_start = Cursor::position(writer) as usize;
            if let Some(block_offsets) = block_offsets.as_mut() {
                block_offsets.push(block_start);
            }
            writer.write_all(block.writer.get_ref())?;
            
            // apply previous relocations
            let all_relocations_to_current = all_relocations.extract_if(.., |(_, token)| {
                token.block_id as usize == block_id
            });
            
            for (offset, token) in all_relocations_to_current {
                let writer_pos = Cursor::position(writer);
                writer.set_position(offset as u64);
                
                domain.apply_reference(writer, block_start + token.offset as usize)?;
                
                writer.set_position(writer_pos);
            }
            
            // push new relocations
            all_relocations.extend(block.relocations.iter().copied()
                .map(|(local_offset, token)| (block_start + local_offset, token)));
        }
        Ok(())
    }
}

impl<W: Writer> Default for WriteHeap<W> {
    fn default() -> Self {
        Self::new()
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
