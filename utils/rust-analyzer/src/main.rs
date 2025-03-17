use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::Result;
use cargo_run_bin::metadata::get_project_root;
use flate2::write::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use tap::{Pipe, Tap};

fn main() -> Result<()> {
    let release = std::env::var("RA_VERSION")
        .ok()
        .unwrap_or("2025-03-17".into());

    let path = get_project_root()?
        .join(".bin/rust-analyzer")
        .join(&release)
        .join("rust-analyzer");

    if !path.exists() {
        download(&release, &path)?;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        fs::metadata(&path)?
            .permissions()
            .tap_mut(|p| p.set_mode(0o755))
            .pipe(|p| fs::set_permissions(&path, p))?;
    }

    Command::new(path)
        .args(std::env::args().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?
        .code()
        .unwrap_or_default()
        .pipe(std::process::exit);
}

fn download<P: AsRef<Path>>(release: &str, outfile: P) -> Result<()> {
    let platform = env!("TARGET");

    let url = format!("https://github.com/rust-lang/rust-analyzer/releases/download/{release}/rust-analyzer-{platform}.gz");
    // rust-analyzer uses zip files for windows so this won't work on windows

    let mut res = reqwest::blocking::get(url)?;

    let bar = if let Some(len) = res.content_length() {
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
    .tap(|b| b.enable_steady_tick(Duration::from_millis(100)));

    static BAR_TEMPLATE: &str = "{spinner:.cyan} {prefix} {bar:20.green} {binary_bytes:.yellow} {binary_total_bytes:.yellow} {binary_bytes_per_sec:.yellow}";

    fs::create_dir_all(outfile.as_ref().parent().unwrap())?;

    fs::File::create(outfile)?
        .pipe(io::BufWriter::new)
        .pipe(GzDecoder::new)
        .pipe(Progress::new(bar))
        .pipe(|mut w| res.copy_to(&mut w).and(Ok(w)))?
        .0
        .finish()?;

    Ok(())
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
