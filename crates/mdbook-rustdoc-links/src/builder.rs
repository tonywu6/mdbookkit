use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsStr,
    fmt::Debug,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Output, Stdio},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{
    PackageId,
    camino::{Utf8Path, Utf8PathBuf},
    diagnostic::Diagnostic,
};
use serde::Deserialize;
use tap::{Pipe, Tap};
use tempfile::TempDir;
use tracing::Level;

use mdbookkit::{emit_warning, env::is_logging, error::IntoAnyhow, ticker, ticker_event};

use crate::{
    options::{
        BuildConfig, BuildOptions, Builder, CommandRunner, PackageSelector, PackageSpec,
        WorkspaceMember,
    },
    tracker::LinkTracker,
};

pub fn build_docs(config: BuildConfig, tracker: &mut LinkTracker) -> Result<()> {
    let BuildConfig {
        manifest_dir,
        build,
        build_options,
    } = config;

    // https://github.com/rust-lang/cargo/issues/16834
    let manifest_dir = if let Some(dir) = manifest_dir {
        Some(dir)
    } else if let Ok(workspace) = LocateProject::workspace()
        .context("Could not determine the current workspace root")
        .inspect_err(emit_warning!())
    {
        Some(workspace.directory().to_owned())
    } else {
        None
    };

    let workspace = (build_options.cargo)
        .command("metadata")
        .cwd(manifest_dir.as_ref())
        .output()
        .checked()?
        .into_cargo_metadata()?;

    let builds = if build.is_empty() {
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

    for builder in builds {
        let ctx = BuildContext {
            workspace: &workspace,
            builder,
            tracker,
        };
        run_builder(ctx)?;
    }

    Ok(())
}

struct BuildContext<'a, 'r> {
    workspace: &'a cargo_metadata::Metadata,
    builder: Builder,
    tracker: &'a mut LinkTracker<'r>,
}

fn run_builder(ctx: BuildContext) -> Result<()> {
    let BuildContext {
        workspace,
        builder: Builder { targets, options },
        tracker,
    } = ctx;

    let BuildOptions {
        all_features,
        no_default_features,
        ..
    } = options;

    let all_features = all_features.unwrap_or(false);
    let no_default_features = no_default_features.unwrap_or(false);

    let build_workspace = if options.vary() {
        let BuildOptions {
            ref features,
            ref cargo,
            ref runner,
            ..
        } = options;
        cargo
            .command("metadata")
            .options("--format-version", ["1"])
            .options("--features", features)
            .flag("--all-features", all_features)
            .flag("--no-default-features", no_default_features)
            .runner(runner)
            .current_dir(&workspace.workspace_root)
            .output()
            .checked()?
            .into_cargo_metadata()?
            .pipe(Cow::Owned)
    } else {
        Cow::Borrowed(workspace)
    };

    let BuildOptions {
        packages,
        preludes,
        features,
        rustc_args,
        rustdoc_args,
        cargo,
        runner,
        all_features: _,
        no_default_features: _,
    } = options;

    let packages = packages
        .into_iter()
        .flat_map(|spec| resolve_packages(spec, &build_workspace))
        .collect::<Vec<_>>();

    let preludes = if let Some(preludes) = preludes {
        preludes
    } else if build_workspace.workspace_default_packages().len() == 1
        && let Some(pkg) = build_workspace.workspace_default_packages().first()
        && let Some(lib) = pkg.targets.iter().find_map(|t| {
            if t.is_lib() || t.is_dylib() || t.is_proc_macro() || t.is_rlib() {
                Some(format!("{}::*", t.name))
            } else {
                None
            }
        })
    {
        vec![lib]
    } else {
        vec![]
    };

    let path_mapper = PathMapper::new(
        workspace,
        if runner.is_undefined() {
            None
        } else {
            Some(&build_workspace)
        },
    );

    let rustc_args = if !rustc_args.is_empty() {
        let flags = toml::Value::from(rustc_args);
        Some(format!("build.rustflags={flags}"))
    } else {
        None
    };

    let rustdoc_args = if !rustdoc_args.is_empty() {
        let flags = toml::Value::from(rustdoc_args);
        Some(format!("build.rustdocflags={flags}"))
    } else {
        None
    };

    let mut artifacts = CompilerOutput::new(path_mapper);

    let progress = CargoProgress::new();

    let mut proc = cargo
        .command("doc")
        .options("--message-format", ["json"])
        .options("--target", &targets)
        .flag("--no-deps", !packages.is_empty())
        .options("--package", &packages)
        .options("--features", &features)
        .flag("--all-features", all_features)
        .flag("--no-default-features", no_default_features)
        .options("--config", &rustc_args)
        .options("--config", &rustdoc_args)
        .options("--config", progress.cargo_options)
        .runner(&runner)
        .current_dir(&workspace.workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let ticker = ticker!(Level::INFO, "cargo-doc", "running cargo doc");
    progress.ticker(ticker, &mut proc);

    artifacts.update(proc)?;

    let mut proc = cargo
        .command("check")
        .options("--message-format", ["json"])
        .options("--target", &targets)
        .options("--package", &packages)
        .options("--features", &features)
        .flag("--all-features", all_features)
        .flag("--no-default-features", no_default_features)
        .options("--config", &rustc_args)
        .options("--config", &rustdoc_args)
        .options("--config", progress.cargo_options)
        .runner(&runner)
        .current_dir(&workspace.workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let ticker = ticker!(Level::INFO, "cargo-check", "running cargo check");
    progress.ticker(ticker, &mut proc);

    artifacts.update(proc)?;

    for target in artifacts.targets() {
        let Some(docstring) = tracker.rustdoc_input() else {
            break;
        };

        let tempdir = TempDir::new_in(&workspace.target_directory)?;

        let mut rustdoc = Command::new("rustdoc")
            .values(cargo.toolchain())
            .options("--target", target.as_deref())
            .options("--out-dir", [tempdir.path()])
            .options("--edition", ["2024"])
            .options("--crate-type", ["lib"])
            .options("--error-format", ["json"])
            .values(["-"]);

        let mut library_paths = HashSet::new();

        let mut crate_name = 0;
        macro_rules! crate_name {
            () => {
                format!("temporary_crate_{crate_name}")
            };
        }

        for name in artifacts.crates() {
            if name == crate_name!() {
                crate_name += 1;
            }

            if let Some(source) = (artifacts.get_doc(name, &target))
                .as_ref()
                .and_then(|dir| dir.parent())
            {
                let target = tempdir.path().join(name);
                symlink_dir_all(source, target)?;
            }

            if let Some(file) = artifacts.get_lib(name, &target) {
                rustdoc.arg("--extern").arg(format!("{name}={file}"));

                if let Some(parent) = file.parent()
                    && !library_paths.contains(parent)
                {
                    library_paths.insert(parent.to_owned());
                }
            }
        }

        for path in library_paths {
            rustdoc.arg("-L").arg(path);
        }

        let mut rustdoc = rustdoc
            .options("--crate-name", [crate_name!()])
            .runner(&runner)
            .current_dir(&workspace.workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            let mut stdin = rustdoc.stdin.take().expect("should have stdio");
            writeln!(stdin, "{docstring}")?;
            for prelude in preludes.iter() {
                writeln!(stdin, "use {prelude};")?;
            }
        }

        let output = rustdoc.wait_with_output()?;

        if !output.status.success() {
            return Err(rustc_json_error(output).context("Failed to run `rustdoc`"));
        }

        let output = BuildOutput {
            workspace,
            crates: &artifacts.crates,
            stdout: {
                let path = tempdir.path().join(crate_name!()).join("index.html");
                std::fs::read_to_string(path)?
            },
            stderr: String::from_utf8(output.stderr)?,
        };

        tracker.rustdoc_output(output)?;
    }

    Ok(())
}

pub struct BuildOutput<'a> {
    pub workspace: &'a cargo_metadata::Metadata,
    pub crates: &'a BTreeMap<Arc<str>, PackageId>,
    pub stdout: String,
    pub stderr: String,
}

struct CompilerOutput {
    paths: PathMapper,
    targets: BTreeSet<Arc<str>>,
    crates: BTreeMap<Arc<str>, PackageId>,
    libs: ArtifactMap,
    docs: ArtifactMap,
}

#[derive(Default)]
struct ArtifactMap(HashMap<Arc<str>, HashMap<Option<Arc<str>>, Utf8PathBuf>>);

impl CompilerOutput {
    fn new(paths: PathMapper) -> Self {
        Self {
            paths,
            targets: Default::default(),
            crates: Default::default(),
            libs: Default::default(),
            docs: Default::default(),
        }
    }

    fn update(&mut self, mut proc: std::process::Child) -> Result<()> {
        let mut success = false;

        for msg in (proc.stdout.take())
            .expect("should have stdio")
            .pipe(BufReader::new)
            .pipe(cargo_metadata::Message::parse_stream)
        {
            match msg? {
                cargo_metadata::Message::CompilerArtifact(artifact) => {
                    self.update_unit(artifact);
                }
                cargo_metadata::Message::BuildFinished(finished) => {
                    success = finished.success;
                }
                _ => {}
            }
        }

        proc.wait_with_output().checked()?;

        if !success {
            bail!("cargo process did not succeed")
        }

        Ok(())
    }

    fn update_unit(&mut self, artifact: cargo_metadata::Artifact) {
        let cargo_metadata::Artifact {
            package_id,
            target: cargo_metadata::Target { name, kind, .. },
            filenames,
            ..
        } = artifact;

        match kind.first() {
            Some(cargo_metadata::TargetKind::ProcMacro) => {}
            Some(cargo_metadata::TargetKind::Lib) => {}
            Some(cargo_metadata::TargetKind::RLib) => {}
            Some(cargo_metadata::TargetKind::DyLib) => {}
            Some(cargo_metadata::TargetKind::StaticLib) => {}
            _ => return,
        }

        let name = Arc::<str>::from(name);
        self.crates.insert(name.clone(), package_id);

        for path in filenames {
            self.update_file(name.clone(), &path)
                .inspect_err(emit_warning!())
                .ok();
        }
    }

    fn update_file(&mut self, name: Arc<str>, path: &Utf8Path) -> Result<()> {
        let path = self.paths.relative_path(path)?;

        match path.extension() {
            Some("html") if path.file_name() == Some("index.html") => {
                let target = (path.components())
                    .nth_back(3)
                    .map(|dir| self.put_target(dir.as_str()));

                (self.docs.0.entry(name))
                    .or_default()
                    .insert(target, path.to_owned());
            }

            Some(kind @ ("rmeta" | "rlib" | "so" | "dylib" | "dll")) => {
                let target = match path.parent().and_then(|dir| dir.file_name()) {
                    // build dir v2
                    // https://blog.rust-lang.org/2026/03/13/call-for-testing-build-dir-layout-v2/
                    Some("out") => path.components().nth_back(6),
                    // build dir v1
                    Some("deps") => path.components().nth_back(3),
                    _ => bail!("unsupported path pattern {path:?}"),
                }
                .map(|dir| self.put_target(dir.as_str()));

                if kind == "rmeta" {
                    (self.libs.0.entry(name))
                        .or_default()
                        .entry(target)
                        .or_insert(path.to_owned());
                } else {
                    (self.libs.0.entry(name))
                        .or_default()
                        .insert(target, path.to_owned());
                }
            }

            _ => {}
        }

        Ok(())
    }

    fn put_target(&mut self, target: &str) -> Arc<str> {
        if let Some(target) = self.targets.get(target) {
            target.clone()
        } else {
            let target = Arc::<str>::from(target);
            self.targets.insert(target.clone());
            target
        }
    }

    fn targets(&self) -> Vec<Option<Arc<str>>> {
        if self.targets.is_empty() {
            vec![None]
        } else {
            self.targets.iter().cloned().map(Some).collect()
        }
    }

    fn crates(&self) -> impl Iterator<Item = &str> {
        self.crates.keys().map(|c| &**c)
    }

    fn get_doc(&self, name: &str, target: &Option<Arc<str>>) -> Option<Utf8PathBuf> {
        Some(self.paths.doc_path(self.docs.get(name, target)?))
    }

    fn get_lib(&self, name: &str, target: &Option<Arc<str>>) -> Option<Utf8PathBuf> {
        Some(self.paths.lib_path(self.libs.get(name, target)?))
    }
}

impl ArtifactMap {
    fn get(&self, name: &str, target: &Option<Arc<str>>) -> Option<&Utf8PathBuf> {
        let files = self.0.get(name)?;
        if target.is_some() {
            files.get(target).or_else(|| files.get(&None))
        } else {
            files.get(target)
        }
    }
}

struct PathMapper {
    host: WorkspacePaths,
    build: Option<WorkspacePaths>,
}

struct WorkspacePaths {
    target_dir: Utf8PathBuf,
    build_dir: Option<Utf8PathBuf>,
}

impl PathMapper {
    fn new(host: &cargo_metadata::Metadata, build: Option<&cargo_metadata::Metadata>) -> Self {
        Self {
            host: WorkspacePaths {
                target_dir: host.target_directory.clone(),
                build_dir: host.build_directory.clone(),
            },
            build: if let Some(cargo_metadata::Metadata {
                target_directory,
                build_directory,
                ..
            }) = build
            {
                Some(WorkspacePaths {
                    target_dir: target_directory.clone(),
                    build_dir: build_directory.clone(),
                })
            } else {
                None
            },
        }
    }

    fn relative_path<'a>(&self, path: &'a Utf8Path) -> Result<&'a Utf8Path> {
        let WorkspacePaths {
            target_dir,
            build_dir,
        } = self.build.as_ref().unwrap_or(&self.host);
        if let Some(root) = &build_dir
            && let Ok(path) = path.strip_prefix(root)
        {
            Ok(path)
        } else if let Ok(path) = path.strip_prefix(target_dir) {
            Ok(path)
        } else {
            bail!("{path:?} is not within workspace")
        }
    }

    fn doc_path(&self, path: &Utf8Path) -> Utf8PathBuf {
        self.host.target_dir.join(path)
    }

    fn lib_path(&self, path: &Utf8Path) -> Utf8PathBuf {
        let Self { host, build } = self;
        let root = if let Some(build) = build {
            build.build_dir.as_ref().unwrap_or(&build.target_dir)
        } else {
            host.build_dir.as_ref().unwrap_or(&host.target_dir)
        };
        root.join(path)
    }
}

fn resolve_packages(spec: PackageSpec, metadata: &cargo_metadata::Metadata) -> Vec<&String> {
    let pkg = match spec {
        PackageSpec::Name(name) => PackageSelector {
            name: Some(name),
            workspace: WorkspaceMember::None,
            dependencies: Default::default(),
        },
        PackageSpec::Selector(pkg) => pkg,
    };

    let (pkgs, deps) = match pkg {
        PackageSelector {
            name: Some(name),
            dependencies,
            ..
        } => {
            let pkgs = (metadata.packages.iter())
                .filter(|p| p.name == name)
                .collect::<Vec<_>>();
            (pkgs, dependencies)
        }
        PackageSelector {
            workspace,
            dependencies,
            ..
        } => {
            let pkgs = match workspace {
                WorkspaceMember::None => vec![],
                WorkspaceMember::Default => {
                    if metadata.workspace_default_members.is_available() {
                        metadata.workspace_default_packages()
                    } else {
                        metadata.workspace_packages()
                    }
                }
                WorkspaceMember::All => metadata.workspace_packages(),
            };
            (pkgs, dependencies)
        }
    };

    let tree = metadata.resolve.as_ref().expect("should have deps tree");

    let deps = if deps {
        pkgs.iter()
            .flat_map(|p| &tree[&p.id].deps)
            .filter_map(|p| {
                if let Some(cargo_metadata::DepKindInfo {
                    kind: cargo_metadata::DependencyKind::Normal,
                    ..
                }) = p.dep_kinds.first()
                {
                    Some(&*metadata[&p.pkg].name)
                } else {
                    None
                }
            })
            .collect()
    } else {
        vec![]
    };

    pkgs.iter()
        .map(|p| &*p.name)
        .chain(deps.iter().copied())
        .collect()
}

trait CargoMetadataUtil {
    fn into_cargo_metadata(self) -> Result<cargo_metadata::Metadata>;
}

impl CargoMetadataUtil for Output {
    fn into_cargo_metadata(self) -> Result<cargo_metadata::Metadata> {
        let stdout = String::from_utf8(self.stdout)?;
        Ok(cargo_metadata::MetadataCommand::parse(stdout)?)
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

    fn workspace() -> Result<Self> {
        std::process::Command::new("cargo")
            .arg("locate-project")
            .arg("--message-format=json")
            .arg("--workspace")
            .output()
            .checked()
            .context("`cargo locate-project` did not run successfully")?
            .pipe(Self::parse)
    }

    fn parse(output: Output) -> Result<Self> {
        let Output { stdout, .. } = output;
        (String::from_utf8(stdout).anyhow())
            .and_then(|output| serde_json::from_str(&output).anyhow())
            .context("Could not parse `cargo locate-project` output")
    }
}

struct CargoProgress {
    cargo_options: &'static [&'static str],
    line_ending: u8,
}

impl CargoProgress {
    fn new() -> Self {
        if is_logging() {
            Self {
                cargo_options: &["term.color = 'never'", "term.progress.when = 'never'"],
                line_ending: b'\n',
            }
        } else {
            Self {
                cargo_options: &[
                    "term.quiet = true",
                    "term.progress.when = 'always'",
                    "term.progress.width = 1024",
                ],
                line_ending: b'\r',
            }
        }
    }

    fn ticker(&self, ticker: tracing::Span, proc: &mut std::process::Child) {
        let stderr = proc.stderr.take().expect("should have stdio");
        let line_ending = self.line_ending;
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).split(line_ending) {
                let Ok(line) = line else { continue };
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                ticker_event!(&ticker, Level::INFO, "{line}");
            }
        });
    }
}

trait CommandUtil {
    fn values<I, S>(self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;

    fn options<I, J, S>(self, flag: &str, values: I) -> Self
    where
        I: IntoIterator<IntoIter = J>,
        J: ExactSizeIterator<Item = S>,
        S: AsRef<OsStr>;

    fn flag(self, flag: &str, enabled: bool) -> Self;

    fn cwd<P: AsRef<Path>>(self, dir: Option<P>) -> Self;

    fn runner(self, runner: &CommandRunner) -> Self;
}

impl CommandUtil for Command {
    fn values<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args(values);
        self
    }

    fn options<I, J, S>(mut self, flag: &str, values: I) -> Self
    where
        I: IntoIterator<IntoIter = J>,
        J: ExactSizeIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let values = values.into_iter();
        if values.len() == 0 {
            return self;
        }
        for value in values {
            self.arg(flag).arg(value);
        }
        self
    }

    fn flag(mut self, flag: &str, enabled: bool) -> Self {
        if enabled {
            self.arg(flag);
        }
        self
    }

    fn cwd<P: AsRef<Path>>(mut self, dir: Option<P>) -> Self {
        if let Some(dir) = dir {
            self.current_dir(dir);
        }
        self
    }

    fn runner(self, runner: &CommandRunner) -> Self {
        runner.command(self)
    }
}

trait CheckCommand {
    fn checked(self) -> Result<Output>;
}

impl CheckCommand for std::io::Result<Output> {
    fn checked(self) -> Result<Output> {
        let output = self?;
        if output.status.success() {
            Ok(output)
        } else {
            let status = anyhow!("command exited with code {}", output.status);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let error = if stderr.is_empty() {
                status
            } else {
                anyhow!(stderr).context(status)
            };
            Err(error)
        }
    }
}

fn rustc_json_error(output: Output) -> anyhow::Error {
    let status = anyhow!("command exited with code {}", output.status);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(stderr) = stderr
        .lines()
        .fold(None, |error, line| -> Option<anyhow::Error> {
            let text = if let Ok(diag) = serde_json::from_str::<Diagnostic>(line) {
                if let Some(rendered) = diag.rendered {
                    rendered
                } else {
                    diag.message
                }
                .trim()
                .to_owned()
            } else {
                line.to_owned()
            };
            if let Some(error) = error {
                Some(error.context(text))
            } else {
                Some(anyhow!(text))
            }
        })
    {
        stderr.context(status)
    } else {
        status
    }
}

fn symlink_dir_all(existing: impl AsRef<Path>, link: impl AsRef<Path>) -> std::io::Result<()> {
    if let Some(parent) = link.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    match symlink_dir(existing, link) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

fn symlink_dir(existing: impl AsRef<Path>, link: impl AsRef<Path>) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(existing, link);
    #[cfg(windows)]
    return std::os::windows::fs::symlink_dir(existing, link);
}

impl Debug for CompilerOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("CompilerOutput");

        let Self {
            paths,
            targets,
            crates,
            libs,
            docs,
        } = self;

        return f
            .field("targets", &targets)
            .field("crates", &crates)
            .field("libs", &SortedArtifacts::new(libs))
            .field("docs", &SortedArtifacts::new(docs))
            .field("paths", paths)
            .finish();

        struct SortedArtifacts<'a>(Vec<(&'a str, Option<&'a str>, &'a Utf8PathBuf)>);

        impl<'a> SortedArtifacts<'a> {
            fn new(map: &'a ArtifactMap) -> Self {
                let items = (map.0.iter())
                    .flat_map(|(name, files)| {
                        (files.iter()).map(move |(target, file)| (&**name, target.as_deref(), file))
                    })
                    .collect::<Vec<_>>()
                    .tap_mut(|s| s.sort());
                Self(items)
            }
        }

        impl Debug for SortedArtifacts<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let mut f = f.debug_map();
                for (name, target, path) in self.0.iter() {
                    let target = target.unwrap_or("*");
                    f.entry(&format_args!("\"{name} [{target}]\""), &path);
                }
                f.finish()
            }
        }
    }
}

impl Debug for PathMapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("PathMapper");
        if let Some(build) = &self.build {
            f.field(
                "target",
                &format_args!(
                    "{host:?}:{build:?}",
                    host = self.host.target_dir,
                    build = build.target_dir,
                ),
            );
            if build.build_dir.as_ref() != Some(&build.target_dir) {
                f.field(
                    "build",
                    &format_args!(
                        "{host:?}:{build:?}",
                        host = self.host.build_dir,
                        build = build.build_dir,
                    ),
                );
            }
        } else {
            f.field("target", &self.host.target_dir);
            if self.host.build_dir.as_ref() != Some(&self.host.target_dir) {
                f.field("build", &self.host.build_dir);
            }
        }
        f.finish()
    }
}
