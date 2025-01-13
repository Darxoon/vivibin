use std::{any::TypeId, io::Cursor, mem::{transmute, ManuallyDrop}, ptr::read};

use anyhow::Result;
use vivibin::{default_impls::BoolSize, pointers::PointerZero32, scoped_reader_pos, EndianSpecific, Endianness, ReadDomain, Readable, ReadableWithArgs, Reader, Writable, WriteDomain, Writer};

// typedef for more convenient access
type Pointer = PointerZero32;

#[derive(Clone, Copy)]
struct FormatCgfx; // cgfx is an actual data type btw and the main reason I did this (3DS related)

impl FormatCgfx {
    pub fn read_i32(reader: &mut impl Reader) -> Result<i32> {
        let mut bytes: [u8; 4] = Default::default();
        reader.read(&mut bytes)?;
        
        Ok(match Self.endianness() {
            Endianness::Little => i32::from_le_bytes(bytes),
            Endianness::Big => i32::from_be_bytes(bytes),
        })
    }
    
    pub fn write_i32(writer: &mut impl Writer, value: i32) -> Result<()> {
        let bytes = match Self.endianness() {
            Endianness::Little => value.to_le_bytes(),
            Endianness::Big => value.to_be_bytes(),
        };
        
        writer.write(&bytes)?;
        Ok(())
    }
    
    pub fn read_u32(reader: &mut impl Reader) -> Result<u32> {
        let mut bytes: [u8; 4] = Default::default();
        reader.read(&mut bytes)?;
        
        Ok(match Self.endianness() {
            Endianness::Little => u32::from_le_bytes(bytes),
            Endianness::Big => u32::from_be_bytes(bytes),
        })
    }
    
    pub fn write_u32(writer: &mut impl Writer, value: u32) -> Result<()> {
        let bytes = match Self.endianness() {
            Endianness::Little => value.to_le_bytes(),
            Endianness::Big => value.to_be_bytes(),
        };
        
        writer.write(&bytes)?;
        Ok(())
    }
    
    pub fn read_relative_ptr(reader: &mut impl Reader) -> Result<Pointer> {
        let pos = reader.position()?;
        let raw_ptr = u32::from_reader(reader, Self)?;
        Ok(if raw_ptr != 0 { Pointer::new(pos as u32 + raw_ptr) } else { Pointer::new(0) })
    }
    
    pub fn write_relative_ptr(writer: &mut impl Writer, value: Pointer) -> Result<()> {
        let relative = value.value() - writer.position()? as u32;
        relative.to_writer(writer, Self)?;
        Ok(())
    }
    
    pub fn read_str(reader: &mut impl Reader) -> Result<String> {
        // reads a boxed string and not an inline string despite read_boxed's existence
        // because inline strings are never used in this format so this is 100x a more
        // sensible default
        let ptr = Self::read_relative_ptr(reader)?;
        
        scoped_reader_pos!(reader); // jump to pointer will be undone in destructor
        reader.set_position(ptr)?;
        
        Ok(reader.read_c_str()?)
    }
    
    pub fn write_str(writer: &mut impl Writer, value: &String) -> Result<()> {
        // TODO: this is basically a boxed value so idk how to handle this yet
        0u32.to_writer(writer, Self)?;
        Ok(())
    }
}

impl EndianSpecific for FormatCgfx {
    fn endianness(self) -> Endianness {
        Endianness::Little
    }
}

impl ReadDomain for FormatCgfx {
    type Flags = ();
    type Pointer = Pointer;

    fn read<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<T>> {
        let result: Option<T>;
        let type_id = TypeId::of::<T>();
        
        // this can be auto generated by a macro super easily
        // this should also hopefully? get otimized out
        // at least once TypeId::of becomes a stable const fn
        if type_id == TypeId::of::<i32>() {
            let value = ManuallyDrop::new(Self::read_i32(reader)?);
            
            result = Some(unsafe { read(transmute::<&i32, &T>(&value)) });
        } else if type_id == TypeId::of::<u32>() {
            let value = ManuallyDrop::new(Self::read_u32(reader)?);
            
            result = Some(unsafe { read(transmute::<&u32, &T>(&value)) });
        } else if type_id == TypeId::of::<Pointer>() {
            let value = ManuallyDrop::new(Self::read_relative_ptr(reader)?);
            
            result = Some(unsafe { read(transmute::<&Pointer, &T>(&value)) });
        } else if type_id == TypeId::of::<String>() {
            let value = ManuallyDrop::new(Self::read_str(reader)?);
            
            result = Some(unsafe { read(transmute::<&String, &T>(&value)) });
        } else {
            result = None;
        }
        
        Ok(result)
    }
    
    fn read_args<T: 'static, U>(self, _reader: &mut impl Reader, _args: U) -> Result<Option<T>> {
        Ok(None)
    }
    
    fn read_box<T, R: Reader>(self, reader: &mut R, parser: impl FnOnce(&mut R, Self) -> Result<T>) -> Result<Option<T>> {
        let ptr = Self::read_relative_ptr(reader)?;
        
        if ptr.value() == 0 {
            return Ok(None)
        }
        
        scoped_reader_pos!(reader); // jump to pointer will be undone in destructor
        reader.set_position(ptr)?;
        
        Ok(Some(parser(reader, self)?))
    }
    
    fn read_boxed<T: 'static>(self, reader: &mut impl Reader) -> Result<Option<Option<T>>> {
        let result;
        let type_id = TypeId::of::<T>();
        
        // this can be auto generated by a macro super easily
        // this should also hopefully? get otimized out
        // at least once TypeId::of becomes a stable const fn
        if type_id == TypeId::of::<i32>() {
            let value = ManuallyDrop::new(
                self.read_box(reader, |reader, _| Self::read_i32(reader))?);
            
            result = Some(value.as_ref().map(|value| unsafe { read(transmute::<&i32, &T>(value)) }));
        } else if type_id == TypeId::of::<u32>() {
            let value = ManuallyDrop::new(
                self.read_box(reader, |reader, _| FormatCgfx::read_u32(reader))?);
            
            result = Some(value.as_ref().map(|value| unsafe { read(transmute::<&u32, &T>(value)) }));
        } else if type_id == TypeId::of::<Pointer>() {
            let value = ManuallyDrop::new(
                self.read_box(reader, |reader, _| Self::read_relative_ptr(reader))?);
            
            result = Some(value.as_ref().map(|value| unsafe { read(transmute::<&Pointer, &T>(value)) }));
        } else if type_id == TypeId::of::<String>() {
            let value = ManuallyDrop::new(
                self.read_box(reader, |reader, _| Self::read_str(reader))?);
            
            result = Some(value.as_ref().map(|value| unsafe { read(transmute::<&String, &T>(value)) }));
        } else {
            result = None;
        }
        
        Ok(result)
    }
    
    fn read_boxed_args<T: 'static, U>(self, _reader: &mut impl Reader, _args: U) -> Result<Option<Option<T>>> {
        Ok(None)
    }
}

impl WriteDomain for FormatCgfx {
    type Flags = ();
    type Pointer = Pointer;

    fn write<T: 'static>(self, writer: &mut impl vivibin::Writer, value: &T) -> Result<Option<()>> {
        let type_id = TypeId::of::<T>();
        
        // this can be auto generated by a macro super easily
        // this should also hopefully? get otimized out
        // at least once TypeId::of becomes a stable const fn
        if type_id == TypeId::of::<i32>() {
            let value = unsafe { transmute::<&T, &i32>(value) };
            Self::write_i32(writer, *value)?;
            Ok(Some(()))
        } else if type_id == TypeId::of::<u32>() {
            let value = unsafe { transmute::<&T, &u32>(value) };
            Self::write_u32(writer, *value)?;
            Ok(Some(()))
        } else if type_id == TypeId::of::<Pointer>() {
            let value = unsafe { transmute::<&T, &Pointer>(value) };
            Self::write_relative_ptr(writer, *value)?;
            Ok(Some(()))
        } else if type_id == TypeId::of::<String>() {
            let value = unsafe { transmute::<&T, &String>(value) };
            Self::write_str(writer, value)?;
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }
}

impl Readable for Vec3 {
    fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self> {
        let x = match domain.read::<f32>(reader)? {
            Some(x) => x,
            None => f32::from_reader(reader, domain)?,
        };
        let y = match domain.read::<f32>(reader)? {
            Some(y) => y,
            None => f32::from_reader(reader, domain)?,
        };
        let z = match domain.read::<f32>(reader)? {
            Some(z) => z,
            None => f32::from_reader(reader, domain)?,
        };
        
        Ok(Vec3::new(x, y, z))
    }
}

impl Writable for Vec3 {
    fn to_writer(&self, writer: &mut impl Writer, domain: impl WriteDomain) -> Result<()> {
        if domain.write::<f32>(writer, &self.x)?.is_none() {
            self.x.to_writer(writer, domain)?;
        }
        if domain.write::<f32>(writer, &self.y)?.is_none() {
            self.y.to_writer(writer, domain)?;
        }
        if domain.write::<f32>(writer, &self.z)?.is_none() {
            self.z.to_writer(writer, domain)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct Npc {
    name: String,
    position: Vec3,
    // future proc macro?: #[args(BoolSize::U32)]
    is_visible: bool,
    
    // serializing boxed types is a lot more complex;
    // if you want a deserialize+serialize to provide a matching binary,
    // then you have to be really careful with serialization order
    // 
    // future proc macro?: #[boxed] (smart handling of Box<> in field type)
    // child: Option<Box<Npc>>,
}

impl Readable for Npc {
    fn from_reader(reader: &mut impl Reader, domain: impl ReadDomain) -> Result<Self> {
        let name = match domain.read::<String>(reader)? {
            Some(x) => x,
            None => panic!(), // Ideally do a compile time check here :)
        };
        let position = match domain.read::<Vec3>(reader)? {
            Some(x) => x,
            None => Vec3::from_reader(reader, domain)?, // Ideally do a compile time check here :)
        };
        let is_visible = match domain.read_args::<bool, BoolSize>(reader, BoolSize::U32)? {
            Some(x) => x,
            None => bool::from_reader_args(reader, domain, BoolSize::U32)?, // Ideally do a compile time check here :)
        };
        // let child = match domain.read_boxed(reader)? {
        //     Some(x) => x,
        //     None => domain.read_box(reader, |reader, domain| Npc::from_reader(reader, domain))?,
        // }.map(|x| Box::new(x));
        
        Ok(Npc {
            name,
            position,
            is_visible,
            // child,
        })
    }
}

impl Writable for Npc {
    fn to_writer(&self, writer: &mut impl Writer, domain: impl WriteDomain) -> Result<()> {
        // TODO: should I add a special case for &str
        if domain.write::<String>(writer, &self.name)?.is_none() {
            panic!(); // String does not have a default implementation
        }
        if domain.write::<Vec3>(writer, &self.position)?.is_none() {
            self.position.to_writer(writer, domain)?;
        }
        // TODO: booleans with options
        if domain.write::<bool>(writer, &self.is_visible)?.is_none() {
            self.is_visible.to_writer(writer, domain)?;
        }
        // TODO: boxed child
        Ok(())
    }
}

fn main() -> Result<()> {
    const VEC3_BYTES: [u8; 32] = [
        // name ptr
        0x14, 0, 0, 0,
        // position vec3
        0, 0, 0x80, 0x3f, 0, 0, 0, 0x40, 0, 0, 0, 0x3f,
        // isvisible
        1, 0, 0, 0,
        // child
        // 0x10, 0, 0, 0,
        // name string
        0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0,
        
        // // child
        // // name ptr
        // 0x18, 0, 0, 0,
        // // position vec3
        // 0, 0, 0, 0x40, 0, 0, 0x80, 0x3f, 0, 0, 0, 0x3f,
        // // isvisible
        // 0, 0, 0, 0,
        // // child
        // 0, 0, 0, 0,
        // // name string
        // 0x48, 0x69, 0x69, 0x69, 0x69, 0x69, 0, 0,
    ];
    
    let mut cursor: Cursor<&[u8]> = Cursor::new(&VEC3_BYTES);
    let npc = Npc::from_reader(&mut cursor, FormatCgfx)?;
    println!("Hello World {:?}", npc);
    
    let mut writer: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    npc.to_writer(&mut writer, FormatCgfx)?;
    println!("Written {:#x?}", writer.get_ref() as &[u8]);
    Ok(())
}
