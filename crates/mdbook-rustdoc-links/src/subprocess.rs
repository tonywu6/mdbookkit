use std::{
    ffi::OsStr,
    process::{self, Command, Stdio},
};

use anyhow::{Result, anyhow};
use tap::Pipe;
use tracing::debug;

use crate::options::CommandRunner;

pub trait CommandUtil {
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

pub struct Subprocess {
    repr: PrintCommand,
    proc: std::io::Result<process::Child>,
}

impl Subprocess {
    pub fn stdin(&mut self) -> Result<process::ChildStdin> {
        Ok(self.proc()?.stdin.take().expect("should have stdin"))
    }

    pub fn stdout(&mut self) -> Result<process::ChildStdout> {
        Ok(self.proc()?.stdout.take().expect("should have stdout"))
    }

    pub fn stderr(&mut self) -> Result<process::ChildStderr> {
        Ok(self.proc()?.stderr.take().expect("should have stderr"))
    }

    pub fn proc(&mut self) -> Result<&mut process::Child> {
        match self.proc {
            Ok(ref mut proc) => Ok(proc),
            Err(ref error) => Err(self.repr.failed_to_spawn(error)),
        }
    }

    pub fn result(self) -> Result<SubprocessResult> {
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

    pub fn checked(self) -> Result<process::Output> {
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

pub struct SubprocessResult {
    pub output: process::Output,
    pub status: Option<anyhow::Error>,
    pub repr: PrintCommand,
}

pub struct PrintCommand(String);

impl PrintCommand {
    pub fn as_context(&self) -> anyhow::Error {
        anyhow!("command: {}\n", self.0)
    }

    pub fn failed_to_spawn(&self, error: &std::io::Error) -> anyhow::Error {
        (self.as_context())
            .context(anyhow!("{error}"))
            .context("failed to spawn command")
    }
}
