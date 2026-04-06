use std::marker::PhantomData;

use serde::{
    Deserialize, Deserializer,
    de::value::{EnumAccessDeserializer, MapAccessDeserializer, SeqAccessDeserializer},
};

pub fn value_or_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(PhantomData<T>);

    macro_rules! forward {
        ($f:ident($v:ty)) => {
            fn $f<E>(self, v: $v) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                use serde::de::IntoDeserializer;
                Ok(vec![T::deserialize(v.into_deserializer())?])
            }
        };
    }

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an item or a list of items")
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            Vec::deserialize(SeqAccessDeserializer::new(seq))
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            Ok(vec![T::deserialize(MapAccessDeserializer::new(map))?])
        }

        fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::EnumAccess<'de>,
        {
            Ok(vec![T::deserialize(EnumAccessDeserializer::new(data))?])
        }

        forward!(visit_bool(bool));
        forward!(visit_i8(i8));
        forward!(visit_i16(i16));
        forward!(visit_i32(i32));
        forward!(visit_i64(i64));
        forward!(visit_i128(i128));
        forward!(visit_u8(u8));
        forward!(visit_u16(u16));
        forward!(visit_u32(u32));
        forward!(visit_u64(u64));
        forward!(visit_u128(u128));
        forward!(visit_f32(f32));
        forward!(visit_f64(f64));
        forward!(visit_char(char));
        forward!(visit_str(&str));
        forward!(visit_borrowed_str(&'de str));
        forward!(visit_string(String));
        forward!(visit_bytes(&[u8]));
        forward!(visit_borrowed_bytes(&'de [u8]));
        forward!(visit_byte_buf(Vec<u8>));
    }

    deserializer.deserialize_any(Visitor(PhantomData))
}

pub trait FieldDocs {
    fn field_docs() -> Vec<FieldDescription>;
}

#[derive(Debug, Clone, Copy)]
pub struct FieldDescription {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static [&'static str],
}

#[macro_export]
macro_rules! de_struct {
    (@derive $(#[$struct_att_:meta])* [$(($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)))*] []) => {$(
        de_struct!(@field_docs $name [$($body)*] [] [$($body)*]);
        de_struct!(@deserialize $(#[$struct_attr])* $name ($($body)*));
    )*};
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)*] [$($rest)*]);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)*] []);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*, $($rest)*]);
    };
    (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*]);
    };

    (@deserialize $(#[$struct_attr:meta])* $name:ident ($($body:tt)*)) => {
        #[automatically_derived]
        #[allow(non_camel_case_types)]
        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                de_struct!(@define $(#[$struct_attr])* $name [] [] [$($body)*]);
                let de_struct!(@unpack $name [] [$($body)*]) = ::serde::Deserialize::deserialize(deserializer)?;
                #[allow(clippy::redundant_field_names)]
                Ok(de_struct!(@result Self [] [$($body)*]))
            }
        }
    };

    (@define $(#[$struct_attr:meta])* $name:ident [$(($(#[$field_attr:meta])* $field:ident $type:ty))*] [$($infer:ident)*] []) => {
        #[derive(::serde::Deserialize)]
        $(#[$struct_attr])*
        struct $name<$($infer),*> {
            $($(#[$field_attr])* $field: $type),*
        }
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident, $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] [$($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] []);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty, $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] [$($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] []);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*, $($rest)*]);
    };
    (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*]);
    };

    (@unpack $name:ident [$($field:ident)*] []) => {
        $name { $($field),* }
    };
    (@unpack $name:ident [$($field:ident)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@unpack $name [$($field)* $next] [$($rest)*])
    };
    (@unpack $name:ident [$($field:ident)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?]) => {
        de_struct!(@unpack $name [$($field)* $next] [])
    };
    (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@unpack $name [$($field)*] [$($body)*, $($rest)*])
    };
    (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@unpack $name [$($field)*] [$($body)*])
    };

    (@result $name:ident [$(($field:ident: $($value:tt)*))*] []) => {
        $name {
            $($field: $($value)*),*
        }
    };
    (@result $name:ident [$($item:tt)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@result $name [$($item)* ($next: $next)] [$($rest)*])
    };
    (@result $name:ident [$($item:tt)*] [$(#[$attr:meta])* $next:ident $(as $type:ty)?]) => {
        de_struct!(@result $name [$($item)* ($next: $next)] [])
    };
    (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [$($rest)*])
    };
    (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [])
    };

    (@field_docs $name:ident [$($orig:tt)*] [$(($field:ident, [$($attr:tt)*]))*] []) => {
        #[automatically_derived]
        impl $crate::config::FieldDocs for $name {
            fn field_docs() -> Vec<$crate::config::FieldDescription> {
                fn type_name<T>(_: Option<T>) -> &'static str {
                    ::std::any::type_name::<T>()
                }
                let ($($field),*) = if let Some(de_struct!(@result Self [] [$($orig)*])) = None {
                    ($(Some($field)),*)
                } else {
                    ($({let $field = None; $field}),*)
                };
                vec![$($crate::config::FieldDescription {
                    name: stringify!($field),
                    ty: type_name($field),
                    doc: de_struct!(@doc_string [] [$($attr)*]),
                }),*]
            }
        }
    };
    (@field_docs $name:ident [$($orig:tt)*] [$($field:tt)*] [$(#[$($attr:tt)*])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
        de_struct!(@field_docs $name [$($orig)*] [$($field)* ($next, [$([$($attr)*])*])] [$($rest)*]);
    };
    (@field_docs $name:ident [$($orig:tt)*] [$($field:tt)*] [$(#[$($attr:tt)*])* $next:ident $(as $type:ty)?]) => {
        de_struct!(@field_docs $name [$($orig)*] [$($field)* ($next, [$([$($attr)*])*])] []);
    };
    (@field_docs $name:ident [$($orig:tt)*] [$($field:tt)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
        de_struct!(@field_docs $name [$($orig)*] [$($field)*] [$($body)*, $($rest)*]);
    };
    (@field_docs $name:ident [$($orig:tt)*] [$($field:tt)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
        de_struct!(@field_docs $name [$($orig)*] [$($field)*] [$($body)*]);
    };

    (@doc_string [$($doc:expr)*] []) => {
        &[$($doc),*]
    };
    (@doc_string [$($doc:expr)*] [[doc = $next:expr] $([$($rest:tt)*])*]) => {
        de_struct!(@doc_string [$($doc)* $next] [$([$($rest)*])*])
    };
    (@doc_string [$($doc:expr)*] [[$attr:meta] $([$($rest:tt)*])*]) => {
        de_struct!(@doc_string [$($doc)*] [$([$($rest)*])*])
    };

    ($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)) => {
        de_struct!(@derive $(#[$struct_attr])* [($(#[$struct_attr])* $name ($($body)*))] [$($body)*]);
    };
}
