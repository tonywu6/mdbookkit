use std::process::Command;

use crate::options::CommandRunner;

pub trait CommandRunnerUtil {
    fn runner(self, runner: &CommandRunner) -> Self;
}

impl CommandRunnerUtil for Command {
    fn runner(self, runner: &CommandRunner) -> Self {
        runner.command(self)
    }
}
