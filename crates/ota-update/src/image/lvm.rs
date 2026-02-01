use super::pipeline::{CommandSpec, Pipeline};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_with::{DisplayFromStr, serde_as};
use std::path::PathBuf;
use tokio::process::Command;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Volume {
    pub lv_name: String,
    pub vg_name: String,

    #[serde(default)]
    pub lv_attr: Option<String>,

    /// Parsed from strings like "34359738368"
    #[serde(default)]
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub lv_size_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct LvsJson {
    report: Vec<LvsReport>,
}

#[derive(Deserialize)]
struct LvsReport {
    lv: Vec<Volume>,
}

impl Volume {
    #[must_use]
    pub fn device_file(&self) -> PathBuf {
        let mut path = PathBuf::from("/dev/mapper");
        path.push(format!("{}-{}", self.vg_name, self.lv_name));
        path
    }

    #[must_use]
    pub fn device_file_string(&self) -> String {
        self.device_file().as_path().to_string_lossy().into_owned()
    }

    pub fn rename_to(&self, new_name: impl AsRef<str>) -> Pipeline {
        Pipeline::new(
            CommandSpec::new("lvrename")
                .arg(&self.vg_name)
                .arg(&self.lv_name)
                .arg(new_name.as_ref()),
        )
    }

    #[cfg(test)]
    pub fn new(name: &str) -> Self {
        Self {
            lv_name: name.to_string(),
            vg_name: "vg0".into(),
            lv_attr: None,
            lv_size_bytes: Some(1_1000_1000_1000),
        }
    }
}

pub(crate) fn parse_lvs_json(input: impl AsRef<[u8]>) -> Result<Vec<Volume>> {
    let parsed: LvsJson = serde_json::from_slice(input.as_ref()).context("parsing lvs json")?;

    Ok(parsed.report.into_iter().flat_map(|r| r.lv).collect())
}

pub(crate) async fn read_lvs_output() -> Result<Vec<Volume>> {
    let output = Command::new("lvs")
        .args([
            "--all",
            "--report-format",
            "json",
            "--units",
            "b",
            "--no-suffix",
        ])
        .env("LC_NUMERIC", "C") // Set locale to C, actually not needed with --units, but let be deterministic
        .output()
        .await
        .context("failed to execute lvs")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("lvs failed: {}", stderr.trim());
    }

    parse_lvs_json(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Version;
    use crate::image::slot::{Kind, Slot};

    #[test]
    fn parse_lvs_output_and_slots() {
        let output = r#"
  {
      "report": [
          {
              "lv": [
                  {"lv_name":"root_1.2.3_deadbeef", "vg_name":"vg0", "lv_attr":"-wi-ao----", "lv_size":"10000000000", "pool_lv":"", "origin":"", "data_percent":"", "metadata_percent":"", "move_pv":"", "mirror_log":"", "copy_percent":"", "convert_lv":""},
                  {"lv_name":"swap", "vg_name":"vg0", "lv_attr":"-wi-ao----", "lv_size":"2000000000", "pool_lv":"", "origin":"", "data_percent":"", "metadata_percent":"", "move_pv":"", "mirror_log":"", "copy_percent":"", "convert_lv":""},
                  {"lv_name":"root_empty", "vg_name":"vg0", "lv_attr":"-wi-ao----", "lv_size":"2000000000", "pool_lv":"", "origin":"", "data_percent":"", "metadata_percent":"", "move_pv":"", "mirror_log":"", "copy_percent":"", "convert_lv":""}
              ]
          }
      ]
      ,
      "log": [
      ]
  }
        "#;

        let volumes = parse_lvs_json(&output).unwrap();
        assert_eq!(volumes.len(), 3);

        let (slots, _) = Slot::from_volumes(volumes);
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].kind(), Kind::Root);
        assert_eq!(
            slots[0].version().as_deref(),
            Some(&Version::new("1.2.3".into(), Some("deadbeef".into())))
        );
    }

    #[test]
    fn device_file() {
        let vol = Volume::new("test");
        assert_eq!(
            vol.device_file().as_path(),
            std::path::Path::new("/dev/mapper/vg0-test")
        )
    }

    #[test]
    fn rename_to() {
        let vol = Volume::new("test");
        let pipeline = vol.rename_to("swap");
        assert_eq!(pipeline.format_shell(), "lvrename vg0 test swap")
    }
}
