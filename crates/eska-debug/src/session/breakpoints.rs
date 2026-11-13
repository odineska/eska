//! Translate breakpoint arguments into the platform's conditional stop and output fields.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde_json::Value;
use url::Url;

use super::{Failure, INVALID, optional_expression};
use crate::dbgs::Breakpoint;

const BAD_HIT_COUNT: Failure = "debug-invalid-hit-count";
const BAD_LOG_TEMPLATE: Failure = "debug-invalid-log-template";

/// VS Code sends independent breakpoint lists for the native file and its read-only view.
pub(super) struct Source<'a> {
    pub path: PathBuf,
    pub protected_uri: Option<&'a str>,
}

impl<'a> Source<'a> {
    pub(super) fn parse(path: &'a str) -> Option<Self> {
        let Some(protected_path) = path.strip_prefix("eska-protected:") else {
            return Some(Self {
                path: PathBuf::from(path),
                protected_uri: None,
            });
        };
        if !protected_path.starts_with('/') {
            return None;
        }
        let url = Url::parse(&format!("file:{protected_path}")).ok()?;
        if url.query().is_some() || url.fragment().is_some() {
            return None;
        }
        Some(Self {
            path: url.to_file_path().ok()?,
            protected_uri: Some(path),
        })
    }
}

#[derive(Clone, Default)]
pub(super) struct ModuleBreakpoints {
    native: Vec<Breakpoint>,
    protected: Option<(String, Vec<Breakpoint>)>,
}

impl ModuleBreakpoints {
    pub(super) fn replace(&mut self, uri: Option<&str>, points: Vec<Breakpoint>) {
        if let Some(uri) = uri {
            self.protected = (!points.is_empty()).then(|| (uri.to_owned(), points));
        } else {
            self.native = points;
        }
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.native.is_empty() && self.protected.is_none()
    }

    pub(super) fn contains(&self, line: u32) -> bool {
        self.native.iter().any(|point| point.line == line)
            || self
                .protected
                .as_ref()
                .is_some_and(|(_, points)| points.iter().any(|point| point.line == line))
    }

    pub(super) fn active(&self) -> impl IntoIterator<Item = &Breakpoint> {
        // dbgs has one breakpoint per line; the native entry wins if both views set it.
        self.protected
            .iter()
            .flat_map(|(_, points)| points)
            .chain(&self.native)
            .map(|point| (point.line, point))
            .collect::<BTreeMap<_, _>>()
            .into_values()
    }

    pub(super) fn display_path<'a>(&'a self, path: &'a Path, line: u32) -> Option<&'a str> {
        if let Some((uri, _)) = &self.protected
            && !self.native.iter().any(|point| point.line == line)
        {
            return Some(uri);
        }
        path.to_str()
    }
}

pub(super) fn parse(entry: &Value, line: u32) -> Result<Breakpoint, Failure> {
    let condition = optional_expression(entry, "condition")?.map(str::to_owned);
    let hit_count = optional_expression(entry, "hitCondition")?
        .map(|text| {
            text.trim()
                .parse::<i32>()
                .ok()
                .filter(|count| *count > 0)
                .ok_or(BAD_HIT_COUNT)
        })
        .transpose()?;
    let log_expression = optional_expression(entry, "logMessage")?
        .map(log_expression)
        .transpose()?;
    Ok(Breakpoint {
        line,
        condition,
        hit_count,
        log_expression,
    })
}

fn log_expression(template: &str) -> Result<String, Failure> {
    let mut chars = template.char_indices().peekable();
    let mut parts = Vec::new();
    let mut literal = String::new();
    while let Some((index, character)) = chars.next() {
        match character {
            '{' | '}' if chars.next_if(|&(_, next)| next == character).is_some() => {
                literal.push(character);
            }
            '{' => {
                push_literal(&mut parts, &mut literal);
                let mut in_string = false;
                let mut end = None;
                for (position, symbol) in chars.by_ref() {
                    match symbol {
                        '"' => in_string = !in_string,
                        '{' if !in_string => return Err(BAD_LOG_TEMPLATE),
                        '}' if !in_string => {
                            end = Some(position);
                            break;
                        }
                        _ => {}
                    }
                }
                let expression = template[index + 1..end.ok_or(BAD_LOG_TEMPLATE)?].trim();
                if expression.is_empty() || expression.chars().any(char::is_control) {
                    return Err(BAD_LOG_TEMPLATE);
                }
                parts.push(format!("Строка(({expression}))"));
            }
            '}' => return Err(BAD_LOG_TEMPLATE),
            control if control.is_control() => {
                push_literal(&mut parts, &mut literal);
                parts.push(format!("Символ({})", u32::from(control)));
            }
            _ => literal.push(character),
        }
    }
    push_literal(&mut parts, &mut literal);
    let result = parts.join(" + ");
    if result.is_empty() || result.len() > 65536 {
        return Err(INVALID);
    }
    Ok(result)
}

fn push_literal(parts: &mut Vec<String>, literal: &mut String) {
    if !literal.is_empty() {
        parts.push(format!("\"{}\"", literal.replace('"', "\"\"")));
        literal.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_source_resolves_to_the_same_native_path() {
        #[cfg(windows)]
        let file = Path::new(r"C:\Demo #%\Модуль.bsl");
        #[cfg(not(windows))]
        let file = Path::new("/Demo #%/Модуль.bsl");
        let url = Url::from_file_path(file).unwrap();
        let protected = format!(
            "eska-protected:{}",
            url.as_str().strip_prefix("file:").unwrap()
        );
        let resolved = Source::parse(&protected).unwrap();
        assert_eq!(resolved.path, file);
        assert_eq!(resolved.protected_uri, Some(protected.as_str()));
        assert!(Source::parse("eska-protected:relative.bsl").is_none());
        assert!(Source::parse("eska-protected:/source.bsl?unrelated").is_none());
        assert!(Source::parse("eska-protected:/source.bsl#fragment").is_none());
        #[cfg(windows)]
        {
            assert_eq!(
                Source::parse("eska-protected:/c%3A/Demo%20%23%25/Test.bsl")
                    .unwrap()
                    .path,
                Path::new(r"c:\Demo #%\Test.bsl")
            );
            assert_eq!(
                Source::parse("eska-protected://server/share/Test.bsl")
                    .unwrap()
                    .path,
                Path::new(r"\\server\share\Test.bsl")
            );
        }
    }

    #[test]
    fn breakpoint_lists_can_be_replaced_independently_without_duplicate_lines() {
        let file = Path::new("/sources/Module.bsl");
        let uri = "eska-protected:/sources/Module.bsl";
        let point = |line| Breakpoint {
            line,
            ..Breakpoint::default()
        };
        let mut native = point(2);
        native.condition = Some("A = 1".into());
        let mut module = ModuleBreakpoints::default();
        module.replace(None, vec![point(1), native]);
        module.replace(Some(uri), vec![point(2), point(3)]);
        let lines = || {
            module
                .active()
                .into_iter()
                .map(|point| point.line)
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(), [1, 2, 3]);
        assert_eq!(
            module
                .active()
                .into_iter()
                .nth(1)
                .unwrap()
                .condition
                .as_deref(),
            Some("A = 1")
        );
        assert_eq!(module.display_path(file, 3), Some(uri));
        assert_eq!(module.display_path(file, 2), file.to_str());
        module.replace(None, vec![]);
        assert!(module.contains(2) && !module.contains(1));
        assert_eq!(module.display_path(file, 2), Some(uri));
        module.replace(Some(uri), vec![]);
        assert!(module.is_empty());
    }

    #[test]
    fn log_template_preserves_text_and_compiles_bsl_interpolations() {
        assert_eq!(
            log_expression("текст \"целиком\"").unwrap(),
            "\"текст \"\"целиком\"\"\""
        );
        assert_eq!(
            log_expression("{{Сумма}} = {Сумма + 1}\n{\"}\"}").unwrap(),
            "\"{Сумма} = \" + Строка((Сумма + 1)) + Символ(10) + Строка((\"}\"))",
        );
        for invalid in ["text {", "{}", "{A{B}}", "text }", "{\"unfinished}"] {
            assert_eq!(log_expression(invalid), Err(BAD_LOG_TEMPLATE));
        }
    }
}
