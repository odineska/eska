use super::*;
use eska_project_core::project::forms::FormPreviewInput;

fn style_inputs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-style-composition")
}

fn style_matrix() -> Value {
    serde_json::from_str(&fs::read_to_string(style_inputs().join("metrics.json")).unwrap()).unwrap()
}

fn style_project<'a>(
    workspace: &'a mut MetadataWorkspace,
    member: &str,
) -> &'a mut crate::backend::project::metadata_workspace::ProjectSession {
    workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse(member.into()).unwrap(),
        ))
        .unwrap()
}

fn setup_style(case: &Value) -> (TestDir, MetadataWorkspace, NodeId) {
    let directory = TestDir::new();
    for member in ["base", "first", "second"] {
        let root = directory.0.join(member);
        copy(&style_inputs().join(member), &root.join("src"));
        let kind = if member == "base" {
            "configuration"
        } else {
            "extension"
        };
        fs::write(
            root.join("eska.toml"),
            format!("[project]\nname='{member}'\ntype='{kind}'\n"),
        )
        .unwrap();
    }
    for input in case["inputs"].as_array().unwrap() {
        fs::copy(
            style_inputs().join(input["input"].as_str().unwrap()),
            directory
                .0
                .join(input["member"].as_str().unwrap())
                .join("src")
                .join(input["output"].as_str().unwrap()),
        )
        .unwrap();
    }
    let extensions = case["activeExtensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| format!("{{path='{}'}}", name.as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(directory.0.join("eska.toml"), format!("[workspace]\n[workspace.members.native]\nconfiguration='base'\nextensions=[{extensions}]\n")).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], true).unwrap();
    let project = style_project(&mut workspace, case["selectedMember"].as_str().unwrap());
    let node = project
        .reveal_source(Path::new(case["payload"].as_str().unwrap()))
        .unwrap()
        .unwrap()
        .ancestry
        .last()
        .unwrap()
        .clone();
    (directory, workspace, node)
}

fn assert_style(case: &Value, preview: &FormPreview) {
    let label = case["id"].as_str().unwrap();
    assert!(
        preview.form.diagnostics.is_empty(),
        "{label}: {:?}",
        preview.form.diagnostics
    );
    assert!(
        preview.presentation.diagnostics.is_empty(),
        "{label}: {:?}",
        preview.presentation.diagnostics
    );
    let presentation = |name| {
        let identity = &named(&preview.form.root, name).unwrap().identity;
        preview
            .presentation
            .elements
            .iter()
            .find(|item| &item.identity == identity)
            .unwrap()
    };
    let caption = presentation("Caption");
    for name in ["Caption", "B"] {
        let actual = presentation(name);
        let expected = &case["expected"]["font"];
        assert_eq!(
            actual.font.points,
            expected["points"].as_f64().unwrap(),
            "{label}: {name}"
        );
        assert_eq!(
            u64::from(actual.font.weight),
            expected["weight"].as_u64().unwrap(),
            "{label}: {name}"
        );
        assert_eq!(
            actual.font.italic,
            expected["italic"].as_bool().unwrap(),
            "{label}: {name}"
        );
        assert_eq!(
            actual.font.underline,
            expected["underline"].as_bool().unwrap(),
            "{label}: {name}"
        );
    }
    for name in ["Caption", "A"] {
        assert_eq!(
            serde_json::to_value(&presentation(name).foreground).unwrap(),
            case["expected"]["color"],
            "{label}: {name}"
        );
    }
    assert_eq!(
        serde_json::to_value(&presentation("C").foreground).unwrap(),
        case["expected"]["explicitColor"],
        "{label}"
    );
    let origin = &case["expected"]["fontOrigin"];
    let input = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == origin["member"].as_str()
                && input.path == Path::new(origin["path"].as_str().unwrap())
        })
        .unwrap();
    assert!(
        caption
            .font
            .origins
            .iter()
            .any(|origin| origin.source == input.id),
        "{label}: font provenance"
    );
}

#[test]
fn native_style_composition_matches_static_client_observations_without_writes() {
    for case in style_matrix()["cases"].as_array().unwrap() {
        let (directory, mut workspace, node) = setup_style(case);
        let before = bytes(&directory.0);
        let project = style_project(&mut workspace, case["selectedMember"].as_str().unwrap());
        let preview = project
            .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
            .unwrap();
        assert_style(case, &preview);
        assert_eq!(bytes(&directory.0), before);
    }
}

#[test]
fn native_style_inherited_default_loads_extension_values_without_redeclaring_default_style() {
    let matrix = style_matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "styles-inherited-own-items")
        .unwrap();
    let (directory, mut workspace, node) = setup_style(case);
    let before = bytes(&directory.0);
    let project = style_project(&mut workspace, "first");
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    assert!(
        preview
            .dependencies
            .iter()
            .any(|input| input.member.as_deref() == Some("first")
                && input.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml")),
        "inherited style extension payload must be a dependency"
    );
    assert_style(case, &preview);
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn native_style_dirty_selection_preserves_defining_layer_and_element_identity() {
    let matrix = style_matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "styles-adopted-base-items")
        .unwrap();
    let fallback = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "styles-active-base-items")
        .unwrap();
    let updated = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "styles-base-updated")
        .unwrap();
    let (directory, mut workspace, node) = setup_style(case);
    let before = bytes(&directory.0);
    let project = style_project(&mut workspace, "base");
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    assert_style(case, &preview);
    let configuration = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("first") && input.path == Path::new("Configuration.xml")
        })
        .unwrap();
    let original = fs::read_to_string(configuration.root.join(&configuration.path)).unwrap();
    let selected = FormOverlay {
        source: configuration.id.clone(),
        text: original.replace("Style.ProbeStyleValues", "Style.F_ReportStyle"),
        version: "1".into(),
    };
    assert_ne!(selected.text, original);
    let changed = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: std::slice::from_ref(&selected),
                previous: &preview.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_style(fallback, &changed);
    assert_eq!(changed.form.root.identity, preview.form.root.identity);
    let base_style = changed
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("base")
                && input.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml")
        })
        .unwrap();
    let style = FormOverlay {
        source: base_style.id.clone(),
        text: fs::read_to_string(style_inputs().join("variants/BaseUpdatedStyle.xml")).unwrap(),
        version: "2".into(),
    };
    let changed_again = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: &[selected, style.clone()],
                previous: &changed.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_style(fallback, &changed_again);
    let restored = FormOverlay {
        source: configuration.id.clone(),
        text: original,
        version: "3".into(),
    };
    let recovered = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: &[restored, style],
                previous: &changed_again.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_style(updated, &recovered);
    assert_eq!(recovered.form.root.identity, preview.form.root.identity);
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn style_reference_cycle_across_layers_is_bounded_and_keeps_sources_unchanged() {
    let matrix = style_matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "styles-owned-items")
        .unwrap();
    let (directory, mut workspace, node) = setup_style(case);
    fs::copy(
        style_inputs().join("variants/BaseNoStyleConfiguration.xml"),
        directory.0.join("base/src/Configuration.xml"),
    )
    .unwrap();
    for (path, target) in [
        ("base/src/StyleItems/ProbeFont.xml", "F_ProbeFont"),
        ("first/src/StyleItems/F_ProbeFont.xml", "ProbeFont"),
    ] {
        let path = directory.0.join(path);
        let mut text = fs::read_to_string(&path).unwrap();
        let range = roxmltree::Document::parse(&text)
            .unwrap()
            .descendants()
            .find(|node| node.has_tag_name(("http://v8.1c.ru/8.3/MDClasses", "Value")))
            .unwrap()
            .range();
        text.replace_range(
            range,
            &format!("<Value xsi:type=\"v8ui:Font\" ref=\"style:{target}\"/>"),
        );
        fs::write(path, text).unwrap();
    }
    let before = bytes(&directory.0);
    let project = style_project(&mut workspace, "first");
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    assert!(
        preview
            .presentation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "style_cycle")
    );
    assert!(preview.dependencies.len() < 32);
    assert_eq!(
        preview
            .dependencies
            .iter()
            .map(|input| &input.id)
            .collect::<BTreeSet<_>>()
            .len(),
        preview.dependencies.len()
    );
    assert_eq!(bytes(&directory.0), before);
}
