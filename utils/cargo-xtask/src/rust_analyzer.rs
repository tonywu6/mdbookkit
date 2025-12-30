//! Download a copy of rust-analyzer to /.bin to use in testing.

use std::{
    fs,
    io::{self, Seek, SeekFrom},
    path::PathBuf,
    process::{self, Stdio},
    time::Duration,
};

use anyhow::Result;
use cargo_run_bin::metadata::get_project_root;
use flate2::write::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use tap::{Pipe, Tap};
use tempfile::tempfile;

#[derive(clap::Parser, Debug)]
pub struct Program {
    #[arg(long)]
    ra_version: Option<String>,
    #[arg(long)]
    ra_path: Option<PathBuf>,
    #[clap(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    Download,
    Analyzer {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    Version,
}

impl Program {
    pub fn run(self) -> Result<()> {
        let release = std::env::var("RA_VERSION")
            .ok()
            .unwrap_or("2025-12-01".into());

        let path = match self.ra_path {
            Some(path) => path,
            None => get_project_root()?
                .join(".bin/rust-analyzer")
                .join(&release)
                .join("rust-analyzer"),
        };

        let download = Download { release, path };

        match self.command {
            Command::Download => download.download(),
            Command::Analyzer { args } => analyzer(&download, args),
            Command::Version => {
                print!("{}", download.release);
                Ok(())
            }
        }
    }
}

fn analyzer(download: &Download, args: Vec<String>) -> Result<()> {
    if !download.path.exists() {
        download.download()?;
    }
    process::Command::new(&download.path)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?
        .code()
        .unwrap_or_default()
        .pipe(std::process::exit);
}

#[derive(Debug)]
struct Download {
    release: String,
    path: PathBuf,
}

impl Download {
    fn download(&self) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        self.download_gzip()?;
        #[cfg(target_os = "windows")] // ugh
        self.download_zip()?;
        Ok(())
    }

    #[cfg_attr(target_os = "windows", allow(unused))]
    fn download_gzip(&self) -> Result<()> {
        let Self { release, path } = self;

        let platform = env!("TARGET");
        let url = format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{release}/rust-analyzer-{platform}.gz"
        );

        let mut res = reqwest::blocking::get(url)?.error_for_status()?;

        fs::create_dir_all(path.parent().unwrap())?;

        fs::File::create(path)?
            .pipe(io::BufWriter::new)
            .pipe(GzDecoder::new)
            .pipe(Progress::new(Self::progress_bar(&res)))
            .pipe(|mut w| res.copy_to(&mut w).and(Ok(w)))?
            .0
            .finish()?;

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(path)?
                .permissions()
                .tap_mut(|p| p.set_mode(0o755))
                .pipe(|p| fs::set_permissions(path, p))?;
        }

        Ok(())
    }

    #[cfg_attr(not(target_os = "windows"), allow(unused))]
    fn download_zip(&self) -> Result<()> {
        let Self { release, path } = self;

        let temp = tempfile()?;

        let platform = env!("TARGET");
        let url = format!(
            "https://github.com/rust-lang/rust-analyzer/releases/download/{release}/rust-analyzer-{platform}.zip"
        );

        let mut res = reqwest::blocking::get(url)?.error_for_status()?;

        let temp = temp
            .pipe(io::BufWriter::new)
            .pipe(Progress::new(Self::progress_bar(&res)))
            .pipe(|mut w| res.copy_to(&mut w).and(Ok(w)))?
            .0
            .into_inner()
            .unwrap()
            .tap_mut(|file| file.seek(SeekFrom::Start(0)).map(|_| ()).unwrap());

        let mut archive = zip::ZipArchive::new(temp)?;

        fs::create_dir_all(path.parent().unwrap())?;

        let mut bin = archive.by_name("rust-analyzer.exe")?;
        let mut out = fs::File::create(path)?;

        std::io::copy(&mut bin, &mut out)?;

        Ok(())
    }

    fn progress_bar(res: &reqwest::blocking::Response) -> ProgressBar {
        static BAR_TEMPLATE: &str = "{spinner:.cyan} {prefix} {bar:20.green} {binary_bytes:.yellow} {binary_total_bytes:.yellow} {binary_bytes_per_sec:.yellow}";

        if let Some(len) = res.content_length() {
            ProgressBar::new(len)
        } else {
            ProgressBar::new_spinner()
        }
        .with_prefix("downloading rust-analyzer")
        .with_style(
            ProgressStyle::with_template(BAR_TEMPLATE)
                .unwrap()
                .tick_chars("⠇⠋⠙⠸⠴⠦⠿")
                .progress_chars("⠿⠦⠴⠸⠙⠋⠇ "),
        )
        .tap(|b| b.enable_steady_tick(Duration::from_millis(100)))
    }
}

struct Progress<W>(W, ProgressBar);

impl<W: io::Write> io::Write for Progress<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.0.write(buf)?;
        self.1.inc(written as _);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        let written = self.0.write_vectored(bufs)?;
        self.1.inc(written as _);
        Ok(written)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.1.inc(buf.len() as _);
        self.0.write_all(buf)
    }
}

impl<W> Progress<W> {
    fn new(p: ProgressBar) -> impl FnOnce(W) -> Self {
        |w| Self(w, p)
    }
}
