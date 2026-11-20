//! Only coverage-affecting inherited settings are interpreted; upstream validates the full schema.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::Failure;
use crate::process::Analyzer;

pub(super) struct Policy {
    pub exclusions: Vec<PathBuf>,
    pub parents: BTreeMap<PathBuf, [u8; 32]>,
}

/// Follow file-relative `extends`, but keep inherited exclusions relative to the final config.
/// Parent bytes include settings we do not interpret, such as diagnostics and library options.
pub(super) fn capture(
    config: &Path,
    contents: &str,
    analyzer: &Analyzer<'_>,
) -> Result<Policy, Failure> {
    let root = config.parent().ok_or_else(invalid_extends)?;
    let mut file = config.to_path_buf();
    let mut table = parse(contents)?;
    let mut visited = BTreeSet::from([identity(config)?]);
    let mut layers = Vec::new();
    let mut parents = BTreeMap::new();
    loop {
        if analyzer.is_cancelled() {
            return Err(crate::Error::Cancelled.into());
        }
        let target = table.remove("extends");
        layers.push(project(&table));
        let Some(target) = target else { break };
        let target = target
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(invalid_extends)?;
        file = parent_path(&file, target)?;
        let canonical = identity(&file)?;
        if !visited.insert(canonical.clone()) {
            return Err(invalid_extends());
        }
        let bytes = read(&file, &canonical)?;
        if identity(&file)? != canonical {
            return Err(Failure::InputsChanged);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid_extends())?;
        table = parse(text)?;
        let relative = canonical.strip_prefix(root).unwrap_or(&canonical);
        let name = relative.to_str().ok_or_else(|| Failure::UnsupportedPath {
            path: canonical.clone(),
        })?;
        let mut hash = Sha256::new();
        hash.update(b"analyzer-parent-v1");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(&bytes);
        parents.insert(file.clone(), hash.finalize().into());
    }
    let mut effective = toml::Table::new();
    for layer in layers.into_iter().rev() {
        merge(&mut effective, layer);
    }
    reject_partial(&effective)?;
    Ok(Policy {
        exclusions: exclusions(&effective, root)?,
        parents,
    })
}

const fn invalid_extends() -> Failure {
    Failure::PartialPolicy { field: "extends" }
}

fn parse(contents: &str) -> Result<toml::Table, Failure> {
    toml::from_str(contents).map_err(|_| invalid_extends())
}

/// Preserve scalar/table replacement while projecting only the fields needed by coverage.
fn project(table: &toml::Table) -> toml::Table {
    [
        ("analysis", &["diff_base", "ignored_authors"][..]),
        ("source", &["exclude"][..]),
    ]
    .into_iter()
    .filter_map(|(key, fields)| {
        let value = table.get(key)?;
        let value = value.as_table().map_or_else(
            || value.clone(),
            |table| {
                toml::Value::Table(
                    table
                        .iter()
                        .filter(|(key, _)| fields.contains(&key.as_str()))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                )
            },
        );
        Some((key.to_owned(), value))
    })
    .collect()
}

/// TOML tables merge recursively; child arrays and scalars replace their parent value.
fn merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(value)) => {
                merge(existing, value);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn reject_partial(table: &toml::Table) -> Result<(), Failure> {
    for (key, field) in [
        ("diff_base", "analysis.diff_base"),
        ("ignored_authors", "analysis.ignored_authors"),
    ] {
        if table
            .get("analysis")
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get(key))
            .is_some_and(|value| match value {
                toml::Value::String(value) => !value.is_empty(),
                toml::Value::Array(value) => !value.is_empty(),
                _ => true,
            })
        {
            return Err(Failure::PartialPolicy { field });
        }
    }
    Ok(())
}

/// Resolve the directory, retaining a symlinked filename's location for its next parent.
fn parent_path(file: &Path, target: &str) -> Result<PathBuf, Failure> {
    let target = Path::new(target);
    let path = if target.is_absolute() {
        target.to_owned()
    } else {
        file.parent().ok_or_else(invalid_extends)?.join(target)
    };
    let directory = path.parent().ok_or_else(invalid_extends)?;
    let name = path.file_name().ok_or_else(invalid_extends)?;
    Ok(resolve_directory(directory)?.join(name))
}

/// Resolve existing prefixes before simplifying missing suffixes, preserving symlink/`..` order.
fn resolve_directory(path: &Path) -> Result<PathBuf, Failure> {
    let mut existing = path;
    let mut missing = Vec::new();
    let mut resolved = loop {
        match fs::canonicalize(existing) {
            Ok(path) => break path,
            Err(source) => {
                let Some(parent) = existing.parent() else {
                    return Err(Failure::Io {
                        path: path.into(),
                        source,
                    });
                };
                missing.push(
                    existing
                        .components()
                        .next_back()
                        .ok_or_else(invalid_extends)?
                        .as_os_str()
                        .to_owned(),
                );
                existing = parent;
            }
        }
    };
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    let mut normalized = PathBuf::new();
    for component in resolved.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component),
        }
    }
    Ok(normalized)
}

fn identity(path: &Path) -> Result<PathBuf, Failure> {
    fs::canonicalize(path).map_err(|source| Failure::Io {
        path: path.into(),
        source,
    })
}

/// Reuse the bounded reader on the resolved target, deliberately allowing parent-name symlinks.
fn read(path: &Path, canonical: &Path) -> Result<Vec<u8>, Failure> {
    match crate::storage::read_optional(canonical) {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(Failure::Io {
            path: path.into(),
            source: io::Error::new(io::ErrorKind::NotFound, "parent config disappeared"),
        }),
        Err(crate::Error::ManagedConflict { .. }) => {
            Err(Failure::UnsupportedPath { path: path.into() })
        }
        Err(error) => Err(error.into()),
    }
}

/// Literal exclusions keep the existing coverage contract and are never rebased to parent files.
fn exclusions(policy: &toml::Table, root: &Path) -> Result<Vec<PathBuf>, Failure> {
    let Some(value) = policy
        .get("source")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("exclude"))
    else {
        return Ok(Vec::new());
    };
    let invalid = || Failure::PartialPolicy {
        field: "source.exclude",
    };
    let values = value.as_array().ok_or_else(invalid)?;
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(invalid)?;
            let mut path = root.to_path_buf();
            for component in Path::new(value).components() {
                match component {
                    Component::Normal(value) => path.push(value),
                    Component::CurDir => {}
                    Component::ParentDir if path != root => {
                        path.pop();
                    }
                    _ => return Err(invalid()),
                }
            }
            if path.try_exists().map_err(|source| Failure::Io {
                path: path.clone(),
                source,
            })? {
                let canonical = identity(&path)?;
                if canonical != path {
                    return Err(Failure::UnsupportedPath { path });
                }
            }
            Ok(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::{sync::atomic::AtomicBool, time::Duration};

    fn resolve(config: &Path, contents: &str) -> Result<Policy, Failure> {
        fs::write(config, contents).expect("child config");
        let cancelled = AtomicBool::new(false);
        capture(
            config,
            contents,
            &Analyzer {
                binary: PathBuf::new(),
                timeout: Duration::from_secs(1),
                cancelled: &cancelled,
                connection_closed: None,
            },
        )
    }

    #[test]
    fn nested_parent_paths_are_file_relative_but_exclusions_are_child_relative() {
        let fixture = TestDir::new();
        let root = fixture.0.join("project");
        let policies = fixture.0.join("policies");
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(policies.join("nested")).unwrap();
        fs::write(policies.join("nested/mid.toml"), "extends='../base.toml'\n").unwrap();
        fs::write(
            policies.join("base.toml"),
            "[source]\nexclude=['src/generated']\n",
        )
        .unwrap();
        let result = resolve(
            &root.join("bsl-analyzer.toml"),
            "extends='../policies/nested/mid.toml'\n",
        )
        .unwrap();
        assert_eq!(result.exclusions, [root.join("src/generated")]);
        assert_eq!(result.parents.len(), 2);
    }

    #[test]
    fn a_missing_directory_cancelled_by_parent_segments_uses_the_existing_prefix() {
        let fixture = TestDir::new();
        fs::write(
            fixture.0.join("parent.toml"),
            "[source]\nexclude=['generated']\n",
        )
        .unwrap();
        let result = resolve(
            &fixture.0.join("bsl-analyzer.toml"),
            "extends='missing/../parent.toml'\n",
        )
        .unwrap();
        assert_eq!(result.exclusions, [fixture.0.join("generated")]);
    }

    #[test]
    fn a_long_acyclic_chain_has_no_arbitrary_depth_limit() {
        let fixture = TestDir::new();
        for index in 0..260 {
            let contents = if index == 259 {
                "[source]\nexclude=['generated']\n".to_owned()
            } else {
                format!("extends='{}.toml'\n", index + 1)
            };
            fs::write(fixture.0.join(format!("{index}.toml")), contents).unwrap();
        }
        let result = resolve(&fixture.0.join("bsl-analyzer.toml"), "extends='0.toml'\n").unwrap();
        assert_eq!(result.parents.len(), 260);
        assert_eq!(result.exclusions, [fixture.0.join("generated")]);
    }

    #[test]
    fn child_arrays_replace_inherited_exclusions_and_reset_partial_policy() {
        let fixture = TestDir::new();
        fs::write(
            fixture.0.join("parent.toml"),
            "[source]\nexclude=['old']\n[analysis]\ndiff_base='main'\nignored_authors=['bot']\n",
        )
        .unwrap();
        let result = resolve(&fixture.0.join("bsl-analyzer.toml"), "extends='parent.toml'\n[source]\nexclude=['new']\n[analysis]\ndiff_base=''\nignored_authors=[]\n").unwrap();
        assert_eq!(result.exclusions, [fixture.0.join("new")]);
        let cleared = resolve(&fixture.0.join("bsl-analyzer.toml"), "extends='parent.toml'\n[source]\nexclude=[]\n[analysis]\ndiff_base=''\nignored_authors=[]\n").unwrap();
        assert_eq!(cleared.exclusions, [] as [PathBuf; 0]);
    }

    #[test]
    fn inherited_partial_modes_are_rejected() {
        let fixture = TestDir::new();
        for (value, field) in [
            ("diff_base='main'", "analysis.diff_base"),
            ("ignored_authors=['bot']", "analysis.ignored_authors"),
        ] {
            fs::write(
                fixture.0.join("parent.toml"),
                format!("[analysis]\n{value}\n"),
            )
            .unwrap();
            assert!(
                matches!(resolve(&fixture.0.join("bsl-analyzer.toml"), "extends='parent.toml'\n"),
                Err(Failure::PartialPolicy { field: actual }) if actual == field)
            );
        }
    }

    #[test]
    fn parent_bytes_change_snapshot_even_for_uninterpreted_diagnostics() {
        let fixture = TestDir::new();
        let parent = fixture.0.join("parent.toml");
        fs::write(&parent, "[diagnostics.parameters]\nMagicNumber=false\n").unwrap();
        let before = resolve(
            &fixture.0.join("bsl-analyzer.toml"),
            "extends='parent.toml'\n",
        )
        .unwrap();
        fs::write(&parent, "[diagnostics.parameters]\nMagicNumber=true\n").unwrap();
        let after = resolve(
            &fixture.0.join("bsl-analyzer.toml"),
            "extends='parent.toml'\n",
        )
        .unwrap();
        assert_ne!(before.parents, after.parents);
        assert_eq!(before.exclusions, after.exclusions);
        assert_eq!(
            fs::read_to_string(parent).unwrap(),
            "[diagnostics.parameters]\nMagicNumber=true\n"
        );
    }

    #[test]
    fn missing_nonfile_invalid_and_cyclic_parents_are_errors() {
        let fixture = TestDir::new();
        let config = fixture.0.join("bsl-analyzer.toml");
        assert!(resolve(&config, "extends='missing.toml'\n").is_err());
        fs::create_dir(fixture.0.join("directory")).unwrap();
        assert!(resolve(&config, "extends='directory'\n").is_err());
        fs::write(fixture.0.join("broken.toml"), "broken=[").unwrap();
        assert!(resolve(&config, "extends='broken.toml'\n").is_err());
        fs::write(
            fixture.0.join("parent.toml"),
            "extends='bsl-analyzer.toml'\n",
        )
        .unwrap();
        assert!(matches!(
            resolve(&config, "extends='parent.toml'\n"),
            Err(Failure::PartialPolicy { field: "extends" })
        ));
        assert!(resolve(&config, "extends=[]\n").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_file_keeps_its_location_and_retargeting_changes_identity() {
        use std::os::unix::fs::symlink;
        let fixture = TestDir::new();
        let links = fixture.0.join("links");
        let real = fixture.0.join("real");
        fs::create_dir(&links).unwrap();
        fs::create_dir(&real).unwrap();
        fs::write(real.join("parent.toml"), "extends='next.toml'\n").unwrap();
        fs::write(
            real.join("next.toml"),
            "[analysis]\ndiff_base='must-not-be-used'\n",
        )
        .unwrap();
        fs::write(links.join("next.toml"), "[source]\nexclude=['generated']\n").unwrap();
        symlink(real.join("parent.toml"), links.join("parent.toml")).unwrap();
        let config = fixture.0.join("bsl-analyzer.toml");
        let before = resolve(&config, "extends='links/parent.toml'\n").unwrap();
        assert_eq!(before.exclusions, [fixture.0.join("generated")]);
        fs::write(real.join("other.toml"), "extends='next.toml'\n").unwrap();
        fs::remove_file(links.join("parent.toml")).unwrap();
        symlink(real.join("other.toml"), links.join("parent.toml")).unwrap();
        let after = resolve(&config, "extends='links/parent.toml'\n").unwrap();
        assert_ne!(before.parents, after.parents);
        assert_eq!(before.exclusions, after.exclusions);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_alias_of_a_parent_is_a_cycle() {
        let fixture = TestDir::new();
        fs::write(fixture.0.join("parent.toml"), "extends='alias.toml'\n").unwrap();
        std::os::unix::fs::symlink("parent.toml", fixture.0.join("alias.toml")).unwrap();
        assert!(matches!(
            resolve(
                &fixture.0.join("bsl-analyzer.toml"),
                "extends='parent.toml'\n"
            ),
            Err(Failure::PartialPolicy { field: "extends" })
        ));
    }
}
