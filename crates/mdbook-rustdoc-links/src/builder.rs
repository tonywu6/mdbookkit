use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::{Debug, Write as _},
    io::{BufRead, BufReader, Write as _},
    path::Path,
    process::{self, Command},
    sync::Arc,
    thread::JoinHandle,
};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{
    PackageId,
    camino::{Utf8Path, Utf8PathBuf},
    diagnostic::Diagnostic,
};
use tap::{Pipe, Tap, TapFallible};
use tempfile::TempDir;
use tracing::{Level, debug, info, info_span, trace, warn};

use mdbookkit::{
    emit_debug, emit_error, emit_warning,
    env::is_logging,
    error::{Break, ExpectFmt, PathDebug},
    ticker, ticker_event,
};

use crate::{
    options::{
        BuildConfigResolved, BuildOptions, Builder, PackageSelector, PackageSpec, WorkspaceMember,
    },
    subprocess::{CommandUtil, Subprocess},
    tracker::LinkTracker,
    with_notes,
};

pub fn build_docs(options: BuildConfigResolved, tracker: &mut LinkTracker) -> Result<(), Break> {
    let BuildConfigResolved {
        manifest_dir,
        builders,
    } = options;

    let mut counter = BuildCounter::new(builders.len());

    for (build_id, builder) in builders.into_iter().enumerate() {
        let build_id = build_id + 1;

        counter.prebuild(build_id, &builder);

        let result = info_span!("build", instance = build_id)
            .in_scope(|| run_builder(&manifest_dir, builder, tracker));

        counter.postbuild(build_id, result);
    }

    counter.finish().or_else(emit_error!())
}

fn run_builder(
    manifest_dir: &Utf8Path,
    builder: Builder,
    tracker: &mut LinkTracker<'_>,
) -> Result<(), Break> {
    tracker.notes().mark_option_specified(&builder.options);

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
        .or_else(with_notes!(emit_warning, tracker.notes()))?;

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
            .or_else(emit_warning!())?;
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
        (tracker.notes())
            .mark_preludes_not_derived_because("the `build.preludes` option has been specified");
        preludes
    } else {
        if let Some(lib) = resolve_prelude(tracker, &metadata, &packages) {
            tracker.notes().mark_preludes_derived(vec![lib])
        } else {
            vec![]
        }
    };

    debug!("resolved preludes: {preludes:#?}");

    let rustflags = if !rustc_args.is_empty() {
        Some(into_cargo_config("build.rustflags", rustc_args))
    } else {
        None
    };

    let rustdocflags = if !rustdoc_args.is_empty() {
        Some(into_cargo_config(
            "build.rustdocflags",
            rustdoc_args.clone(),
        ))
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
            .or_else(emit_warning!())?;
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
        .options("--package", packages.to_args())
        .options("--features", features.list())
        .flag("--all-features", features.all_features())
        .flag("--no-default-features", features.no_default_features())
        .options("--config", &rustflags)
        .options("--config", &rustdocflags)
        .options("--config", artifacts.term.cargo_options)
        .runner(&cargo.runner)
        .current_dir(manifest_dir)
        .run();

    artifacts
        .record(proc, ticker!(Level::INFO, "cargo-doc", "cargo doc"))
        .context("`cargo doc` did not succeed")
        .or_else(with_notes!(emit_warning, tracker.notes()))?;

    let proc = cargo
        .command("check")
        .options("--message-format", ["json"])
        .options("--target", &targets)
        .options("--package", packages.to_args())
        .options("--features", features.list())
        .flag("--all-features", features.all_features())
        .flag("--no-default-features", features.no_default_features())
        .options("--config", &rustflags)
        .options("--config", &rustdocflags)
        .options("--config", artifacts.term.cargo_options)
        .runner(&cargo.runner)
        .current_dir(manifest_dir)
        .run();

    artifacts
        .record(proc, ticker!(Level::INFO, "cargo-check", "cargo check"))
        .context("`cargo check` did not succeed")
        .or_else(with_notes!(emit_warning, tracker.notes()))?;

    trace!("{artifacts:?}");

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
            .or_else(emit_warning!())?;

        let mut rustdoc = Command::new("rustdoc")
            .values(cargo.toolchain())
            .options("--target", target.as_deref())
            .options("--out-dir", [tempdir.path()])
            .options("--edition", ["2024"])
            .options("--crate-type", ["lib"])
            .options("--error-format", ["json"])
            .values(["-"]);

        rustdoc.args(&rustdoc_args);

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
                    .or_else(emit_warning!())
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
                        .or_else(emit_warning!())
                        .ok();
                };
            }

            let mut stdin = rustdoc.stdin().or_else(emit_warning!())?;

            write_to!(stdin, "{docstring}");

            for prelude in preludes.iter() {
                write_to!(stdin, "use {prelude};");
            }
        }

        let result = rustdoc
            .result()
            .context("`rustdoc` did not succeed")
            .or_else(with_notes!(emit_warning, tracker.notes()))?;

        if let Some(status) = result.status {
            let stderr = rustc_json_error(&result.output.stderr);
            let stderr = stderr.trim_end();
            let stderr = if stderr.is_empty() { "(empty)" } else { stderr };
            return Err(status)
                .context(format!("--- rustdoc stderr\n{stderr}"))
                .context("`rustdoc` did not succeed")
                .or_else(with_notes!(emit_warning, tracker.notes()))?;
        } else {
            if tracing::enabled!(Level::TRACE) {
                let stderr = String::from_utf8_lossy(&result.output.stderr);
                let stderr = stderr.trim_end();
                let stderr = if stderr.is_empty() { "(empty)" } else { stderr };
                trace!("--- rustdoc stderr\n{stderr}");
            }
        }

        let output = BuildOutput {
            metadata: &metadata,
            crates: &artifacts.crates,
            stdout: {
                let path = tempdir.path().join(crate_name!()).join("index.html");
                std::fs::read_to_string(&path)
                    .with_context(|| format!("expected {:?}", path.debug()))
                    .context("failed to read from `rustdoc` output")
                    .or_else(emit_warning!())?
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
                .or_else(emit_warning!())
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
            let cargo_stderr = stderr
                .join()
                .map_err(|_| anyhow!("failed to recover stderr"))
                .or_else(emit_debug!())
                .unwrap_or_default();

            let cargo_stderr = if let Some(stderr) = cargo_stderr {
                stderr.join("\n")
            } else {
                "(see logs above)".into()
            };

            let rustc_errors = rustc_errors.join("");

            let cargo_stderr = cargo_stderr
                .trim_end()
                .pipe(|s| if s.is_empty() { "(empty)" } else { s });
            let rustc_errors = rustc_errors
                .trim_end()
                .pipe(|s| if s.is_empty() { "(empty)" } else { s });

            let error = error
                .context(format!("--- cargo stderr\n{cargo_stderr}"))
                .context(format!("--- rustc errors\n{rustc_errors}"));

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
                .or_else(emit_warning!())
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
) -> Result<PackageList, Break> {
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
        .or_else(emit_warning!())?
        .iter()
        .flat_map(|output| {
            output.lines().filter_map(|line| {
                let mut iter = line.split(' ');
                let name = iter.next()?.to_owned();
                let version = iter.next()?;
                let version = version.strip_prefix('v')?.to_owned();
                Some((name, version))
            })
        })
        .collect::<BTreeSet<_>>()
        .pipe(PackageList)
        .pipe(Ok)
}

fn load_docs_rs_options(
    builder: &mut Builder,
    metadata: &cargo_metadata::Metadata,
    packages: &PackageList,
) -> Result<()> {
    let selected = if packages.is_empty() {
        metadata.workspace_default_packages()
    } else {
        (metadata.workspace_members)
            .iter()
            .filter_map(|id| {
                let pkg = &metadata[id];
                if packages.contains(pkg) {
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

fn resolve_prelude(
    tracker: &mut LinkTracker<'_>,
    metadata: &cargo_metadata::Metadata,
    packages: &PackageList,
) -> Option<String> {
    let default_packages = metadata.workspace_default_packages();
    if default_packages.len() != 1 {
        let reason = "workspace has multiple default members";
        tracker.notes().mark_preludes_not_derived_because(reason);
        return None;
    }

    let pkg = default_packages[0];
    if !packages.contains(pkg) {
        let reason =
            "the default workspace member has been filtered out by the `build.packages` option";
        tracker.notes().mark_preludes_not_derived_because(reason);
        return None;
    }

    let Some(lib) = pkg.targets.iter().find_map(|t| {
        if t.is_lib() || t.is_dylib() || t.is_proc_macro() || t.is_rlib() {
            Some(format!("{}::*", t.name))
        } else {
            None
        }
    }) else {
        let reason = "the default workspace member is not a library";
        tracker.notes().mark_preludes_not_derived_because(reason);
        return None;
    };

    Some(lib)
}

#[derive(Debug, Default)]
struct PackageList(BTreeSet<(String, String)>);

impl PackageList {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn contains(&self, pkg: &cargo_metadata::Package) -> bool {
        if self.is_empty() {
            true
        } else {
            let spec = (pkg.name.to_string(), pkg.version.to_string());
            self.0.contains(&spec)
        }
    }

    fn to_args(&self) -> Vec<String> {
        (self.0.iter())
            .map(|(name, version)| format!("{name}@{version}"))
            .collect()
    }
}

fn into_cargo_config(key: &str, value: impl Into<toml::Value>) -> String {
    format!("{key}={}", value.into())
}

struct CargoProgress {
    cargo_options: &'static [&'static str],
    term_progress: bool,
}

impl CargoProgress {
    #[must_use]
    fn ticker(
        &self,
        ticker: tracing::Span,
        stderr: process::ChildStderr,
    ) -> JoinHandle<Option<Vec<String>>> {
        let term_progress = self.term_progress;
        let line_ending = if term_progress { b'\r' } else { b'\n' };
        let visible = !ticker.is_disabled();

        std::thread::spawn(move || {
            let mut buffer = vec![];
            let mut reader = BufReader::new(stderr);

            loop {
                let mut buf = vec![];
                let Ok(1..) = reader.read_until(line_ending, &mut buf) else {
                    break;
                };
                let buf = String::from_utf8_lossy(&buf);

                for line in buf.lines() {
                    let ending = line.as_bytes().last();
                    match (visible, term_progress, ending) {
                        (true, true, Some(b'\r')) | (true, false, _) => {
                            ticker_event!(&ticker, Level::INFO, "{}", line.trim_end());
                        }
                        _ => {
                            buffer.push(line.trim_end().to_owned());
                        }
                    }
                }
            }

            if visible && !term_progress {
                None
            } else {
                Some(buffer)
            }
        })
    }
}

impl Default for CargoProgress {
    fn default() -> Self {
        if is_logging() {
            Self {
                cargo_options: &["term.color = 'never'", "term.progress.when = 'never'"],
                term_progress: false,
            }
        } else {
            Self {
                cargo_options: &[
                    "term.quiet = true",
                    "term.progress.when = 'always'",
                    "term.progress.width = 1024",
                ],
                term_progress: true,
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
