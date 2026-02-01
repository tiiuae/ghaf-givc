use shell_escape::escape;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    pub stages: Vec<CommandSpec>,
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
        }
    }

    #[must_use]
    pub fn pipe(mut self, next: CommandSpec) -> Self {
        self.stages.push(next);
        self
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
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
            .join(" | ")
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
}
