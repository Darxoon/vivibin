use core::marker::PhantomData;
use std::io::Cursor;

use anyhow::Result;
use vivibin::{
    scoped_reader_pos, CanRead, CanReadVec, CanWrite, CanWriteBox, CanWriteSlice,
    CanWriteSliceWithArgs, CanWriteWithArgs, EndianSpecific, Endianness, HeapCategory, ReadDomain,
    ReadVecFallbackExt, Readable, Reader, SimpleWritable, Writable, WriteCtx, WriteDomain,
    WriteDomainExt, WriteSliceWithArgsFallbackExt, Writer,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Pointer(u32);

impl Pointer {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
    
    pub fn value(self) -> u32 {
        self.0
    }
}

impl From<Pointer> for u64 {
    fn from(value: Pointer) -> Self {
        value.0 as u64
    }
}

impl From<usize> for Pointer {
    fn from(value: usize) -> Self {
        Pointer(value as u32)
    }
}

#[derive(Clone, Default)]
struct FormatCgfx<C: HeapCategory>(PhantomData<C>); // cgfx is an actual data type btw and the main reason I did this (3DS related)

impl<C: HeapCategory> FormatCgfx<C> {
    pub fn read_relative_ptr(reader: &mut impl Reader) -> Result<Pointer> {
        let pos = reader.position()?;
        let raw_ptr = u32::from_reader(reader, Self::default())?;
        Ok(if raw_ptr != 0 {
            Pointer::new(pos as u32 + raw_ptr)
        } else {
            Pointer::new(0)
        })
    }
    
    pub fn write_relative_ptr(writer: &mut impl Writer, value: Pointer) -> Result<()> {
        let relative = value.value() - writer.position()? as u32;
        relative.to_writer_simple(writer, &mut Self::default())?;
        Ok(())
    }
    
    pub fn read_str(reader: &mut impl Reader) -> Result<String> {
        // reads a boxed string and not an inline string despite read_boxed's existence
        // because inline strings are never used in this format so this is 100x a more
        // sensible default
        let ptr = Self::read_relative_ptr(reader)?;
        
        scoped_reader_pos!(reader); // jump to pointer will be undone in destructor
        reader.set_position(ptr)?;
        
        reader.read_c_str()
    }
    
    pub fn write_str(ctx: &mut impl WriteCtx<C>, value: &str) -> Result<()> {
        let token = ctx.allocate_next_block(None, move |ctx| {
            ctx.write_c_str(value)?;
            Ok(())
        })?;
        
        ctx.write_token::<4>(token)?;
        Ok(())
    }
    
    pub fn write_str_new(&mut self, ctx: &mut impl WriteCtx<C>) -> Result<()> {
        0u32.to_writer(ctx, self)?;
        Ok(())
    }
    
    pub fn write_str_new_post(&mut self, ctx: &mut impl WriteCtx<C>, value: &str) -> Result<()> {
        ctx.write_c_str(value)?;
        Ok(())
    }
    
    pub fn read_vec<T, R: Reader>(reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Vec<T>> {
        let count = u32::from_reader(reader, Self::default())?;
        let content = Self::default().read_box_nullable(reader, |reader| {
            let mut result = Vec::with_capacity(count as usize);
            
            for _ in 0..count {
                result.push(read_content(reader)?);
            }
            
            Ok(result)
        })?;
        
        Ok(content.unwrap_or_default())
    }
}

// https://github.com/rust-lang/rust/issues/26925
impl<C: HeapCategory> Copy for FormatCgfx<C> {}

impl<C: HeapCategory> EndianSpecific for FormatCgfx<C> {
    fn endianness(&self) -> Endianness {
        Endianness::Little
    }
}

impl<C: HeapCategory> ReadDomain for FormatCgfx<C> {
    type Pointer = Pointer;
    
    fn read_box_nullable<T, R: Reader>(self, reader: &mut R, read_content: impl FnOnce(&mut R) -> Result<T>) -> Result<Option<T>> {
        let ptr = Self::read_relative_ptr(reader)?;
        
        if ptr.value() == 0 {
            return Ok(None)
        }
        
        scoped_reader_pos!(reader); // jump to pointer will be undone in destructor
        reader.set_position(ptr)?;
        
        Ok(Some(read_content(reader)?))
    }
}

impl<C: HeapCategory> CanReadVec for FormatCgfx<C> {
    fn read_std_vec_of<T, R: Reader>(self, reader: &mut R, read_content: impl Fn(&mut R) -> Result<T>) -> Result<Vec<T>> {
        Self::read_vec(reader, read_content)
    }
}

impl<C: HeapCategory> CanRead<String> for FormatCgfx<C> {
    fn read(self, reader: &mut impl Reader) -> Result<String> {
        Self::read_str(reader)
    }
}

impl<C: HeapCategory> CanRead<Pointer> for FormatCgfx<C> {
    fn read(self, reader: &mut impl Reader) -> Result<Pointer> {
        Self::read_relative_ptr(reader)
    }
}

// ... more CanRead implementations

impl<C: HeapCategory> WriteDomain for FormatCgfx<C> {
    type Pointer = Pointer;
    type Cat = C;
    
    fn apply_reference(&mut self, writer: &mut impl Writer, heap_offset: usize) -> Result<()> {
        Self::write_relative_ptr(writer, heap_offset.into())
    }
}

impl<C: HeapCategory> CanWriteBox<C> for FormatCgfx<C> {
    fn write_box_of<W: WriteCtx<C>>(
        &mut self,
        ctx: &mut W,
        write_content: impl FnOnce(&mut Self, &mut W::InnerCtx<'_>) -> Result<()>,
    ) -> Result<()> {
        let token = ctx.allocate_next_block(None, |ctx| {
            write_content(self, ctx)
        })?;
        
        ctx.write_token::<4>(token)
    }
}

impl<C: HeapCategory> CanWriteSlice<C> for FormatCgfx<C> {
    fn write_slice_of<T: 'static, W: WriteCtx<C>>(
        &mut self,
        ctx: &mut W,
        values: &[T],
        write_content: impl Fn(&mut Self, &mut W::InnerCtx<'_>, &T) -> Result<()>,
    ) -> Result<()> {
        (values.len() as u32).to_writer(ctx, self)?;
        let item_ids_token = ctx.allocate_next_block(None, |ctx| {
            for value in values {
                write_content(self, ctx, value)?;
            }
            Ok(())
        })?;
        ctx.write_token::<4>(item_ids_token)?;
        Ok(())
    }
}

impl<C: HeapCategory, T: 'static> CanWriteSliceWithArgs<C, T, NewSerialization> for FormatCgfx<C> {
    fn write_slice_args_of<W: WriteCtx<C>>(
        &mut self,
        ctx: &mut W,
        values: &[T],
        _args: NewSerialization,
        _write_content: impl Fn(&mut Self, &mut W::InnerCtx<'_>, &T) -> Result<()>,
    ) -> Result<()> {
        (values.len() as u32).to_writer(ctx, self)?;
        0u32.to_writer(ctx, self)?;
        Ok(())
    }
    
    fn write_slice_args_post_of<W: WriteCtx<C>>(
        &mut self,
        ctx: &mut W,
        values: &[T],
        _args: NewSerialization,
        write_content: impl Fn(&mut Self, &mut W, &T) -> Result<()>,
    ) -> Result<()> {
        for value in values {
            write_content(self, ctx, value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct NewSerialization;

impl<C: HeapCategory> CanWrite<C, str> for FormatCgfx<C> {
    fn write(&mut self, ctx: &mut impl WriteCtx<C>, value: &str) -> Result<()> {
        Self::write_str(ctx, value)
    }
}
impl<C: HeapCategory> CanWrite<C, String> for FormatCgfx<C> {
    fn write(&mut self, ctx: &mut impl WriteCtx<C>, value: &String) -> Result<()> {
        Self::write_str(ctx, value)
    }
}
impl<C: HeapCategory> CanWriteWithArgs<C, String, NewSerialization> for FormatCgfx<C> {
    fn write_args(&mut self, ctx: &mut impl WriteCtx<C>, _value: &String, _: NewSerialization) -> Result<()> {
        self.write_str_new(ctx)
    }
    
    fn write_args_post(&mut self, ctx: &mut impl WriteCtx<C>, value: &String, _: NewSerialization) -> Result<()> {
        self.write_str_new_post(ctx, value)
    }
}
impl<C: HeapCategory> CanWrite<C, Pointer> for FormatCgfx<C> {
    fn write(&mut self, ctx: &mut impl WriteCtx<C>, value: &Pointer) -> Result<()> {
        Self::write_relative_ptr(ctx.cur_writer(), *value)
    }
}

#[derive(Debug, Clone, Readable, Writable)]
struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Readable)]
#[boxed]
struct BoxedChild {
    id: u32,
    visible: bool,
}

impl<C: HeapCategory, D: CanWriteBox<C>> Writable<C, D> for BoxedChild {
    fn to_writer_unboxed(&self, ctx: &mut impl WriteCtx<C>, domain: &mut D) -> Result<()> {
        self.id.to_writer(ctx, domain)?;
        self.visible.to_writer(ctx, domain)?;
        Ok(())
    }
    
    fn to_writer(&self, ctx: &mut impl WriteCtx<C>, domain: &mut D) -> Result<()> {
        domain.write_box_of(ctx, |domain, ctx| {
            self.to_writer_unboxed(ctx, domain)
        })
    }
}

#[derive(Debug, Clone, Readable)]
#[boxed]
struct NewBoxedChild {
    id: u32,
    visible: bool,
}

impl<C: HeapCategory, D: CanWriteBox<C>> Writable<C, D> for NewBoxedChild {
    fn to_writer_unboxed(&self, ctx: &mut impl WriteCtx<C>, domain: &mut D) -> Result<()> {
        self.id.to_writer(ctx, domain)?;
        self.visible.to_writer(ctx, domain)?;
        Ok(())
    }
    
    // #[boxed] with the new serialization method would generate this:
    // (ignoring the lack of relocation support for this)
    fn to_writer(&self, ctx: &mut impl WriteCtx<C>, domain: &mut D) -> Result<()> {
        0u32.to_writer(ctx, domain)?;
        Ok(())
    }
    
    fn to_writer_post(&self, ctx: &mut impl WriteCtx<C>, domain: &mut D) -> Result<()> {
        self.to_writer_unboxed(ctx, domain)?;
        self.to_writer_unboxed_post(ctx, domain)?;
        Ok(())
    }
}

#[derive(Debug, Readable, Writable)]
#[allow(dead_code)]
#[extra_write_domain_deps(CanWriteBox<Cat>)]
struct SimpleNpc {
    #[require_domain]
    name: String,
    position: Vec3,
    is_visible: bool,
    item_ids: Vec<u32>,
    child: BoxedChild,
}

#[derive(Debug)]
#[allow(dead_code)]
struct Npc {
    name: String,
    position: Vec3,
    // future proc macro?: #[args(BoolSize::U32)]
    is_visible: bool,
    
    item_ids: std::vec::Vec<u32>,
    
    child: NewBoxedChild,
}

impl<D: CanRead<String> + CanReadVec> Readable<D> for Npc {
    fn from_reader_unboxed<R: Reader>(reader: &mut R, domain: D) -> Result<Self> {
        let name = domain.read(reader)?;
        let position = Vec3::from_reader(reader, domain)?;
        let is_visible = bool::from_reader(reader, domain)?;
        let item_ids: Vec<u32> = domain.read_std_vec_fallback::<u32, R>(reader)?;
        // explicitly boxed (TODO: implement this into derive macro)
        // let child = domain.read_box::<BoxedChild, R>(reader, |reader| {
        //     BoxedChild::from_reader_unboxed(reader, domain)
        // })?;
        let child = NewBoxedChild::from_reader(reader, domain)?;
        
        Ok(Npc {
            name,
            position,
            is_visible,
            item_ids,
            child,
        })
    }
}

impl<Cat, D> Writable<Cat, D> for Npc
where
    Cat: HeapCategory,
    D: CanWrite<Cat, String>
        + CanWriteWithArgs<Cat, String, NewSerialization>
        + CanWriteSlice<Cat>
        + CanWriteSliceWithArgs<Cat, u32, NewSerialization>
        + CanWriteBox<Cat>,
{
    fn to_writer_unboxed(&self, ctx: &mut impl WriteCtx<Cat>, domain: &mut D) -> Result<()> {
        domain.write_args(ctx, &self.name, NewSerialization)?;
        self.position.to_writer(ctx, domain)?;
        self.is_visible.to_writer(ctx, domain)?;
        
        domain.write_slice_args_fallback(ctx, &self.item_ids, NewSerialization)?;
        
        self.child.to_writer(ctx, domain)?;
        Ok(())
    }
    
    fn to_writer_unboxed_post(&self, ctx: &mut impl WriteCtx<Cat>, domain: &mut D) -> Result<()> {
        domain.write_args_post(ctx, &self.name, NewSerialization)?;
        self.position.to_writer_post(ctx, domain)?;
        self.is_visible.to_writer_post(ctx, domain)?;
        
        domain.write_slice_args_post_fallback(ctx, &self.item_ids, NewSerialization)?;
        
        self.child.to_writer_post(ctx, domain)?;
        Ok(())
    }
}

fn main() -> Result<()> {
    const BYTES: &[u8] = &[
        // name ptr
        0x20, 0, 0, 0,
        // position vec3
        0, 0, 0x80, 0x3f, 0, 0, 0, 0x40, 0, 0, 0, 0x3f,
        // isvisible
        1, 0, 0, 0,
        // item_ids
        3, 0, 0, 0, 0x14, 0, 0, 0,
        // child_ptr
        0x1c, 0, 0, 0,
        
        // name string
        0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0,
        // item_ids values
        0x1, 0, 0, 0, 0x4, 0, 0, 0, 0x8, 0, 0, 0, 
        
        // child
        // id
        0x18, 0, 0, 0,
        // visible
        1, 0, 0, 0,
    ];
    
    let mut cursor: Cursor<&[u8]> = Cursor::new(BYTES);
    let npc = Npc::from_reader(&mut cursor, FormatCgfx::<()>::default())?;
    println!("Hello World {npc:?}");
    
    let mut ctx = FormatCgfx::<()>::new_ctx();
    npc.to_writer(&mut ctx, &mut FormatCgfx::<()>::default())?;
    npc.to_writer_post(&mut ctx, &mut FormatCgfx::<()>::default())?;
    
    let written = ctx.to_buffer(&mut FormatCgfx::<()>::default(), None)?;
    println!("Written {written:x?}");
    assert_eq!(&written, &BYTES, "Serialization failure, result not matching");
    Ok(())
}
