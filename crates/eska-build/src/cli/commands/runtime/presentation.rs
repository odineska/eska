//! Optional bilingual captions using the same metadata notation as `eska diff`.

use std::{collections::BTreeMap, path::PathBuf};

use bstr::ByteSlice;
use eska_build_runtime::runtime::apply::{Plan, application::ApplicationPlan};
use eska_presentation::{
    localization::{Locale, Localizer},
    metadata::{metadata_kind, render_metadata_path},
};
use eska_project_core::project::{ProjectType, metadata};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Caption {
    label: String,
    group: Option<String>,
}

type Display = BTreeMap<PathBuf, BTreeMap<&'static str, Caption>>;

pub(super) fn paths<'a>(paths: impl Iterator<Item = &'a PathBuf>, kind: ProjectType) -> Display {
    let (Ok(ru), Ok(en)) = (
        Localizer::try_new(Locale::RuRu),
        Localizer::try_new(Locale::EnUs),
    ) else {
        // Captions are optional; clients retain the exact paths if presentation is unavailable.
        return Display::new();
    };
    paths
        .map(|path| {
            let normalized = path
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let logical = metadata::from_path(kind, normalized.as_bytes().as_bstr());
            let captions = [("ru-RU", &ru), ("en-US", &en)]
                .into_iter()
                .map(|(locale, localizer)| {
                    let label = logical.as_ref().map_or_else(
                        || path.display().to_string(),
                        |logical| render_metadata_path(logical, localizer),
                    );
                    let group = logical
                        .as_ref()
                        .map(|logical| metadata_kind(logical.group, localizer));
                    (locale, Caption { label, group })
                })
                .collect();
            (path.clone(), captions)
        })
        .collect()
}

pub(super) fn plan(plan: &Plan) -> Display {
    paths(
        plan.changes
            .keys()
            .chain(plan.import_files.iter().flatten())
            .chain(&plan.picture_references),
        if plan.extension.is_some() {
            ProjectType::Extension
        } else {
            ProjectType::Configuration
        },
    )
}

pub(super) fn application(plan: &ApplicationPlan) -> BTreeMap<&str, Display> {
    plan.components
        .iter()
        .map(|component| {
            let display = component.plan.as_ref().map_or_else(
                || {
                    paths(
                        component.review.iter().flat_map(|review| {
                            review
                                .review
                                .changes
                                .iter()
                                .flat_map(|changes| changes.keys())
                        }),
                        ProjectType::Configuration,
                    )
                },
                self::plan,
            );
            (component.member.as_str(), display)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captions_share_diff_notation_and_preserve_unknown_paths() {
        let files = [
            PathBuf::from("Catalogs/Товары/Ext/ManagerModule.bsl"),
            PathBuf::from("custom/<script>.bin"),
        ];
        let display = paths(files.iter(), ProjectType::Configuration);
        assert_eq!(
            display[&files[0]]["ru-RU"].label,
            "Справочник.Товары.МодульМенеджера"
        );
        assert_eq!(
            display[&files[0]]["en-US"].label,
            "Catalog.Товары.ManagerModule"
        );
        assert_eq!(
            display[&files[1]]["ru-RU"].label,
            files[1].display().to_string()
        );
        assert!(display[&files[1]]["en-US"].group.is_none());
    }
}
