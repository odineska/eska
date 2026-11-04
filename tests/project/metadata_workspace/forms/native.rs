use super::*;
use crate::backend::project::forms::{
    EffectiveElement, ElementKey, ElementKind, FormDependencyRole, FormPresentation, FormPreview,
    PresentationText, ResolvedColor,
};
use serde_json::{Value, json};

mod owners;
mod styles;

fn inputs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-extension-composition")
}

fn cases() -> Value {
    serde_json::from_str(&fs::read_to_string(inputs().join("metrics.json")).unwrap()).unwrap()
}

fn native_setup(case: &Value) -> (TestDir, MetadataWorkspace, NodeId) {
    let directory = TestDir::new();
    for (member, prefix, kind) in [
        ("base", "Base", "configuration"),
        ("first", "First", "extension"),
        ("second", "Second", "extension"),
    ] {
        let root = directory.0.join(member);
        let source = root.join("src");
        fs::create_dir_all(source.join("CommonForms/ProbeCompose/Ext")).unwrap();
        fs::write(
            root.join("eska.toml"),
            format!("[project]\nname='{member}'\ntype='{kind}'\n"),
        )
        .unwrap();
        let configuration = if member == "base" {
            "BaseConfiguration.xml"
        } else {
            case["configurationInputs"][member].as_str().unwrap()
        };
        let payload = if member == "base" {
            case["base"].as_str().unwrap().to_owned()
        } else {
            format!("{prefix}.xml")
        };
        for (input, output) in [
            (configuration.to_owned(), "Configuration.xml"),
            (
                format!("{prefix}Descriptor.xml"),
                "CommonForms/ProbeCompose.xml",
            ),
            (payload, "CommonForms/ProbeCompose/Ext/Form.xml"),
        ] {
            fs::copy(inputs().join(input), source.join(output)).unwrap();
        }
        if member == "base" {
            fs::create_dir_all(source.join("Languages")).unwrap();
            fs::copy(
                inputs().join("Language.xml"),
                source.join("Languages/Русский.xml"),
            )
            .unwrap();
        }
    }
    let extensions = case["activeRegistrationOrder"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| format!("{{path='{}'}}", name.as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        directory.0.join("eska.toml"),
        format!("[workspace]\n[workspace.members.native]\nconfiguration='base'\nextensions=[{extensions}]\n"),
    )
    .unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], true).unwrap();
    let project = native_project(&mut workspace);
    let located = project
        .reveal_source(Path::new("CommonForms/ProbeCompose/Ext/Form.xml"))
        .unwrap()
        .unwrap();
    let node = located.ancestry.last().unwrap().clone();
    (directory, workspace, node)
}

fn native_project(
    workspace: &mut MetadataWorkspace,
) -> &mut crate::backend::project::metadata_workspace::ProjectSession {
    workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("base".into()).unwrap(),
        ))
        .unwrap()
}

fn named<'a>(node: &'a EffectiveElement, name: &str) -> Option<&'a EffectiveElement> {
    if node.name.as_deref() == Some(name) {
        Some(node)
    } else {
        node.children.iter().find_map(|child| named(child, name))
    }
}

fn captions(node: &EffectiveElement, presentation: &FormPresentation, output: &mut Vec<Value>) {
    if matches!(
        node.kind,
        Some(ElementKind::LabelDecoration | ElementKind::UsualGroup)
    ) {
        let name = node.name.as_ref().unwrap();
        let appearance = presentation
            .elements
            .iter()
            .find(|e| e.identity == node.identity)
            .unwrap();
        let PresentationText::Literal { items } = &appearance.caption else {
            panic!(
                "missing native caption for {name}: {:?}",
                appearance.caption
            );
        };
        let title = items.iter().find(|item| item.language == "en").unwrap();
        output.push(json!({"name":name,"title":title.content}));
    }
    for child in &node.children {
        captions(child, presentation, output);
    }
}

fn assert_native(case: &Value, preview: &FormPreview) {
    let id = case["id"].as_str().unwrap();
    assert!(
        preview.form.diagnostics.is_empty(),
        "{id}: {:?}",
        preview.form.diagnostics
    );
    let mut actual = Vec::new();
    captions(&preview.form.root, &preview.presentation, &mut actual);
    assert_eq!(json!(actual), case["expected"]["captionsInOrder"], "{id}");
    let sources: BTreeMap<_, _> = preview
        .dependencies
        .iter()
        .filter(|dependency| dependency.role == FormDependencyRole::Form)
        .map(|dependency| {
            (
                dependency.member.as_deref().unwrap(),
                dependency.id.as_str(),
            )
        })
        .collect();
    let expected_order = std::iter::once("base")
        .chain(
            case["applicationOrder"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap()),
        )
        .map(|member| sources[member])
        .collect::<Vec<_>>();
    assert_eq!(
        preview
            .documents
            .iter()
            .map(|doc| doc.source.as_str())
            .collect::<Vec<_>>(),
        expected_order,
        "{id}"
    );
    let caption = named(&preview.form.root, "Caption").unwrap();
    let title = caption
        .properties
        .iter()
        .find(|p| p.property.key.name == "Title")
        .unwrap();
    assert_eq!(
        title.origin.source,
        sources[case["expected"]["captionSource"].as_str().unwrap()],
        "{id}"
    );
    let b = named(&preview.form.root, "B").unwrap();
    assert_eq!(b.identity.source, sources["base"]);
    assert_eq!(
        b.identity.key,
        ElementKey::Xml(case["expected"]["baseBXmlId"].as_i64().unwrap())
    );
    let appearance = preview
        .presentation
        .elements
        .iter()
        .find(|e| e.identity == caption.identity)
        .unwrap();
    if case["expected"]["blue14PointCaption"].as_bool().unwrap() {
        assert!((appearance.font.points - 14.0).abs() < f64::EPSILON, "{id}");
        assert_eq!(
            appearance.foreground,
            ResolvedColor::Rgb("#0000ff".into()),
            "{id}"
        );
        assert!(
            appearance
                .font
                .origins
                .iter()
                .any(|origin| origin.source == sources["first"])
        );
    }
    if let (Some(first), Some(second)) = (
        named(&preview.form.root, "F_Inserted"),
        named(&preview.form.root, "S_Inserted"),
    ) {
        assert_eq!(first.identity.key, ElementKey::Xml(1000));
        assert_eq!(second.identity.key, ElementKey::Xml(1000));
        assert_ne!(first.identity, second.identity);
    }
}

#[test]
fn native_xml_form_composition_matches_eleven_observed_static_client_cases() {
    for case in cases()["cases"].as_array().unwrap() {
        let (directory, mut workspace, node) = native_setup(case);
        let before = bytes(&directory.0);
        let preview = native_project(&mut workspace)
            .form_preview(&node, Some("native"), &[], &[], Vec::new(), None)
            .unwrap();
        assert_native(case, &preview);
        assert_eq!(bytes(&directory.0), before);
    }
}

#[test]
fn dirty_extension_purpose_reorders_layers_without_changing_element_identity_or_files() {
    let matrix = cases();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "second-addon")
        .unwrap();
    let (directory, mut workspace, node) = native_setup(case);
    let project = native_project(&mut workspace);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, Some("native"), &[], &[], Vec::new(), None)
        .unwrap();
    assert_native(case, &preview);
    let configuration = preview
        .dependencies
        .iter()
        .find(|dep| {
            dep.member.as_deref() == Some("second") && dep.path == Path::new("Configuration.xml")
        })
        .unwrap();
    let text = fs::read_to_string(configuration.root.join(&configuration.path)).unwrap();
    let overlay = FormOverlay {
        source: configuration.id.clone(),
        text: text.replace(">AddOn<", ">Customization<"),
        version: "15".into(),
    };
    let changed = project
        .form_preview(
            &node,
            Some("native"),
            &[overlay],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    let reversed = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "both-reversed")
        .unwrap();
    assert_native(reversed, &changed);
    assert_eq!(
        named(&preview.form.root, "Caption").unwrap().identity,
        named(&changed.form.root, "Caption").unwrap().identity
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn invalid_extension_purpose_does_not_masquerade_as_customization() {
    let matrix = cases();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "second-addon")
        .unwrap();
    let (directory, mut workspace, node) = native_setup(case);
    let project = native_project(&mut workspace);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, Some("native"), &[], &[], Vec::new(), None)
        .unwrap();
    let configuration = preview
        .dependencies
        .iter()
        .find(|dep| {
            dep.member.as_deref() == Some("second") && dep.path == Path::new("Configuration.xml")
        })
        .unwrap();
    for value in ["", "UnknownPurpose"] {
        let overlay = FormOverlay {
            source: configuration.id.clone(),
            text: fs::read_to_string(configuration.root.join(&configuration.path))
                .unwrap()
                .replace(">AddOn<", &format!(">{value}<")),
            version: "16".into(),
        };
        assert!(
            matches!(project.form_preview(&node, Some("native"), &[overlay], &preview.dependencies, Vec::new(), None), Err(FormPreviewError::Form(FormContextError::InvalidMetadata {source})) if source == configuration.id)
        );
    }
    let restored = project
        .form_preview(
            &node,
            Some("native"),
            &[],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_native(case, &restored);
    assert_eq!(bytes(&directory.0), before);
}
