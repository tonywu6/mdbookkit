use std::{borrow::Cow, process::Command};

use cargo_metadata::camino::Utf8PathBuf;
use serde::Deserialize;

use mdbookkit::error::OnWarning;

use crate::de_struct;

use self::_serde::{command_line_args, value_or_vec};

de_struct!(
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    Config(
        build(BuildConfig(
            #[serde(default)]
            manifest_dir,
            #[serde(default, deserialize_with = "value_or_vec")]
            build as Vec<Builder>,
            #[serde(default)]
            build_options
        )),
        #[serde(default)]
        fail_on_warnings
    )
);

de_struct!(
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    Builder(
        #[serde(default)]
        targets,
        options(BuildOptions(
            #[serde(default, deserialize_with = "value_or_vec")]
            packages as Vec<PackageSpec>,
            #[serde(default)]
            preludes,
            #[serde(default)]
            features,
            #[serde(default)]
            all_features,
            #[serde(default)]
            no_default_features,
            #[serde(default, deserialize_with = "command_line_args")]
            rustc_args as Vec<String>,
            #[serde(default, deserialize_with = "command_line_args")]
            rustdoc_args as Vec<String>,
            cargo(CargoOptions(
                #[serde(default)]
                toolchain,
                #[serde(default, deserialize_with = "command_line_args")]
                cargo_args as Vec<String>
            )),
            #[serde(default)]
            runner
        ))
    )
);

#[derive(Debug, Default)]
pub struct Config {
    pub build: BuildConfig,
    pub fail_on_warnings: OnWarning,
}

#[derive(Debug, Default)]
pub struct BuildConfig {
    pub manifest_dir: Option<Utf8PathBuf>,
    pub build: Vec<Builder>,
    pub build_options: BuildOptions,
}

#[derive(Debug, Default)]
pub struct Builder {
    pub targets: Vec<String>,
    pub options: BuildOptions,
}

/// <https://github.com/rust-lang/docs.rs/blob/c173de9/crates/lib/metadata/lib.rs#L103-L147>
#[derive(Debug, Default)]
pub struct BuildOptions {
    pub packages: Vec<PackageSpec>,
    pub preludes: Option<Vec<String>>,

    pub features: Vec<String>,
    pub all_features: Option<bool>,
    pub no_default_features: Option<bool>,

    pub rustc_args: Vec<String>,
    pub rustdoc_args: Vec<String>,

    pub cargo: CargoOptions,
    pub runner: CommandRunner,
}

#[derive(Debug, Default)]
pub struct CargoOptions {
    pub toolchain: Option<String>,
    pub cargo_args: Vec<String>,
}

impl BuildOptions {
    pub fn vary(&self) -> bool {
        let Self {
            packages: _,
            preludes: _,
            features,
            all_features,
            no_default_features,
            rustc_args: _,
            rustdoc_args: _,
            cargo: _,
            runner,
        } = &self;
        !features.is_empty()
            || all_features.is_some()
            || no_default_features.is_some()
            || !runner.is_undefined()
    }

    pub fn assign(&mut self, other: &Self) {
        macro_rules! assign_if {
            ( $lhs:expr, $value:ident, $empty:ident ) => {
                if $lhs.$value.$empty() {
                    $lhs.$value = $value.clone();
                }
            };
        }
        macro_rules! extend {
            ( $lhs:expr, $value:ident ) => {
                $lhs.$value.extend_from_slice(&$value);
            };
            ( $lhs:expr, $value:ident ? ) => {
                if let Some(rhs) = $value {
                    if let Some(ref mut lhs) = $lhs.$value {
                        lhs.extend_from_slice(rhs);
                    } else {
                        $lhs.$value = $value.clone();
                    }
                }
            };
        }
        {
            let Self {
                packages,
                preludes,
                features,
                all_features,
                no_default_features,
                rustc_args,
                rustdoc_args,
                cargo: _,
                runner,
            } = other;
            assign_if!(self, all_features, is_none);
            assign_if!(self, no_default_features, is_none);
            extend!(self, packages);
            extend!(self, preludes?);
            extend!(self, features);
            extend!(self, rustc_args);
            extend!(self, rustdoc_args);
            assign_if!(self, runner, is_undefined);
        }
        {
            let CargoOptions {
                toolchain,
                cargo_args,
            } = &other.cargo;
            assign_if!(self.cargo, toolchain, is_none);
            extend!(self.cargo, cargo_args);
        }
    }
}

impl CargoOptions {
    pub fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new("cargo");
        command
            .args(self.toolchain())
            .arg(subcommand)
            .args(&self.cargo_args);
        command
    }

    pub fn toolchain(&self) -> Option<String> {
        self.toolchain.as_ref().map(|t| format!("+{t}"))
    }
}

#[derive(Debug, Clone)]
pub enum PackageSpec {
    Name(String),
    Selector(PackageSelector),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PackageSelector {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub workspace: WorkspaceMember,
    #[serde(default)]
    pub dependencies: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum WorkspaceMember {
    None,
    #[default]
    Default,
    All,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct CommandRunner(Option<CustomCommand>);

impl CommandRunner {
    pub fn command(&self, args: Command) -> Command {
        let Some(CustomCommand(runner)) = &self.0 else {
            return args;
        };

        let mut command = Command::new(runner.get_program());

        for arg in runner.get_args() {
            match arg.to_string_lossy().as_bytes() {
                b"$*" => {
                    let args = std::iter::once(args.get_program())
                        .chain(args.get_args())
                        .map(|s| s.to_string_lossy())
                        .collect::<Vec<_>>();
                    let args = shlex::try_join(args.iter().map(Cow::as_ref))
                        .expect("args should not have null bytes");
                    command.arg(args);
                }
                b"$@" => {
                    command.arg(args.get_program()).args(args.get_args());
                }
                _ => {
                    command.arg(arg);
                }
            }
        }

        if let Some(dir) = args.get_current_dir() {
            command.current_dir(dir);
        }

        for (k, v) in args.get_envs() {
            match v {
                Some(v) => command.env(k, v),
                None => command.env_remove(k),
            };
        }

        command
    }

    pub fn is_undefined(&self) -> bool {
        self.0.is_none()
    }
}

#[derive(Debug)]
pub struct CustomCommand(pub Command);

impl Clone for CustomCommand {
    fn clone(&self) -> Self {
        let mut cmd = Command::new(self.0.get_program());

        cmd.args(self.0.get_args());

        if let Some(dir) = self.0.get_current_dir() {
            cmd.current_dir(dir);
        }

        for (k, v) in self.0.get_envs() {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }

        Self(cmd)
    }
}

mod _serde {
    use std::{marker::PhantomData, process::Command};

    use anyhow::Context;
    use serde::{
        Deserialize, Deserializer,
        de::value::{EnumAccessDeserializer, MapAccessDeserializer, SeqAccessDeserializer},
    };
    use shlex::Shlex;

    use super::{CustomCommand, PackageSpec, WorkspaceMember};

    impl<'de> Deserialize<'de> for WorkspaceMember {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct Visitor;

            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = WorkspaceMember;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str(r#""default" or "all""#)
                }

                fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    if v {
                        Ok(WorkspaceMember::Default)
                    } else {
                        Ok(WorkspaceMember::None)
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    match v {
                        "default" => Ok(WorkspaceMember::Default),
                        "all" => Ok(WorkspaceMember::All),
                        "none" => Ok(WorkspaceMember::None),
                        _ => Err(E::unknown_variant(v, &["default", "all", "none"])),
                    }
                }
            }

            deserializer.deserialize_any(Visitor)
        }
    }

    impl<'de> Deserialize<'de> for PackageSpec {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct Visitor;

            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = PackageSpec;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("package spec")
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(PackageSpec::Name(v.to_owned()))
                }

                fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(PackageSpec::Name(v))
                }

                fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
                where
                    A: serde::de::MapAccess<'de>,
                {
                    let selector = Deserialize::deserialize(MapAccessDeserializer::new(map))?;
                    Ok(PackageSpec::Selector(selector))
                }
            }

            deserializer.deserialize_any(Visitor)
        }
    }

    impl<'de> Deserialize<'de> for CustomCommand {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let mut args = command_line_args(deserializer)?.into_iter();

            let program = args
                .next()
                .context("unexpected empty command")
                .map_err(serde::de::Error::custom)?;

            let mut cmd = Command::new(program);

            cmd.args(args);

            Ok(Self(cmd))
        }
    }

    pub fn command_line_args<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CommandLineArgs {
            String(String),
            Array(Vec<String>),
        }
        match CommandLineArgs::deserialize(deserializer)? {
            CommandLineArgs::String(args) => Ok(Shlex::new(&args).collect()),
            CommandLineArgs::Array(args) => Ok(args),
        }
    }

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

    #[macro_export]
    macro_rules! de_struct {
        (@derive $(#[$struct_att_:meta])* [$(($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)))*] []) => {$(
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
        )*};
        (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?]) => {
            de_struct!(@derive $(#[$struct_attr])* [$($item)*] []);
        };
        (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$(#[$field_attr:meta])* $field:ident $(as $type:ty)?, $($rest:tt)*]) => {
            de_struct!(@derive $(#[$struct_attr])* [$($item)*] [$($rest)*]);
        };
        (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*))]) => {
            de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*]);
        };
        (@derive $(#[$struct_attr:meta])* [$($item:tt)*] [$_:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
            de_struct!(@derive $(#[$struct_attr])* [$($item)* ($(#[$struct_attr])* $inner($($body)*))] [$($body)*, $($rest)*]);
        };

        (@define $(#[$struct_attr:meta])* $name:ident [$(($(#[$field_attr:meta])* $field:ident $type:ty))*] [$($infer:ident)*] []) => {
            #[derive(::serde::Deserialize)]
            $(#[$struct_attr])*
            struct $name<$($infer),*> {
                $($(#[$field_attr])* $field: $type),*
            }
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] []);
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident, $($rest:tt)*]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $next)] [$($infer)* $next] [$($rest)*]);
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] []);
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$(#[$field_attr:meta])* $next:ident as $type:ty, $($rest:tt)*]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))* ($(#[$field_attr])* $next $type)] [$($infer)*] [$($rest)*]);
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*]);
        };
        (@define $(#[$struct_attr:meta])* $name:ident [$(($($field:tt)*))*] [$($infer:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
            de_struct!(@define $(#[$struct_attr])* $name [$(($($field)*))*] [$($infer)*]  [$($body)*, $($rest)*]);
        };

        (@unpack $name:ident [$($field:ident)*] []) => {
            $name { $($field),* }
        };
        (@unpack $name:ident [$($field:ident)*] [$(#[$field_attr:meta])* $next:ident $(as $type:ty)?]) => {
            de_struct!(@unpack $name [$($field)* $next] [])
        };
        (@unpack $name:ident [$($field:ident)*] [$(#[$field_attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
            de_struct!(@unpack $name [$($field)* $next] [$($rest)*])
        };
        (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
            de_struct!(@unpack $name [$($field)*] [$($body)*])
        };
        (@unpack $name:ident [$($field:ident)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
            de_struct!(@unpack $name [$($field)*] [$($body)*, $($rest)*])
        };

        (@result $name:ident [$(($field:ident: $($value:tt)*))*] []) => {
            $name {
                $($field: $($value)*),*
            }
        };
        (@result $name:ident [$($item:tt)*] [$(#[$field_attr:meta])* $next:ident $(as $type:ty)?]) => {
            de_struct!(@result $name [$($item)* ($next: $next)] [])
        };
        (@result $name:ident [$($item:tt)*] [$(#[$field_attr:meta])* $next:ident $(as $type:ty)?, $($rest:tt)*]) => {
            de_struct!(@result $name [$($item)* ($next: $next)] [$($rest)*])
        };
        (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*))]) => {
            de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [])
        };
        (@result $name:ident [$($item:tt)*] [$next:ident ($inner:ident ($($body:tt)*)), $($rest:tt)*]) => {
            de_struct!(@result $name [$($item)* ($next: de_struct!(@result $inner [] [$($body)*]))] [$($rest)*])
        };

        ($(#[$struct_attr:meta])* $name:ident ($($body:tt)*)) => {
            de_struct!(@derive $(#[$struct_attr])* [($(#[$struct_attr])* $name ($($body)*))] [$($body)*]);
        };
    }
}
