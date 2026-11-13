//! Read only the connection selected from a local 1C launcher list.

use std::{collections::BTreeMap, env, fs::File, io::Read, path::PathBuf};

use eska_project_core::project::local_settings::Infobase;
use serde::{Deserialize, Serialize};

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
    Some(
        entries(&contents)
            .into_iter()
            .filter_map(|(name, value)| {
                Some(Entry {
                    name: name.to_owned(),
                    infobase: connection(value?)?,
                })
            })
            .collect(),
    )
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
