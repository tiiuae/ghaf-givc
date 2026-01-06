/*
 * JsonNode
 *
 * A generic helper for working with JSON objects:
 *  - Create from string or file
 *  - Read nested fields using a path (GetField-style)
 *  - Add or update nested fields using a path
 */

use anyhow::{Result, anyhow};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

/*
 * JsonNode
 *
 * Represents a JSON value that is expected to be an object at the top level.
 * Internally stores a serde_json::Value and provides helper methods to:
 *  - Parse JSON input
 *  - Navigate nested fields
 *  - Mutate fields
 */
#[derive(Debug, Clone)]
pub struct JsonNode {
    data: Value,
}

impl JsonNode {
    /*
     * from_bytes
     *
     *  Parses JSON from a raw byte slice.
     *  Requirements:
     *    - Top-level JSON must be an object.
     *
     *  Returns:
     *    Ok(JsonNode) on success
     */
    fn from_bytes(raw: &[u8]) -> Result<Self> {
        let data: Value =
            serde_json::from_slice(raw).map_err(|e| anyhow!("failed to parse JSON: {e}"))?;

        if !data.is_object() {
            return Err(anyhow!("top-level JSON must be an object"));
        }

        Ok(Self { data })
    }

    /*
     * from_str
     *
     *  Parses JSON from a string.
     *  Delegates to from_bytes().
     *
     *  Example:
     *    let node = JsonNode::from_str(r#"{"type":"file","name":"demo"}"#)?;
     */
    pub fn from_str(json_str: &str) -> Result<Self> {
        Self::from_bytes(json_str.as_bytes())
    }

    /*
     * from_file
     *
     *  Reads a JSON file from disk and parses it into JsonNode.
     *
     *  Parameters:
     *    path - Path to the JSON file
     *
     *  Example:
     *    let node = JsonNode::from_file("/etc/policies/meta.json")?;
     */
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let raw = fs::read(path.as_ref())
            .map_err(|e| anyhow!("failed to read JSON file {:?}: {e}", path.as_ref()))?;
        Self::from_bytes(&raw)
    }

    /*
     * new
     *
     *  Creates a new empty JsonNode with an empty object at the top.
     */
    pub fn new() -> Self {
        Self {
            data: Value::Object(Map::new()),
        }
    }

    /*
     * get_field
     *
     *  Walks nested keys similar to the Go version:
     *
     *    Go:  GetField("policy", "rules", "allow")
     *    Rust: node.get_field(&["policy", "rules", "allow"])
     *
     *  Parameters:
     *    path - slice of key segments representing the nested path
     *
     *  Behavior:
     *    - Returns an empty String if any part of the path does not exist
     *      or is not an object.
     *    - If the final value is a string, the string is returned.
     *    - Otherwise, the value is formatted as JSON and returned.
     */
    pub fn get_field<S>(&self, path: &[S]) -> String
    where
        S: AsRef<str>,
    {
        if path.is_empty() {
            return String::new();
        }

        let mut current = &self.data;

        for key in path {
            let k = key.as_ref();
            let obj = match current.as_object() {
                Some(o) => o,
                None => return String::new(),
            };

            match obj.get(k) {
                Some(v) => current = v,
                None => return String::new(),
            }
        }

        match current {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }

    /*
     * get_value
     *
     *  Returns a reference to the raw serde_json::Value at the given path.
     *
     *  Parameters:
     *    path - slice of key segments
     *
     *  Returns:
     *    Some(&Value) if the full path exists
     *    None otherwise
     *
     *  Useful if you want to manually inspect or cast the value.
     */
    pub fn get_value<S>(&self, path: &[S]) -> Option<&Value>
    where
        S: AsRef<str>,
    {
        if path.is_empty() {
            return None;
        }

        let mut current = &self.data;
        for key in path {
            let k = key.as_ref();
            let obj = current.as_object()?;
            current = obj.get(k)?;
        }
        Some(current)
    }

    /*
     * add_field
     *
     *  Adds or overwrites a field at a nested path.
     *  This is the "set" counterpart to get_field().
     *
     *  Example:
     *    node.add_field(&["policy", "name"], json!("demo"))?;
     *    node.add_field(&["policy", "rules", "allow"], json!(true))?;
     *
     *  Parameters:
     *    path  - path segments as an iterable (e.g. &["a","b","c"])
     *    value - serde_json::Value to set
     *
     *  Behavior:
     *    - Creates intermediate objects as needed.
     *    - Requires path to be non-empty.
     */
    pub fn add_field<I, S>(&mut self, path: I, value: Value) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let keys: Vec<String> = path.into_iter().map(|s| s.as_ref().to_string()).collect();
        if keys.is_empty() {
            return Err(anyhow!("add_field: path must not be empty"));
        }

        /* Ensure top-level is an object */
        if !self.data.is_object() {
            self.data = Value::Object(Map::new());
        }

        let mut current = &mut self.data;

        /* Walk all but the last key, creating objects as needed */
        for key in &keys[..keys.len() - 1] {
            if !current.is_object() {
                *current = Value::Object(Map::new());
            }

            let obj = current.as_object_mut().unwrap();
            current = obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()));
        }

        /* Set the last key */
        let last_key = keys.last().unwrap().clone();
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        let obj = current.as_object_mut().unwrap();
        obj.insert(last_key, value);

        Ok(())
    }

    /*
     * get_keys
     *
     * Returns all keys for an object at a given path.
     *
     * Behavior:
     * - If path is empty, returns the primary (top-level) keys.
     * - If the path points to a JSON object, returns a Vec of its keys.
     * - If the path points to a non-object value (string, int, etc.) or
     * the path does not exist, returns an empty Vec.
     */
    pub fn get_keys<S>(&self, path: &[S]) -> Vec<String>
    where
        S: AsRef<str>,
    {
        let mut current = &self.data;

        /*  1. Navigate to the target path */
        for key in path {
            let k = key.as_ref();
            match current.as_object() {
                Some(obj) => {
                    match obj.get(k) {
                        Some(v) => current = v,
                        None => return Vec::new(), // Path segment doesn't exist
                    }
                }
                None => return Vec::new(), // Current level is not an object, can't go deeper
            }
        }

        /* 2. Extract keys if the final location is an object */
        current
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default()
    }

    /*
     * to_string
     *
     *  Serializes JsonNode to a compact JSON string.
     */
    pub fn to_string(&self) -> Result<String> {
        serde_json::to_string(&self.data).map_err(|e| anyhow!("failed to serialize JSON: {e}"))
    }

    /*
     * to_pretty_string
     *
     *  Serializes JsonNode to a pretty-printed JSON string.
     */
    pub fn to_pretty_string(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.data)
            .map_err(|e| anyhow!("failed to serialize pretty JSON: {e}"))
    }

    /*
     * to_file
     *
     *  Serializes JsonNode as pretty JSON and writes it to a file.
     *
     *  Parameters:
     *    path - output file path
     */
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let s = self.to_pretty_string()?;
        fs::write(path, s)?;
        Ok(())
    }

    /*
     * inner
     *
     *  Returns a reference to the underlying serde_json::Value.
     *  Useful if direct low-level access is needed.
     */
    pub fn inner(&self) -> &Value {
        &self.data
    }

    /*
     * inner_mut
     *
     *  Returns a mutable reference to the underlying serde_json::Value.
     *  Use with care if you bypass the helper methods.
     */
    pub fn inner_mut(&mut self) -> &mut Value {
        &mut self.data
    }
}

/*====================================================*/
/*              Example usage of JsonNode             */
/*====================================================*/
/*
fn main() -> anyhow::Result<()> {
    let mut node = JsonNode::new();

    node.add_field(&["policy", "name"], json!("demo-policy"))?;
    node.add_field(&["policy", "rules", "allow"], json!(true))?;

    let name = node.get_field(&["policy", "name"]);
    println!("policy.name = {}", name);

    println!("{}", node.to_pretty_string()?);

    let mut node2 = JsonNode::from_str(r#"{"type":"file","meta":{"version":"1.0"}}"#)?;
    node2.add_field(&["meta", "version"], json!("2.0"))?;
    let version = node2.get_field(&["meta", "version"]);
    println!("meta.version = {}", version);

    // Get primary keys: ["id", "metadata"]
    let root_keys = node2.get_keys::<&str>(&[]);

    // Get nested keys: ["version"]
    let meta_keys = node.get_keys(&["meta"]);

    // Get non object keys: []
    let meta_keys = node.get_keys(&["type"]);

    Ok(())
}
*/
