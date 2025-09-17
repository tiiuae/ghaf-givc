use anyhow::{Context, bail};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmName<'a> {
    Vm(&'a str),
    App(&'a str),
}

impl std::fmt::Display for VmName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Self::Vm(vm) => write!(f, "{vm}"),
            Self::App(app) => write!(f, "{app}-vm"),
        }
    }
}

impl VmName<'_> {
    pub fn agent_service(&self) -> String {
        format!("givc-{self}.service")
    }

    pub fn vm_service(&self) -> String {
        format!("microvm@{self}.service")
    }
}

#[must_use]
pub fn parse_vm_name(name: &str) -> Option<&str> {
    name.strip_prefix("microvm@")?.strip_suffix(".service")
}

/// From `agent` code, ported for future
/// # Errors
/// Return `Err()` if parsing fails
pub fn parse_application_name(name: &str) -> anyhow::Result<(&str, i32)> {
    if let Some(name_no_suffix) = name.strip_suffix(".service")
        && let Some((left, right)) = name_no_suffix.rsplit_once('@')
    {
        let num = right
            .parse()
            .with_context(|| format!("While parsing number part of {name}"))?;
        return Ok((left, num));
    }
    bail!("App name {} not it app@<number>.service format", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_application_name() -> anyhow::Result<()> {
        let good = parse_application_name("good-app@42.service")?;
        assert_eq!(good, ("good-app", 42));

        let bad = parse_application_name("just-a.service");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "App name just-a.service not it app@<number>.service format"
        );

        Ok(())
    }
}
