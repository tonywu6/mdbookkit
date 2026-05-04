use std::{
    borrow::Cow,
    fmt::Debug,
    ops::Deref,
    path::Path,
    process::{self, Command},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use serde::{
    Deserialize, Deserializer,
    de::{IntoDeserializer, value::MapAccessDeserializer},
};
use shlex::Shlex;
use tap::Pipe;
use tracing::debug;

use mdbookkit::{
    config::value_or_vec,
    de_struct, emit_error,
    env::is_ci,
    error::{Break, OnWarning},
    url::{UrlPath, UrlUtil},
};

use crate::subprocess::CommandUtil;

de_struct!(
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    Config(
        builder(BuilderConfig(
            #[serde(default)]
            manifest_dir,
            #[serde(default, deserialize_with = "value_or_vec")]
            build as Vec<Builder>,
            #[serde(default)]
            build_options
        )),
        env(EnvConfig(
            #[serde(default, deserialize_with = "base_url_config")]
            base_url as BaseUrlConfig
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
            features(FeatureSelection(
                #[serde(default)]
                features,
                #[serde(default)]
                all_features,
                #[serde(default)]
                no_default_features
            )),
            #[serde(default, deserialize_with = "command_line_args")]
            rustc_args as Vec<String>,
            #[serde(default, deserialize_with = "command_line_args")]
            rustdoc_args as Vec<String>,
            cargo(CargoOptions(
                #[serde(default)]
                toolchain,
                #[serde(default, deserialize_with = "command_line_args")]
                cargo_args as Vec<String>,
                #[serde(default)]
                runner
            )),
            #[serde(default)]
            docs_rs
        ))
    )
);

#[derive(Debug, Default)]
pub struct Config {
    pub builder: BuilderConfig,
    pub env: EnvConfig,
    pub fail_on_warnings: OnWarning,
}

#[derive(Debug, Default)]
pub struct BuilderConfig {
    manifest_dir: Option<Utf8PathBuf>,
    build: Vec<Builder>,
    build_options: BuildOptions,
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
    pub features: FeatureSelection,

    pub rustc_args: Vec<String>,
    pub rustdoc_args: Vec<String>,

    pub cargo: CargoOptions,
    pub docs_rs: Option<bool>,
}

#[derive(Debug, Default)]
pub struct FeatureSelection {
    pub features: Vec<String>,
    pub all_features: Option<bool>,
    pub no_default_features: Option<bool>,
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

#[derive(Debug, Default)]
pub struct CargoOptions {
    pub toolchain: Option<String>,
    pub cargo_args: Vec<String>,
    pub runner: CommandRunner,
}

#[derive(Debug, Default)]
pub struct EnvConfig {
    pub base_url: BaseUrlConfig,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BaseUrlConfig {
    #[serde(default)]
    dev: BaseUrl,
    #[serde(default)]
    release: BaseUrl,
}

#[derive(Debug, Clone)]
pub struct BaseUrl(pub UrlPath);

#[derive(Debug)]
pub struct BuildConfigResolved {
    pub manifest_dir: Utf8PathBuf,
    pub builders: Vec<Builder>,
}

impl BuilderConfig {
    pub fn resolve(self, book_dir: &Utf8Path) -> Result<BuildConfigResolved, Break> {
        let Self {
            manifest_dir,
            build,
            build_options,
        } = self;

        let builders = if build.is_empty() {
            vec![Default::default()]
        } else {
            build
        }
        .into_iter()
        .map(|mut builder| {
            builder.options.assign(&build_options);
            builder
        })
        .collect::<Vec<_>>();

        let default_cargo = if builders.len() == 1 {
            &builders[0].options.cargo
        } else {
            &build_options.cargo
        };

        // https://github.com/rust-lang/cargo/issues/16834
        let manifest_dir = if let Some(dir) = manifest_dir {
            book_dir
                .join(dir)
                .canonicalize_utf8()
                .context("failed to resolve `manifest-dir` to an absolute path")
                .or_else(emit_error!())?
        } else {
            default_cargo
                .workspace(book_dir.as_std_path())
                .context("this preprocessor will run `cargo doc`, which requires a Cargo project")
                .context("failed to find a Cargo project")
                .or_else(emit_error!())?
                .directory()
                .to_owned()
        };

        debug!("resolved manifest dir: {manifest_dir}");

        Ok(BuildConfigResolved {
            manifest_dir,
            builders,
        })
    }
}

impl BuildOptions {
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
                features: _,
                rustc_args,
                rustdoc_args,
                cargo: _,
                docs_rs,
            } = other;
            extend!(self, packages);
            extend!(self, preludes?);
            extend!(self, rustc_args);
            extend!(self, rustdoc_args);
            assign_if!(self, docs_rs, is_none);
        }
        {
            let FeatureSelection {
                features,
                all_features,
                no_default_features,
            } = &other.features;
            assign_if!(self.features, all_features, is_none);
            assign_if!(self.features, no_default_features, is_none);
            extend!(self.features, features);
        }
        {
            let CargoOptions {
                toolchain,
                cargo_args,
                runner,
            } = &other.cargo;
            assign_if!(self.cargo, toolchain, is_none);
            extend!(self.cargo, cargo_args);
            assign_if!(self.cargo, runner, is_undefined);
        }
    }
}

impl FeatureSelection {
    pub fn list(&self) -> &[String] {
        &self.features
    }

    pub fn all_features(&self) -> bool {
        self.all_features.unwrap_or(false)
    }

    pub fn no_default_features(&self) -> bool {
        self.no_default_features.unwrap_or(false)
    }
}

impl FromStr for BaseUrl {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut pat = s.parse::<UrlPath>()?;
        pat.ensure_trailing_slash();
        Ok(Self(pat))
    }
}

impl Default for BaseUrl {
    fn default() -> Self {
        // https://doc.rust-lang.org/cargo/reference/unstable.html#rustdoc-map
        "https://docs.rs/{pkg_name}/{version}"
            .parse()
            .expect("should be valid")
    }
}

impl<'de> Deserialize<'de> for BaseUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        (String::deserialize(deserializer)?.parse::<Self>())
            .map_err(|err| serde::de::Error::custom(format_args!("{err:?}")))
    }
}

fn base_url_config<'de, D>(deserializer: D) -> Result<BaseUrlConfig, D::Error>
where
    D: Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = BaseUrlConfig;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a map")
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            Self::Value::deserialize(MapAccessDeserializer::new(map))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let url = BaseUrl::deserialize(v.into_deserializer())?;
            Ok(Self::Value {
                dev: url.clone(),
                release: url.clone(),
            })
        }
    }

    deserializer.deserialize_any(Visitor)
}

impl Deref for BaseUrlConfig {
    type Target = BaseUrl;

    fn deref(&self) -> &Self::Target {
        if is_ci().is_some() {
            &self.release
        } else {
            &self.dev
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

    fn workspace(&self, cwd: &Path) -> Result<LocateProject> {
        self.command("locate-project")
            .arg("--message-format=json")
            .arg("--workspace")
            .current_dir(cwd)
            .run()
            .checked()
            .context("`cargo locate-project` did not run successfully")?
            .pipe(LocateProject::parse)
            .context("could not parse output of `cargo locate-project`")
    }
}

#[derive(Deserialize)]
struct LocateProject {
    root: Utf8PathBuf,
}

impl LocateProject {
    fn directory(&self) -> &Utf8Path {
        (self.root.parent()).expect("path to Cargo.toml should have a parent")
    }

    fn parse(output: process::Output) -> Result<Self> {
        let process::Output { stdout, .. } = output;
        let output = String::from_utf8(stdout)?;
        Ok(serde_json::from_str(&output)?)
    }
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

        let program = (args.next())
            .context("unexpected empty command")
            .map_err(|e| serde::de::Error::custom(format_args!("{e:?}")))?;

        let mut cmd = Command::new(program);

        cmd.args(args);

        Ok(Self(cmd))
    }
}

fn command_line_args<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
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
        CommandLineArgs::String(args) => {
            let mut shlex = Shlex::new(&args);
            let parsed = shlex.by_ref().collect();
            if !shlex.had_error {
                Ok(parsed)
            } else {
                let error = anyhow!("parsed: {parsed:?}")
                    .context(format!("string: {args:?}"))
                    .context("malformed command line args");
                Err(serde::de::Error::custom(format_args!("{error:?}")))
            }
        }
        CommandLineArgs::Array(args) => Ok(args),
    }
}

impl Builder {
    pub fn debug(&self) -> impl Debug {
        struct DebugBuilder<'a>(&'a Builder);
        return DebugBuilder(self);

        impl Debug for DebugBuilder<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut f = f.debug_list();

                let Builder { targets, options } = self.0;
                let BuildOptions {
                    packages,
                    preludes,
                    features:
                        FeatureSelection {
                            features,
                            all_features,
                            no_default_features,
                        },
                    rustc_args,
                    rustdoc_args,
                    cargo:
                        CargoOptions {
                            toolchain,
                            cargo_args,
                            runner,
                        },
                    docs_rs,
                } = options;

                let mut non_exhaustive = preludes.is_some()
                    || !rustc_args.is_empty()
                    || !rustdoc_args.is_empty()
                    || !cargo_args.is_empty()
                    || !runner.is_undefined();

                if matches!(docs_rs, Some(true)) {
                    f.entry(&format_args!("docs.rs"));
                }

                if let Some(toolchain) = toolchain {
                    f.entry(&format_args!("{toolchain}"));
                }

                if targets.is_empty() {
                    f.entry(&format_args!("default targets"));
                } else {
                    for t in &self.0.targets {
                        f.entry(&format_args!("{t}"));
                    }
                }

                if features.len() > 3 {
                    non_exhaustive = true
                }
                for feature in features.iter().take(3) {
                    f.entry(&feature);
                }

                if packages.len() > 3 {
                    non_exhaustive = true
                }
                for package in packages.iter().take(3) {
                    if let PackageSpec::Selector(PackageSelector {
                        name: Some(name), ..
                    })
                    | PackageSpec::Name(name) = package
                    {
                        f.entry(&name);
                    } else if let PackageSpec::Selector(PackageSelector {
                        workspace: WorkspaceMember::Default | WorkspaceMember::All,
                        ..
                    }) = package
                    {
                        f.entry(&format_args!("workspace members"));
                    } else {
                        non_exhaustive = true
                    }
                }

                if matches!(all_features, Some(true)) {
                    f.entry(&format_args!("all-features"));
                }
                if matches!(no_default_features, Some(true)) {
                    f.entry(&format_args!("no-default-features"));
                }

                if non_exhaustive {
                    f.entry(&format_args!("(additional options)"));
                    f.finish_non_exhaustive()
                } else {
                    f.finish()
                }
            }
        }
    }
}
