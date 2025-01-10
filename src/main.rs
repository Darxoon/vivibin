use std::{any::TypeId, io::Cursor};

use anyhow::Result;
use vivibin::{scoped_reader_pos, Endianness, ReadDomain, Readable, Reader};

struct Pointer(u32); // I have a more sophisticated pointer class elsewhere

impl Into<u64> for Pointer {
    fn into(self) -> u64 {
        self.0 as u64
    }
}

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
    
    pub fn read_u32(reader: &mut impl Reader) -> Result<u32> {
        let mut bytes: [u8; 4] = Default::default();
        reader.read(&mut bytes)?;
        
        Ok(match Self.endianness() {
            Endianness::Little => u32::from_le_bytes(bytes),
            Endianness::Big => u32::from_be_bytes(bytes),
        })
    }
    
    pub fn read_relative_ptr(reader: &mut impl Reader) -> Result<Pointer> {
        let pos = reader.position()?;
        let raw_ptr = u32::from_reader(reader, Self)?;
        Ok(Pointer(pos as u32 + raw_ptr))
    }
    
    pub fn read_str(reader: &mut impl Reader) -> Result<String> {
        let ptr = Self::read_relative_ptr(reader)?;
        
        scoped_reader_pos!(reader); // jump to pointer will be undone in destructor
        reader.set_position(ptr)?;
        
        let mut bytes = Vec::new();
        loop {
            let b = u8::from_reader(reader, Self)?;
            
            if b == 0 {
                break;
            }
            
            bytes.push(b);
        }
        
        Ok(String::from_utf8(bytes)?)
    }
}

impl ReadDomain for FormatCgfx {
    type Flags = ();
    type Pointer = Pointer;

    fn endianness(self) -> Endianness {
        Endianness::Little
    }
    
    fn read<T: Clone + 'static>(self, reader: &mut impl Reader) -> Result<Option<T>> {
        let result: Option<T>;
        let type_id = TypeId::of::<T>();
        
        // this can be auto generated by a macro super easily
        // this should also hopefully? get otimized out
        // at least once TypeId::of becomes a stable const fn
        if type_id == TypeId::of::<i32>() {
            let value = Self::read_i32(reader)?;
            
            result = Some(unsafe { (*((&value as *const i32) as *const T)).clone() });
        } else if type_id == TypeId::of::<u32>() {
            let value = Self::read_u32(reader)?;
            
            result = Some(unsafe { (*((&value as *const u32) as *const T)).clone() });
        } else if type_id == TypeId::of::<Pointer>() {
            let value = Self::read_relative_ptr(reader)?;
            
            result = Some(unsafe { (*((&value as *const Pointer) as *const T)).clone() });
        } else if type_id == TypeId::of::<String>() {
            let value = Self::read_str(reader)?;
            
            result = Some(unsafe { (*((&value as *const String) as *const T)).clone() });
        } else {
            result = None;
        }
        
        Ok(result)
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug)]
struct Npc {
    name: String,
    position: Vec3,
    is_visible: u32, // TODO: bool
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
        let is_visible = match domain.read::<u32>(reader)? {
            Some(x) => x,
            None => u32::from_reader(reader, domain)?, // Ideally do a compile time check here :)
        };
        
        Ok(Npc {
            name,
            position,
            is_visible,
        })
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
        // name string
        0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0,
    ];
    
    let mut cursor: Cursor<&[u8]> = Cursor::new(&VEC3_BYTES);
    let vec = Npc::from_reader(&mut cursor, FormatCgfx)?;
    println!("Hello World {:?}", vec);
    Ok(())
}
