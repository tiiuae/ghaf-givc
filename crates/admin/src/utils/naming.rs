use anyhow::{Context, bail};

#[must_use]
pub fn format_vm_name(name: &str, vm: Option<&str>) -> String {
    if let Some(vm_name) = vm {
        format!("microvm@{vm_name}.service")
    } else {
        format!("microvm@{name}-vm.service")
    }
}

#[must_use]
pub fn parse_vm_name(name: &str) -> Option<&str> {
    name.strip_prefix("microvm@")?.strip_suffix(".service")
}

#[must_use]
pub fn format_service_name(name: &str, vm: Option<&str>) -> String {
    if let Some(vm_name) = vm {
        format!("givc-{vm_name}.service")
    } else {
        format!("givc-{name}-vm.service")
    }
}

/// # Errors
/// Return `Err()` if parsing fails
pub fn parse_service_name(name: &str) -> anyhow::Result<&str> {
    name.strip_suffix("-vm.service")
        .and_then(|name| name.strip_prefix("givc-"))
        .with_context(|| format!("Doesn't know how to parse VM name: {name}"))
}

/// From `agent` code, ported for future
/// # Errors
/// Return `Err()` if parsing fails
pub fn parse_application_name(name: &str) -> anyhow::Result<(&str, i32)> {
    if let Some(name_no_suffix) = name.strip_suffix(".service") {
        if let Some((left, right)) = name_no_suffix.rsplit_once('@') {
            let num = right
                .parse()
                .with_context(|| format!("While parsing number part of {name}"))?;
            return Ok((left, num));
        }
    }
    bail!("App name {} not it app@<number>.service format", name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_service_name() -> anyhow::Result<()> {
        let good = parse_service_name("givc-good-vm.service")?;
        assert_eq!(good, "good");

        let bad = parse_service_name("just-a.service");
        let err = bad.unwrap_err();
        assert_eq!(
            format!("{}", err.root_cause()),
            "Doesn't know how to parse VM name: just-a.service"
        );
        Ok(())
    }

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
