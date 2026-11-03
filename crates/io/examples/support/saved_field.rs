//! Select an existing Serialize struct field without encoding unrelated map keys.
use serde::{
    Serialize,
    ser::{Error, Impossible, SerializeStruct, Serializer},
};
use serde_json::Value;

pub fn read(value: &impl Serialize, path: &[&str]) -> Result<Value, serde_json::Error> {
    if path.is_empty() {
        return serde_json::to_value(value);
    }
    value.serialize(Select(path))
}
struct Select<'a>(&'a [&'a str]);
struct Fields<'a> {
    path: &'a [&'a str],
    found: Option<Value>,
}
impl SerializeStruct for Fields<'_> {
    type Ok = Value;
    type Error = serde_json::Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        if key == self.path[0] {
            self.found = Some(if self.path.len() == 1 {
                serde_json::to_value(value)?
            } else {
                value.serialize(Select(&self.path[1..]))?
            });
        }
        Ok(())
    }
    fn end(self) -> Result<Value, Self::Error> {
        self.found
            .ok_or_else(|| Self::Error::custom(format!("missing saved field {}", self.path[0])))
    }
}
macro_rules! reject {
    ($name:ident($($arg:ident:$ty:ty),*) -> $ret:ty) => {
        fn $name(self,$($arg:$ty),*) -> Result<$ret,Self::Error> {
            Err(Self::Error::custom("saved field path requires a struct"))
        }
    };
}
impl<'a> Serializer for Select<'a> {
    type Ok = Value;
    type Error = serde_json::Error;
    type SerializeSeq = Impossible<Value, Self::Error>;
    type SerializeTuple = Impossible<Value, Self::Error>;
    type SerializeTupleStruct = Impossible<Value, Self::Error>;
    type SerializeTupleVariant = Impossible<Value, Self::Error>;
    type SerializeMap = Impossible<Value, Self::Error>;
    type SerializeStruct = Fields<'a>;
    type SerializeStructVariant = Impossible<Value, Self::Error>;
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(Fields {
            path: self.0,
            found: None,
        })
    }
    reject!(serialize_bool(_v:bool)->Value);
    reject!(serialize_i8(_v:i8)->Value);
    reject!(serialize_i16(_v:i16)->Value);
    reject!(serialize_i32(_v:i32)->Value);
    reject!(serialize_i64(_v:i64)->Value);
    reject!(serialize_u8(_v:u8)->Value);
    reject!(serialize_u16(_v:u16)->Value);
    reject!(serialize_u32(_v:u32)->Value);
    reject!(serialize_u64(_v:u64)->Value);
    reject!(serialize_f32(_v:f32)->Value);
    reject!(serialize_f64(_v:f64)->Value);
    reject!(serialize_char(_v:char)->Value);
    reject!(serialize_str(_v:&str)->Value);
    reject!(serialize_bytes(_v:&[u8])->Value);
    reject!(serialize_none()->Value);
    reject!(serialize_unit()->Value);
    reject!(serialize_unit_struct(_name:&'static str)->Value);
    reject!(serialize_unit_variant(_name:&'static str,_index:u32,_variant:&'static str)->Value);
    fn serialize_some<T: ?Sized + Serialize>(self, _v: &T) -> Result<Value, Self::Error> {
        Err(Self::Error::custom("expected saved struct"))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, Self::Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _v: &T,
    ) -> Result<Value, Self::Error> {
        Err(Self::Error::custom("expected saved struct"))
    }
    reject!(serialize_seq(_len:Option<usize>)->Self::SerializeSeq);
    reject!(serialize_tuple(_len:usize)->Self::SerializeTuple);
    reject!(serialize_tuple_struct(_name:&'static str,_len:usize)->Self::SerializeTupleStruct);
    reject!(serialize_tuple_variant(_name:&'static str,_index:u32,_variant:&'static str,_len:usize)->Self::SerializeTupleVariant);
    reject!(serialize_map(_len:Option<usize>)->Self::SerializeMap);
    reject!(serialize_struct_variant(_name:&'static str,_index:u32,_variant:&'static str,_len:usize)->Self::SerializeStructVariant);
}
