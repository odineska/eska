//! Read only the connection selected from a local 1C launcher list.

use std::{env, fs::File, io::Read, path::PathBuf};

use eska_project_core::project::local_settings::Infobase;
use serde::Deserialize;

const MAX_LIST_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredInfobase {
    name: String,
    list: Option<PathBuf>,
}

impl RegisteredInfobase {
    pub fn resolve(self) -> Option<Infobase> {
        let path = self.list.or_else(default_list)?;
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
        if contents.len() > MAX_LIST_BYTES {
            return None;
        }
        connection(selected_connection(&contents, &self.name)?)
    }
}

fn default_list() -> Option<PathBuf> {
    let (variable, suffix) = if cfg!(windows) {
        ("APPDATA", "1C/1CEStart/ibases.v8i")
    } else {
        ("HOME", ".1C/1cestart/ibases.v8i")
    };
    Some(PathBuf::from(env::var_os(variable)?).join(suffix))
}

fn selected_connection<'a>(contents: &'a str, name: &str) -> Option<&'a str> {
    let mut selected = false;
    let mut seen = false;
    let mut connection = None;
    for line in contents
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
    {
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            selected = section == name;
            if selected && std::mem::replace(&mut seen, true) {
                return None;
            }
        } else if selected
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("Connect")
            && connection.replace(value.trim()).is_some()
        {
            return None;
        }
    }
    connection
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
        let result = selected_connection(list, "Демо").and_then(connection);
        assert!(
            matches!(result, Some(Infobase::Cluster {host, name}) if host == "host;peer" && name == "test")
        );
        assert!(selected_connection(list, "demo").is_none());
        assert!(selected_connection("[x]\nConnect=a\n[x]\n", "x").is_none());
        assert!(selected_connection("[x]\nConnect=a\nConnect=b", "x").is_none());
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
