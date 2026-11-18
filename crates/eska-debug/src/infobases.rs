//! Read registered infobase connections from 1C launcher lists.

use std::{collections::BTreeMap, env, fs::File, io::Read, path::PathBuf, time::Duration};

use eska_build_runtime::build::{RunnerPreference, ToolOptions};
use eska_project_core::{config::global, project::local_settings::Infobase};
use eska_tool_runtime::{Component, Operation, QueryPolicy, capture};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_LIST_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredInfobase {
    name: String,
    list: Option<PathBuf>,
}

impl RegisteredInfobase {
    pub fn resolve(self) -> Option<Infobase> {
        let contents = read(self.list)?;
        connection(entries(&contents).get(self.name.as_str())?.as_ref()?)
    }
}

#[derive(Serialize)]
pub struct Entry {
    name: String,
    infobase: Infobase,
}

pub fn list(path: Option<PathBuf>) -> Option<Vec<Entry>> {
    let contents = read(path)?;
    Some(parse_list(&contents))
}

fn parse_list(contents: &str) -> Vec<Entry> {
    entries(contents)
        .into_iter()
        .filter_map(|(name, value)| {
            Some(Entry {
                name: name.to_owned(),
                infobase: connection(value?)?,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Source<'a> {
    Host,
    Distrobox { container: &'a str },
}

#[derive(Serialize)]
struct SourcedEntry<'a> {
    #[serde(flatten)]
    entry: Entry,
    source: Source<'a>,
}

/// Extended discovery preserves each source independently, including unavailable lists.
pub fn list_with_distrobox(path: Option<PathBuf>, explicit: Option<String>) -> Value {
    let container = explicit.or_else(|| {
        let configured = global::load()
            .ok()
            .and_then(|(_, config)| config.build.container);
        ToolOptions::default()
            .with_machine_defaults(RunnerPreference::Auto, None, configured)
            .distrobox_name()
    });
    let mut entries = Vec::new();
    let host = append_list(&mut entries, list(path), Source::Host);
    let distrobox = container.as_deref().map(|container| {
        let status = append_list(
            &mut entries,
            read_distrobox(container),
            Source::Distrobox { container },
        );
        json!({"container": container, "status": status})
    });
    json!({
        "schema_version": 1,
        "kind": "registered-infobases",
        "entries": entries,
        "sources": { "host": host, "distrobox": distrobox },
    })
}

fn append_list<'a>(
    target: &mut Vec<SourcedEntry<'a>>,
    list: Option<Vec<Entry>>,
    source: Source<'a>,
) -> &'static str {
    let Some(list) = list else {
        return "unavailable";
    };
    target.extend(list.into_iter().map(|entry| SourcedEntry { entry, source }));
    "ready"
}

fn read_distrobox(container: &str) -> Option<Vec<Entry>> {
    // Only this fixed script is interpreted; container names remain separate validated arguments.
    // Read one byte beyond the limit to reject oversized lists without sending unbounded data.
    let script = format!(
        "test -n \"${{HOME:-}}\" && test -f \"$HOME/.1C/1cestart/ibases.v8i\" && exec head -c {} -- \"$HOME/.1C/1cestart/ibases.v8i\"",
        MAX_LIST_BYTES + 1
    );
    let mut command =
        ToolOptions::distrobox_query(container, "sh", &["-c".into(), script.into()]).ok()?;
    let operation = Operation::new().ok()?;
    let output = capture(
        Component::Build,
        &mut command,
        &operation,
        QueryPolicy {
            timeout: Duration::from_secs(5),
            output_limit: MAX_LIST_BYTES as u64,
            ..QueryPolicy::default()
        },
        || {},
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let contents = String::from_utf8(output.stdout).ok()?;
    Some(parse_list(&contents))
}

fn read(path: Option<PathBuf>) -> Option<String> {
    let path = path.or_else(default_list)?;
    if !path.is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut contents = String::new();
    file.take((MAX_LIST_BYTES + 1) as u64)
        .read_to_string(&mut contents)
        .ok()?;
    (contents.len() <= MAX_LIST_BYTES).then_some(contents)
}

fn default_list() -> Option<PathBuf> {
    let (variable, suffix) = if cfg!(windows) {
        ("APPDATA", "1C/1CEStart/ibases.v8i")
    } else {
        ("HOME", ".1C/1cestart/ibases.v8i")
    };
    Some(PathBuf::from(env::var_os(variable)?).join(suffix))
}

fn entries(contents: &str) -> BTreeMap<&str, Option<&str>> {
    use std::collections::btree_map::Entry;

    let mut entries = BTreeMap::new();
    let mut section = None;
    for line in contents
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
    {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = match entries.entry(name) {
                Entry::Vacant(entry) if !name.is_empty() => {
                    entry.insert(None);
                    Some(name)
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                    None
                }
                Entry::Vacant(_) => None,
            }
        } else if let Some(name) = section
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("Connect")
            && let Some(entry) = entries.get_mut(name)
            && entry.replace(value.trim()).is_some()
        {
            *entry = None;
            section = None;
        }
    }
    entries
}

fn connection(mut input: &str) -> Option<Infobase> {
    let mut path = None;
    let mut host = None;
    let mut name = None;
    while !input.trim().is_empty() {
        let (key, rest) = input.split_once('=')?;
        let (value, tail) = quoted_value(rest.trim_start())?;
        input = tail.trim_start();
        if !input.is_empty() {
            input = input.strip_prefix(';')?;
        }
        let key = key.trim();
        let slot = if key.eq_ignore_ascii_case("File") {
            &mut path
        } else if key.eq_ignore_ascii_case("Srvr") {
            &mut host
        } else if key.eq_ignore_ascii_case("Ref") {
            &mut name
        } else {
            return None;
        };
        if value.trim().is_empty()
            || value.chars().any(char::is_control)
            || slot.replace(value).is_some()
        {
            return None;
        }
    }
    match (path, host, name) {
        (Some(path), None, None) if std::path::Path::new(&path).is_absolute() => {
            Some(Infobase::File { path: path.into() })
        }
        (None, Some(host), Some(name)) => Some(Infobase::Cluster { host, name }),
        _ => None,
    }
}

fn quoted_value(mut input: &str) -> Option<(String, &str)> {
    input = input.strip_prefix('"')?;
    let mut value = String::new();
    loop {
        let (part, tail) = input.split_once('"')?;
        value.push_str(part);
        if let Some(rest) = tail.strip_prefix('"') {
            value.push('"');
            input = rest;
        } else {
            return Some((value, tail));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_one_named_entry_without_using_launcher_options() {
        let list = "\u{feff}[Демо]\r\nConnect=Srvr=\"host;peer\";Ref=\"test\";\r\nAdditionalParameters=/C Ignored\r\n[Other]\r\nConnect=File=\"unused\";";
        let parsed = entries(list);
        let result = parsed["Демо"].and_then(connection);
        assert!(
            matches!(result, Some(Infobase::Cluster {host, name}) if host == "host;peer" && name == "test")
        );
        assert!(!parsed.contains_key("demo"));
        assert_eq!(entries("[x]\nConnect=a\n[x]\n")["x"], None);
        assert_eq!(entries("[x]\nConnect=a\nConnect=b")["x"], None);
    }

    #[test]
    fn reads_quoted_paths_and_rejects_ambiguous_or_unsupported_connections() {
        let path = env::current_dir().unwrap().join("space;quote");
        let value = format!("File=\"{}\";", path.display());
        assert!(matches!(connection(&value), Some(Infobase::File {path: found}) if found == path));
        assert!(connection("File=\"relative\";").is_none());
        assert!(connection("Srvr=\"host\";Ref=\"base\";Usr=\"ignored\"").is_none());
        assert!(connection("Srvr=\"host\";Ref=\"one\";ref=\"two\"").is_none());
        assert_eq!(
            quoted_value("\"quoted\"\"name\";").unwrap().0,
            "quoted\"name"
        );
    }
}
