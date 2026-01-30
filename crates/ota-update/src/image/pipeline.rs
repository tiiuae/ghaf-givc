use shell_escape::escape;
use std::borrow::Cow;
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
    pub fn new<S: Into<String>>(program: S) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.args.push(path.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn arg<S: Into<String>>(mut self, arg: S) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

impl Into<Pipeline> for CommandSpec {
    fn into(self) -> Pipeline {
        Pipeline::new(self)
    }
}

impl Pipeline {
    pub fn new(first: CommandSpec) -> Self {
        Self {
            stages: vec![first],
        }
    }

    pub fn pipe(mut self, next: CommandSpec) -> Self {
        self.stages.push(next);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn format_shell(&self) -> String {
        self.stages
            .iter()
            .map(|cmd| {
                let mut s = cmd.program.clone();
                for arg in &cmd.args {
                    s.push(' ');
                    s.push_str(&escape(Cow::Borrowed(arg)));
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
