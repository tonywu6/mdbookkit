use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsStr,
    fmt::{Debug, Write as _},
    io::{BufRead, BufReader, Write as _},
    path::Path,
    process::{self, Command, Stdio},
    sync::Arc,
    thread::JoinHandle,
};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{
    PackageId,
    camino::{Utf8Path, Utf8PathBuf},
    diagnostic::Diagnostic,
};
use serde::Deserialize;
use tap::{Pipe, Tap, TapFallible};
use tempfile::TempDir;
use tracing::{Level, debug, info, info_span, trace, warn};

use mdbookkit::{
    emit,
    env::is_logging,
    error::{Break, ConsumeError, ExpectFmt, PathDebug},
    ticker, ticker_event,
};

use crate::{
    options::{
        BuildConfig, BuildOptions, Builder, CargoOptions, CommandRunner, PackageSelector,
        PackageSpec, WorkspaceMember,
    },
    tracker::LinkTracker,
};

pub fn build_docs(
    BuildConfig {
        manifest_dir,
        build,
        build_options,
    }: BuildConfig,
    bookdir: &Utf8Path,
    tracker: &mut LinkTracker,
) -> Result<(), Break> {
    let build = if build.is_empty() {
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

    let default_cargo = if build.len() == 1 {
        &build[0].options.cargo
    } else {
        &build_options.cargo
    };

    // https://github.com/rust-lang/cargo/issues/16834
    let manifest_dir = if let Some(dir) = manifest_dir {
        bookdir
            .join(dir)
            .canonicalize_utf8()
            .context("failed to resolve `manifest-dir` to an absolute path")
            .or_error(emit!())?
    } else {
        default_cargo
            .workspace(bookdir.as_std_path())
            .context("while preparing to build docs using `cargo`")
            .context("failed to determine the current workspace root")
            .or_error(emit!())?
            .directory()
            .to_owned()
    };

    debug!("resolved manifest dir: {manifest_dir}");

    let mut counter = BuildCounter::new(build.len());

    for (build_id, builder) in build.into_iter().enumerate() {
        let build_id = build_id + 1;

        counter.prebuild(build_id, &builder);

        let result = info_span!("build", instance = build_id)
            .in_scope(|| run_builder(&manifest_dir, builder, tracker));

        counter.postbuild(build_id, result);
    }

    counter.finish().or_error(emit!())
}

fn run_builder(
    manifest_dir: &Utf8Path,
    builder: Builder,
    tracker: &mut LinkTracker<'_>,
) -> Result<(), Break> {
    let BuildOptions {
        ref packages,
        ref features,
        ref cargo,
        docs_rs,
        ..
    } = builder.options;

    let ticker = ticker!(Level::INFO, "prepare", "preparing");
    ticker_event!(&ticker, Level::INFO, "cargo metadata");

    let metadata = cargo
        .command("metadata")
        .options("--format-version", ["1"])
        .options("--features", features.list())
        .flag("--all-features", features.all_features())
        .flag("--no-default-features", features.no_default_features())
        .current_dir(manifest_dir)
        .run()
        .into_cargo_metadata()
        .context("failed to learn about the workspace via cargo")
        .or_warn(emit!())?;

    let packages = if packages.is_empty() {
        Default::default()
    } else {
        ticker_event!(&ticker, Level::INFO, "resolving packages");
        resolve_packages(&metadata, &builder.options, manifest_dir)?
    };

    debug!("resolved packages: {packages:#?}");

    let mut builder = builder;
    if docs_rs == Some(true) {
        load_docs_rs_options(&mut builder, &metadata, &packages)
            .context("failed to inherit docs.rs options")
            .or_warn(emit!())?;
    }

    debug!("resolved options: {builder:#?}");

    let Builder { targets, options } = builder;
    let BuildOptions {
        packages: _,
        preludes,
        features,
        rustc_args,
        rustdoc_args,
        cargo,
        docs_rs: _,
    } = options;

    let preludes = if let Some(preludes) = preludes {
        preludes
    } else if metadata.workspace_default_packages().len() == 1
        && let Some(pkg) = metadata.workspace_default_packages().first()
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

    debug!("resolved preludes: {preludes:#?}");

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

    let path_mapper = if cargo.runner.is_undefined() {
        PathMapper::new(&metadata, None)
    } else {
        ticker_event!(&ticker, Level::INFO, "cargo metadata");
        let build_metadata = cargo
            .command("metadata")
            .options("--format-version", ["1"])
            .runner(&cargo.runner)
            .current_dir(manifest_dir)
            .run()
            .into_cargo_metadata()
            .context("failed to learn about workspace paths via cargo")
            .or_warn(emit!())?;
        PathMapper::new(&metadata, Some(&build_metadata))
    };

    debug!("resolved path mappings: {path_mapper:#?}");
    drop(ticker);

    let mut artifacts = CargoRecorder::new(path_mapper);

    let proc = cargo
        .command("doc")
        .options("--message-format", ["json"])
        .options("--target", &targets)
        .flag("--no-deps", !packages.is_empty())
        .options("--package", &packages)
        .options("--features", features.list())
        .flag("--all-features", features.all_features())
        .flag("--no-default-features", features.no_default_features())
        .options("--config", &rustc_args)
        .options("--config", &rustdoc_args)
        .options("--config", artifacts.term.cargo_options)
        .runner(&cargo.runner)
        .current_dir(manifest_dir)
        .run();

    artifacts
        .record(proc, ticker!(Level::INFO, "cargo-doc", "cargo doc"))
        .context("`cargo doc` did not succeed")
        .or_warn(emit!())?;

    let proc = cargo
        .command("check")
        .options("--message-format", ["json"])
        .options("--target", &targets)
        .options("--package", &packages)
        .options("--features", features.list())
        .flag("--all-features", features.all_features())
        .flag("--no-default-features", features.no_default_features())
        .options("--config", &rustc_args)
        .options("--config", &rustdoc_args)
        .options("--config", artifacts.term.cargo_options)
        .runner(&cargo.runner)
        .current_dir(manifest_dir)
        .run();

    artifacts
        .record(proc, ticker!(Level::INFO, "cargo-check", "cargo check"))
        .context("`cargo check` did not succeed")
        .or_warn(emit!())?;

    for target in artifacts.targets() {
        let Some(docstring) = tracker.rustdoc_input() else {
            break;
        };

        let _ticker = if let Some(target) = target.as_deref() {
            ticker!(Level::INFO, "rustdoc", "rustdoc [{target}]")
        } else {
            ticker!(Level::INFO, "rustdoc", "rustdoc")
        };

        let tempdir = TempDir::new_in(&metadata.target_directory)
            .context("failed to create temporary directory for doc artifacts")
            .or_warn(emit!())?;

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

            if let Some(doc) = (artifacts.get_doc(name, &target))
                .as_ref()
                .and_then(|dir| dir.parent())
                && let Some(lib) = artifacts.get_lib(name, &target)
            {
                symlink_dir_all(doc, tempdir.path().join(name))
                    .with_context(|| doc.to_owned())
                    .context("failed to locate doc artifacts expected at:")
                    .or_warn(emit!())
                    .ok();

                rustdoc.arg("--extern").arg(format!("{name}={lib}"));

                if let Some(parent) = lib.parent()
                    && !library_paths.contains(parent)
                {
                    library_paths.insert(parent.to_owned());
                }
            }
        }

        for path in library_paths {
            rustdoc.arg("-L").arg(format!("dependency={path}"));
        }

        let mut rustdoc = rustdoc
            .options("--crate-name", [crate_name!()])
            .runner(&cargo.runner)
            .current_dir(manifest_dir)
            .run();

        {
            macro_rules! write_to {
                ( $stdin:ident, $fmt:literal ) => {
                    writeln!($stdin, $fmt)
                        .context("could not pass input to `rustdoc`")
                        .or_warn(emit!())
                        .ok();
                };
            }

            let mut stdin = rustdoc.stdin().or_warn(emit!())?;

            write_to!(stdin, "{docstring}");

            for prelude in preludes.iter() {
                write_to!(stdin, "use {prelude};");
            }
        }

        let result = rustdoc
            .result()
            .context("`rustdoc` did not succeed")
            .or_warn(emit!())?;

        if let Some(status) = result.status {
            let stderr = rustc_json_error(&result.output.stderr);
            let stderr = stderr.trim_end();
            let stderr = if stderr.is_empty() { "(empty)" } else { stderr };
            warn!("--- rustdoc stderr\n{stderr}");
            return Err(status)
                .context("`rustdoc` did not succeed")
                .or_warn(emit!())?;
        } else {
            let stderr = String::from_utf8_lossy(&result.output.stderr);
            let stderr = stderr.trim_end();
            let stderr = if stderr.is_empty() { "(empty)" } else { stderr };
            trace!("--- rustdoc stderr\n{stderr}");
        }

        let output = BuildOutput {
            metadata: &metadata,
            crates: &artifacts.crates,
            stdout: {
                let path = tempdir.path().join(crate_name!()).join("index.html");
                std::fs::read_to_string(&path)
                    .with_context(|| format!("expected {:?}", path.debug()))
                    .context("failed to read from `rustdoc` output")
                    .or_warn(emit!())?
            },
            stderr: result.output.stderr,
        };

        tracker.rustdoc_output(output);
    }

    Ok(())
}

struct BuildCounter {
    num_builds: usize,
    num_failed: usize,
}

impl BuildCounter {
    fn new(num_builds: usize) -> Self {
        Self {
            num_builds,
            num_failed: 0,
        }
    }

    fn prebuild(&self, id: usize, build: &Builder) {
        if self.num_builds == 1 {
            info!("building docs")
        } else {
            info!("running build #{id} {:?}", build.debug())
        }
    }

    fn postbuild(&mut self, id: usize, result: Result<(), Break>) {
        if result.is_err() {
            self.num_failed += 1;
        }
        if self.num_builds != 1 {
            if result.is_err() {
                warn!("build #{id} has failed")
            } else {
                info!("build #{id} done")
            }
        }
    }

    fn finish(self) -> Result<()> {
        if self.num_builds == self.num_failed {
            if self.num_builds == 1 {
                bail!("build failed")
            } else {
                bail!("all builds have failed")
            }
        }
        if self.num_failed != 0 {
            warn!("some builds have failed")
        }
        Ok(())
    }
}

pub struct BuildOutput<'a> {
    pub metadata: &'a cargo_metadata::Metadata,
    pub crates: &'a BTreeMap<Arc<str>, PackageId>,
    pub stdout: String,
    pub stderr: Vec<u8>,
}

struct CargoRecorder {
    paths: PathMapper,
    targets: BTreeSet<Arc<str>>,
    crates: BTreeMap<Arc<str>, PackageId>,
    libs: ArtifactMap,
    docs: ArtifactMap,
    term: CargoProgress,
}

#[derive(Default)]
struct ArtifactMap(HashMap<Arc<str>, HashMap<Option<Arc<str>>, Utf8PathBuf>>);

impl CargoRecorder {
    fn new(paths: PathMapper) -> Self {
        Self {
            paths,
            targets: Default::default(),
            crates: Default::default(),
            libs: Default::default(),
            docs: Default::default(),
            term: Default::default(),
        }
    }

    fn record(&mut self, mut proc: Subprocess, ticker: tracing::Span) -> Result<()> {
        let stderr = self.term.ticker(ticker, proc.stderr()?);

        let mut success = false;
        let mut rustc_errors = vec![];

        for msg in (proc.stdout())?
            .pipe(BufReader::new)
            .pipe(cargo_metadata::Message::parse_stream)
        {
            let Ok(msg) = msg
                .tap_ok(|msg| trace!("{msg:?}"))
                .context("error while reading from cargo")
                .or_warn(emit!())
            else {
                continue;
            };
            match msg {
                cargo_metadata::Message::CompilerArtifact(artifact) => {
                    self.update_unit(artifact);
                }
                cargo_metadata::Message::BuildFinished(finished) => {
                    success = finished.success;
                }
                cargo_metadata::Message::CompilerMessage(message) => {
                    let message = (message.message.rendered)
                        .unwrap_or_else(|| format!("{}\n", message.message.message));
                    rustc_errors.push(message);
                }
                _ => {}
            }
        }

        let result = proc.result()?;

        let error = if let Some(status) = result.status {
            Some(status)
        } else if !success {
            (result.repr.as_context())
                .context("cargo finished with errors")
                .pipe(Some)
        } else {
            None
        };

        if let Some(error) = error {
            let cargo_errors = stderr
                .join()
                .map_err(|_| anyhow!("failed to recover stderr"))
                .or_debug(emit!())
                .unwrap_or_default()
                .join("\n");

            let cargo_errors = cargo_errors
                .trim_end()
                .pipe(|s| if s.is_empty() { "(empty)" } else { s });
            warn!("--- cargo stderr\n{cargo_errors}\n");

            let rustc_errors = rustc_errors.join("");
            let rustc_errors = rustc_errors
                .trim_end()
                .pipe(|s| if s.is_empty() { "(empty)" } else { s });
            warn!("--- rustc stderr\n{rustc_errors}\n");

            Err(error)
        } else {
            Ok(())
        }
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
                .context("error while collecting compiler artifacts")
                .or_warn(emit!())
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

fn resolve_packages(
    metadata: &cargo_metadata::Metadata,
    options: &BuildOptions,
    manifest_dir: &Utf8Path,
) -> Result<BTreeSet<String>, Break> {
    let BuildOptions {
        packages,
        features,
        cargo,
        ..
    } = options;

    let pkgs = packages.iter().flat_map(|spec| match spec {
        PackageSpec::Name(name) => vec![(name, false)],
        PackageSpec::Selector(PackageSelector {
            name: Some(name),
            dependencies,
            ..
        }) => {
            vec![(name, *dependencies)]
        }
        PackageSpec::Selector(PackageSelector {
            workspace,
            dependencies,
            ..
        }) => match workspace {
            WorkspaceMember::None => vec![],
            WorkspaceMember::Default => {
                if metadata.workspace_default_members.is_available() {
                    metadata.workspace_default_packages()
                } else {
                    metadata.workspace_packages()
                }
            }
            WorkspaceMember::All => metadata.workspace_packages(),
        }
        .iter()
        .map(|pkg| (&pkg.id.repr, *dependencies))
        .collect(),
    });

    let mut trees = HashMap::<_, Vec<_>>::new();
    for (pkg, dep) in pkgs {
        let depth = if dep { "1" } else { "0" };
        trees.entry(depth).or_default().push(pkg);
    }

    let command = || {
        cargo
            .command("tree")
            .options("--features", features.list())
            .flag("--all-features", features.all_features())
            .flag("--no-default-features", features.no_default_features())
            .flag("--no-dedupe", true)
            .options("--format", ["{p}"])
            .options("--prefix", ["none"])
            .options("--edges", ["normal"])
    };

    trees
        .into_iter()
        .map(|(depth, packages)| {
            command()
                .options("--package", packages)
                .options("--depth", [depth])
                .runner(&cargo.runner)
                .current_dir(manifest_dir)
                .run()
                .checked()?
                .stdout
                .pipe(String::from_utf8)?
                .pipe(Ok)
        })
        .collect::<Result<Vec<_>>>()
        .context("failed to resolve package versions")
        .or_warn(emit!())?
        .iter()
        .flat_map(|output| {
            output.lines().filter_map(|line| {
                let mut iter = line.split(' ');
                let name = iter.next()?;
                let version = iter.next()?;
                let version = version.strip_prefix('v')?;
                Some(format!("{name}@{version}"))
            })
        })
        .collect::<BTreeSet<_>>()
        .pipe(Ok)
}

fn load_docs_rs_options(
    builder: &mut Builder,
    metadata: &cargo_metadata::Metadata,
    packages: &BTreeSet<String>,
) -> Result<()> {
    let selected = if packages.is_empty() {
        metadata.workspace_default_packages()
    } else {
        (metadata.workspace_members)
            .iter()
            .filter_map(|id| {
                let pkg = &metadata[id];
                let spec = format!("{}@{}", pkg.name, pkg.version);
                if packages.contains(&spec) {
                    Some(pkg)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    if selected.is_empty() {
        bail!("build config did not select any workspace package to read config from")
    }
    if selected.len() > 1 {
        bail!("build config selected multiple workspace packages to read config from")
    }

    let package_name = || format!("the selected workspace package is {:?}", &*selected[0].name);

    let metadata = &selected[0].metadata;
    let metadata = (|| metadata.get("docs")?.get("rs")?.as_object())()
        .with_context(package_name)
        .context("package has no [package.metadata.docs.rs] table")?
        .clone();

    let mut metadata = metadata;
    let default_target = metadata.remove("default-target");
    let targets = metadata.remove("targets");
    let additional_targets = metadata.remove("additional-targets");

    let options = serde_json::from_value::<BuildOptions>(metadata.into())
        .with_context(package_name)
        .context("failed to deserialize config from the [package.metadata.docs.rs] table")?;

    builder.options.assign(&options);

    if builder.targets.is_empty() {
        if let Some(target) = default_target {
            let target = serde_json::from_value::<String>(target)
                .with_context(package_name)
                .context("could not read `default-target` as a string")?;
            builder.targets.push(target);
        }
        if let Some(targets) = targets {
            let targets = serde_json::from_value::<Vec<String>>(targets)
                .with_context(package_name)
                .context("could not read `targets` as a list of strings")?;
            builder.targets.extend(targets);
        }
        if let Some(targets) = additional_targets {
            let targets = serde_json::from_value::<Vec<String>>(targets)
                .with_context(package_name)
                .context("could not read `additional-targets` as a list of strings")?;
            builder.targets.extend(targets);
        }
    } else {
        info! { "ignoring target-related options in [package.metadata.docs.rs] since \
        `targets` has been defined in build config" }
    }

    Ok(())
}

struct CargoProgress {
    cargo_options: &'static [&'static str],
    line_ending: u8,
}

impl CargoProgress {
    #[must_use]
    fn ticker(
        &self,
        ticker: tracing::Span,
        stderr: process::ChildStderr,
    ) -> JoinHandle<Vec<String>> {
        let delim = self.line_ending;
        std::thread::spawn(move || {
            let mut buffer = vec![];
            let mut reader = BufReader::new(stderr);
            loop {
                let mut buf = vec![];
                let Ok(1..) = reader.read_until(delim, &mut buf) else {
                    break;
                };
                let buf = String::from_utf8_lossy(&buf);
                for line in buf.lines() {
                    match (delim, line.as_bytes().last()) {
                        (b'\r', Some(b'\r')) | (b'\n', _) => {
                            ticker_event!(&ticker, Level::INFO, "{}", line.trim_end());
                        }
                        _ => {
                            buffer.push(line.trim_end().to_owned());
                        }
                    }
                }
            }
            buffer
        })
    }
}

impl Default for CargoProgress {
    fn default() -> Self {
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
}

fn rustc_json_error(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .fold(String::new(), |mut error, line| {
            if let Ok(diag) = serde_json::from_str::<Diagnostic>(line) {
                if let Some(rendered) = diag.rendered {
                    write!(&mut error, "{}", rendered)
                } else {
                    writeln!(&mut error, "{}", diag.message.trim())
                }
            } else {
                writeln!(&mut error, "{line}")
            }
            .expect_fmt();
            error
        })
}

trait CargoMetadataUtil {
    fn into_cargo_metadata(self) -> Result<cargo_metadata::Metadata>;
}

impl CargoMetadataUtil for Subprocess {
    fn into_cargo_metadata(self) -> Result<cargo_metadata::Metadata> {
        let stdout = String::from_utf8(self.checked()?.stdout)?;
        Ok(cargo_metadata::MetadataCommand::parse(stdout)?)
    }
}

impl CargoOptions {
    fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new("cargo");
        command
            .args(self.toolchain())
            .arg(subcommand)
            .args(&self.cargo_args);
        command
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

    fn toolchain(&self) -> Option<String> {
        self.toolchain.as_ref().map(|t| format!("+{t}"))
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

    fn runner(self, runner: &CommandRunner) -> Self;

    fn run(&mut self) -> Subprocess;
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

    fn runner(self, runner: &CommandRunner) -> Self {
        runner.command(self)
    }

    fn run(&mut self) -> Subprocess {
        let repr = PrintCommand(format!("{self:?}"));
        debug!("running: {}", repr.0);
        let proc = self
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        Subprocess { repr, proc }
    }
}

struct Subprocess {
    repr: PrintCommand,
    proc: std::io::Result<process::Child>,
}

impl Subprocess {
    fn stdin(&mut self) -> Result<process::ChildStdin> {
        Ok(self.proc()?.stdin.take().expect("should have stdin"))
    }

    fn stdout(&mut self) -> Result<process::ChildStdout> {
        Ok(self.proc()?.stdout.take().expect("should have stdout"))
    }

    fn stderr(&mut self) -> Result<process::ChildStderr> {
        Ok(self.proc()?.stderr.take().expect("should have stderr"))
    }

    fn proc(&mut self) -> Result<&mut process::Child> {
        match self.proc {
            Ok(ref mut proc) => Ok(proc),
            Err(ref error) => Err(self.repr.failed_to_spawn(error)),
        }
    }

    fn result(self) -> Result<SubprocessResult> {
        let Self { repr, proc } = self;

        let proc = match proc {
            Ok(proc) => proc,
            Err(ref error) => return Err(repr.failed_to_spawn(error)),
        };

        let output = match proc.wait_with_output() {
            Ok(output) => output,
            Err(error) => {
                return (repr.as_context())
                    .context(error)
                    .context("error waiting for command to finish")
                    .pipe(Err);
            }
        };

        let status = if output.status.success() {
            None
        } else {
            (repr.as_context())
                .context(format!("command exited with {}", output.status))
                .pipe(Some)
        };

        Ok(SubprocessResult {
            output,
            status,
            repr,
        })
    }

    fn checked(self) -> Result<process::Output> {
        let SubprocessResult { output, status, .. } = self.result()?;
        if let Some(status) = status {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim_end();
            let stderr = if stderr.is_empty() { "(empty)" } else { stderr };
            let error = status.context(format!("--- stderr\n{stderr}\n---"));
            Err(error)
        } else {
            Ok(output)
        }
    }
}

struct SubprocessResult {
    output: process::Output,
    status: Option<anyhow::Error>,
    repr: PrintCommand,
}

struct PrintCommand(String);

impl PrintCommand {
    fn as_context(&self) -> anyhow::Error {
        anyhow!("command: {}\n---", self.0)
    }

    fn failed_to_spawn(&self, error: &std::io::Error) -> anyhow::Error {
        (self.as_context())
            .context(anyhow!("{error}"))
            .context("failed to spawn command")
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

impl Debug for CargoRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("CompilerOutput");

        let Self {
            paths,
            targets,
            crates,
            libs,
            docs,
            term: _,
        } = self;

        return f
            .field("targets", &targets)
            .field("crates", &crates)
            .field("libs", &SortedArtifacts::new(libs))
            .field("docs", &SortedArtifacts::new(docs))
            .field("paths", paths)
            .finish_non_exhaustive();

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
