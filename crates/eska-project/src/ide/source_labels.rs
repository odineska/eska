//! Bounded, IO-free SCM captions using the same Designer XML path model as `eska diff`.

use std::path::{Component, Path};

use eska_presentation::metadata::render_metadata_path;
use eska_project_core::project::{ProjectType, metadata};
use serde_json::{Value, json};

use super::{dto, envelope::error, params};

/// Resolve relative source paths even when an added or removed file cannot be parsed.
pub(super) fn labels(
    labels: &dto::Labels,
    project_type: ProjectType,
    args: &Value,
) -> Result<Value, Value> {
    let paths: Vec<params::Path> = params::decode(&args["paths"])?;
    if paths.len() > 512 {
        return Err(error(-32602));
    }
    let paths = paths
        .iter()
        .map(params::Path::native)
        .collect::<Result<Vec<_>, _>>()?;
    let captions = paths
        .iter()
        .map(|path| {
            let normalized = source_path(path)?;
            let logical = normalized
                .as_deref()
                .and_then(|path| metadata::from_path(project_type, path.as_bytes().into()));
            Ok(json!({
                "path": dto::path(path),
                "translations": logical.map(|logical| json!({
                    "ru-RU": render_metadata_path(&logical, &labels.ru),
                    "en-US": render_metadata_path(&logical, &labels.en),
                })),
            }))
        })
        .collect::<Result<Vec<Value>, Value>>()?;
    Ok(json!({"labels": captions}))
}

/// Keep native separators separate from literal Unix backslashes and reject escaped roots.
fn source_path(path: &Path) -> Result<Option<String>, Value> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || cfg!(windows) && path.as_os_str().to_string_lossy().contains(':')
    {
        return Err(error(-32602));
    }
    Ok(path
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captions_reuse_diff_notation_without_reading_sources() {
        let files = [
            "Documents/ВыдачаЗайма/Ext/ObjectModule.bsl",
            "Documents/ВыдачаЗайма/Forms/Основная/Ext/Form.xml",
            "CommonModules/ОбщийМодуль/Ext/Module.bsl",
            "notes/untracked.txt",
        ];
        let args = json!({"paths": files.map(|path| dto::path(Path::new(path)))});
        let result = labels(
            &dto::Labels::new().unwrap(),
            ProjectType::Configuration,
            &args,
        )
        .unwrap();
        let captions = result["labels"].as_array().unwrap();
        assert_eq!(captions.len(), files.len());
        assert_eq!(
            captions[0]["translations"]["ru-RU"],
            "Документ.ВыдачаЗайма.МодульОбъекта"
        );
        assert_eq!(
            captions[0]["translations"]["en-US"],
            "Document.ВыдачаЗайма.ObjectModule"
        );
        assert_eq!(
            captions[1]["translations"]["ru-RU"],
            "Документ.ВыдачаЗайма.Форма.Основная"
        );
        assert_eq!(
            captions[2]["translations"]["ru-RU"],
            "ОбщийМодуль.ОбщийМодуль"
        );
        assert!(captions[3]["translations"].is_null());
        for (caption, file) in captions.iter().zip(files) {
            assert_eq!(caption["path"], dto::path(Path::new(file)));
        }
    }

    #[test]
    fn captions_reject_outside_paths_and_bound_batches() {
        let localizers = dto::Labels::new().unwrap();
        for path in ["", "../Documents/Loan.xml", "/Documents/Loan.xml"] {
            let args = json!({"paths": [dto::path(Path::new(path))]});
            assert!(labels(&localizers, ProjectType::Configuration, &args).is_err());
        }
        let oversized = vec![dto::path(Path::new("Configuration.xml")); 513];
        assert!(
            labels(
                &localizers,
                ProjectType::Configuration,
                &json!({"paths": oversized})
            )
            .is_err()
        );
        assert_eq!(
            labels(
                &localizers,
                ProjectType::Configuration,
                &json!({"paths": []})
            )
            .unwrap(),
            json!({"labels": []})
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_unix_names_do_not_become_windows_separators() {
        let path = Path::new("Documents/Имя\\Суффикс/Ext/ObjectModule.bsl");
        let result = labels(
            &dto::Labels::new().unwrap(),
            ProjectType::Configuration,
            &json!({"paths": [dto::path(path)]}),
        )
        .unwrap();
        assert_eq!(
            result["labels"][0]["translations"]["ru-RU"],
            "Документ.Имя\\Суффикс.МодульОбъекта"
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_separators_are_normalized() {
        let path = Path::new(r"Documents\Имя с пробелом\Ext\ObjectModule.bsl");
        let result = labels(
            &dto::Labels::new().unwrap(),
            ProjectType::Configuration,
            &json!({"paths": [dto::path(path)]}),
        )
        .unwrap();
        assert_eq!(
            result["labels"][0]["translations"]["ru-RU"],
            "Документ.Имя с пробелом.МодульОбъекта"
        );
        for path in [
            r"C:\outside.xml",
            r"C:outside.xml",
            r"\\server\share\file.xml",
        ] {
            assert!(source_path(Path::new(path)).is_err());
        }
    }
}
