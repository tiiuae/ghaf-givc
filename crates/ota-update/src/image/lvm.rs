use super::slot::{Kind, Slot};
use anyhow::{Result, anyhow};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
struct Volume {
    lv_name: String,
    vg_name: String,
    lv_attr: Option<String>,
    lv_size_bytes: Option<u64>,
}

fn parse_lv_size(value: &str) -> Result<u64> {
    if value.is_empty() {
        return Err(anyhow!("empty LV size"));
    }

    let value = value.trim();
    let (number, unit) = value.split_at(value.len() - 1);

    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "k" => 1024_u64,
        "m" => 1024_u64.pow(2),
        "g" => 1024_u64.pow(3),
        "t" => 1024_u64.pow(4),
        _ => return Err(anyhow!("unknown size unit: {unit}")),
    };

    let number = number.replace(',', ".");
    let number: f64 = number
        .parse()
        .map_err(|_| anyhow!("invalid size number: {number}"))?;

    Ok((number * multiplier as f64) as u64)
}

fn volume_from_map(fields: &HashMap<String, String>) -> Result<Volume> {
    let lv_name = fields
        .get("LVM2_LV_NAME")
        .ok_or_else(|| anyhow!("missing LV name"))?
        .to_string();

    let vg_name = fields
        .get("LVM2_VG_NAME")
        .ok_or_else(|| anyhow!("missing VG name"))?
        .to_string();

    let lv_attr = fields.get("LVM2_LV_ATTR").cloned();

    let lv_size_bytes = fields
        .get("LVM2_LV_SIZE")
        .filter(|s| !s.is_empty())
        .map(|s| parse_lv_size(s))
        .transpose()?;

    Ok(Volume {
        lv_name,
        vg_name,
        lv_attr,
        lv_size_bytes,
    })
}

fn parse_lvs_line(line: &str) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();

    for token in line.split_whitespace() {
        let (key, raw_value) = token
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid token: {token}"))?;

        // ожидаем значение в одинарных кавычках
        let value = raw_value
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .ok_or_else(|| anyhow!("invalid quoted value: {raw_value}"))?;

        map.insert(key.to_string(), value.to_string());
    }

    Ok(map)
}

fn parse_lvs_output(output: &str) -> Vec<Volume> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            let fields = parse_lvs_line(line).ok()?;
            volume_from_map(&fields).ok()
        })
        .collect()
}

fn slots_from_volumes(volumes: &[Volume], vg_name: &str) -> Vec<Slot> {
    volumes
        .iter()
        .filter(|v| v.vg_name == vg_name)
        .filter_map(|v| Slot::try_from(v.lv_name.as_str()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lv_size_gb() {
        let size = parse_lv_size("32,00g").unwrap();
        assert_eq!(size, 32 * 1024_u64.pow(3));
    }

    #[test]
    fn parse_lv_size_fractional() {
        let size = parse_lv_size("1,50g").unwrap();
        assert_eq!(size, (1.5 * 1024_f64.powi(3)) as u64);
    }

    #[test]
    fn parse_lvs_line_basic() {
        let line = "LVM2_LV_NAME='fast'  LVM2_VG_NAME='vg0'   LVM2_LV_SIZE='1,00g'";
        let map = parse_lvs_line(line).unwrap();

        assert_eq!(map["LVM2_LV_NAME"], "fast");
        assert_eq!(map["LVM2_VG_NAME"], "vg0");
        assert_eq!(map["LVM2_LV_SIZE"], "1,00g");
    }

    #[test]
    fn volume_from_map_ok() {
        let mut map = HashMap::new();
        map.insert("LVM2_LV_NAME".into(), "fast".into());
        map.insert("LVM2_VG_NAME".into(), "vg0".into());
        map.insert("LVM2_LV_SIZE".into(), "2,00g".into());

        let vol = volume_from_map(&map).unwrap();
        assert_eq!(vol.lv_name, "fast");
        assert_eq!(vol.vg_name, "vg0");
        assert_eq!(vol.lv_size_bytes, Some(2 * 1024_u64.pow(3)));
    }

    #[test]
    fn parse_lvs_output_and_slots() {
        let output = r#"
          LVM2_LV_NAME='root_1.2.3_deadbeef' LVM2_VG_NAME='vg0' LVM2_LV_SIZE='10,00g'
          LVM2_LV_NAME='swap' LVM2_VG_NAME='vg0' LVM2_LV_SIZE='2,00g'
          LVM2_LV_NAME='root_empty' LVM2_VG_NAME='vg1' LVM2_LV_SIZE='5,00g'
        "#;

        let volumes = parse_lvs_output(output);
        assert_eq!(volumes.len(), 3);

        let slots = slots_from_volumes(&volumes, "vg0");
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].kind, Kind::Root);
        assert_eq!(slots[0].version.as_deref(), Some("1.2.3"));
    }
}
