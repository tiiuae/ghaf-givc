// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, ensure};
use oci_client::Reference;

#[derive(Clone, PartialEq, Eq)]
pub struct UntaggedReference(Reference);

#[derive(Clone, PartialEq, Eq)]
pub struct TaggedReference(Reference);

impl UntaggedReference {
    #[allow(dead_code)]
    pub(crate) fn into_inner(self) -> Reference {
        self.0
    }

    pub(crate) fn as_ref(&self) -> &Reference {
        &self.0
    }

    pub fn repository_path(&self) -> String {
        format!("{}/{}", self.0.resolve_registry(), self.0.repository())
    }

    pub fn for_tag(&self, tag: &str) -> anyhow::Result<TaggedReference> {
        let value = format!("{}:{}", self.repository_path(), tag);
        value.parse().map_err(|err: anyhow::Error| {
            err.context(format!("invalid tag reference generated from {tag}"))
        })
    }
}

impl TaggedReference {
    #[allow(dead_code)]
    pub(crate) fn into_inner(self) -> Reference {
        self.0
    }

    pub(crate) fn as_ref(&self) -> &Reference {
        &self.0
    }

    pub fn repository_path(&self) -> String {
        format!("{}/{}", self.0.resolve_registry(), self.0.repository())
    }
}

impl Into<Reference> for UntaggedReference {
    fn into(self) -> Reference {
        self.0
    }
}

impl Into<Reference> for TaggedReference {
    fn into(self) -> Reference {
        self.0
    }
}

impl FromStr for UntaggedReference {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let suffix = input.rsplit_once('/').map_or(input, |(_, suffix)| suffix);
        ensure!(
            !suffix.contains(':') && !input.contains('@'),
            "reference for discover must not include tag or digest: {input}"
        );
        let reference: Reference = input
            .parse()
            .map_err(anyhow::Error::new)
            .with_context(|| format!("invalid OCI reference: {input}"))?;
        Ok(Self(reference))
    }
}

impl FromStr for TaggedReference {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> anyhow::Result<Self> {
        let suffix = input.rsplit_once('/').map_or(input, |(_, suffix)| suffix);
        ensure!(
            suffix.contains(':') || input.contains('@'),
            "reference must include tag or digest: {input}"
        );
        let reference: Reference = input
            .parse()
            .map_err(anyhow::Error::new)
            .with_context(|| format!("invalid OCI reference: {input}"))?;
        Ok(Self(reference))
    }
}

impl fmt::Debug for UntaggedReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for TaggedReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for UntaggedReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Display for TaggedReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::{TaggedReference, UntaggedReference};

    #[test]
    fn as_ref_and_into_inner_work() {
        let untagged: UntaggedReference = "registry.example/repo".parse().expect("untagged");
        let tagged: TaggedReference = "registry.example/repo:v1".parse().expect("tagged");

        let _borrowed_untagged = untagged.as_ref();
        let _borrowed_tagged = tagged.as_ref();
        let _owned_untagged = untagged.into_inner();
        let _owned_tagged = tagged.into_inner();
    }
}
