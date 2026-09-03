// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use shell_escape::escape;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    stages: Vec<CommandSpec>,
    mode: PipelineMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipelineMode {
    Single,
    Piped,
    Sequential,
}

impl CommandSpec {
    #[must_use]
    pub fn new<S: Into<String>>(program: S) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.args.push(path.as_ref().to_string_lossy().into_owned());
        self
    }

    #[must_use]
    pub fn arg<S: Into<String>>(mut self, arg: S) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

impl From<CommandSpec> for Pipeline {
    fn from(val: CommandSpec) -> Pipeline {
        Pipeline::new(val)
    }
}

impl Pipeline {
    #[must_use]
    pub fn new(first: CommandSpec) -> Self {
        Self {
            stages: vec![first],
            mode: PipelineMode::Single,
        }
    }

    /// Add a parallel stage to `Pipeline` processing previous stage's output
    ///
    /// # Panics
    /// Sequential and parallel stages cannot be mixed in same `Pipeline`
    #[must_use]
    pub fn pipe(mut self, next: CommandSpec) -> Self {
        assert_ne!(
            self.mode,
            PipelineMode::Sequential,
            "cannot append a pipe stage to a sequential pipeline"
        );
        self.mode = PipelineMode::Piped;
        self.stages.push(next);
        self
    }

    /// Add a sequential stage to `Pipeline` running after previous stage
    ///
    /// # Panics
    /// Sequential and parallel stages cannot be mixed in same `Pipeline`
    #[must_use]
    pub fn then(mut self, next: CommandSpec) -> Self {
        assert_ne!(
            self.mode,
            PipelineMode::Piped,
            "cannot append a sequential stage to a piped pipeline"
        );
        self.mode = PipelineMode::Sequential;
        self.stages.push(next);
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    #[must_use]
    pub(crate) fn stages(&self) -> &[CommandSpec] {
        &self.stages
    }

    #[must_use]
    pub(crate) fn is_sequential(&self) -> bool {
        self.mode == PipelineMode::Sequential
    }

    #[must_use]
    pub fn format_shell(&self) -> String {
        self.stages
            .iter()
            .map(|cmd| {
                let mut s = cmd.program.clone();
                for arg in &cmd.args {
                    s.push(' ');
                    s.push_str(&escape(arg.into()));
                }
                s
            })
            .collect::<Vec<_>>()
            .join(if self.is_sequential() { " && " } else { " | " })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_shell_format() {
        let p = Pipeline::new(CommandSpec::new("zstdcat").arg("root.zst"))
            .pipe(CommandSpec::new("dd").arg("of=/dev/null"));

        assert_eq!(p.format_shell(), "zstdcat root.zst | dd of=/dev/null");
    }

    #[test]
    #[should_panic(expected = "cannot append a sequential stage to a piped pipeline")]
    fn rejects_mixed_pipe_and_sequence() {
        let _ = Pipeline::new(CommandSpec::new("producer"))
            .pipe(CommandSpec::new("consumer"))
            .then(CommandSpec::new("commit"));
    }
}
