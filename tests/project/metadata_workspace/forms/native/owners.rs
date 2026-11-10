use super::*;
use eska_project_core::project::forms::FormPreviewInput;

fn owner_inputs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-owner-composition")
}

fn observations() -> Value {
    serde_json::from_str(&fs::read_to_string(owner_inputs().join("metrics.json")).unwrap()).unwrap()
}

fn setup_owner(case: &Value) -> (TestDir, MetadataWorkspace, NodeId) {
    let directory = TestDir::new();
    for member in ["base", "first", "second"] {
        let input = if member == "second" {
            case["secondInput"].as_str().unwrap_or("second")
        } else {
            member
        };
        let root = directory.0.join(member);
        copy(&owner_inputs().join(input), &root.join("src"));
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
    let extensions = case["activeExtensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| format!("{{path='{}'}}", name.as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(directory.0.join("eska.toml"), format!("[workspace]\n[workspace.members.native]\nconfiguration='base'\nextensions=[{extensions}]\n")).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], true).unwrap();
    let node = owner_project(&mut workspace, case)
        .reveal_source(Path::new(case["payload"].as_str().unwrap()))
        .unwrap()
        .unwrap()
        .ancestry
        .last()
        .unwrap()
        .clone();
    (directory, workspace, node)
}

fn owner_project<'a>(
    workspace: &'a mut MetadataWorkspace,
    case: &Value,
) -> &'a mut crate::backend::project::metadata_workspace::ProjectSession {
    workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse(
                case["selectedMember"].as_str().unwrap().into(),
            )
            .unwrap(),
        ))
        .unwrap()
}

fn assert_owner_case(case: &Value, preview: &FormPreview) {
    let label = case["id"].as_str().unwrap();
    assert!(
        preview.form.diagnostics.is_empty(),
        "{label}: {:?}",
        preview.form.diagnostics
    );
    assert!(!preview.limited_context);
    let mut rendered = Vec::new();
    captions(&preview.form.root, &preview.presentation, &mut rendered);
    assert_eq!(
        json!(
            rendered
                .iter()
                .map(|item| item["title"].clone())
                .collect::<Vec<_>>()
        ),
        case["expected"]["captions"],
        "{label}"
    );
    let dependencies: BTreeMap<_, _> = preview
        .dependencies
        .iter()
        .filter(|input| input.role == FormDependencyRole::Form)
        .map(|input| (input.id.as_str(), input.member.as_deref().unwrap()))
        .collect();
    assert_eq!(
        json!(
            preview
                .documents
                .iter()
                .map(|doc| dependencies[doc.source.as_str()])
                .collect::<Vec<_>>()
        ),
        case["expected"]["layers"],
        "{label}"
    );
    let caption = named(&preview.form.root, "Caption").unwrap();
    assert_eq!(
        dependencies[caption.identity.source.as_str()],
        case["expected"]["layers"][0].as_str().unwrap(),
        "{label}"
    );
    let presentation = preview
        .presentation
        .elements
        .iter()
        .find(|item| item.identity == caption.identity)
        .unwrap();
    let points = if case["expected"]["blue14PointCaption"] == true {
        14.0
    } else {
        10.0
    };
    assert_eq!(presentation.font.points, points, "{label}");
    if case["expected"]["blue14PointCaption"] == true {
        assert_eq!(
            presentation.foreground,
            ResolvedColor::Rgb("#0000ff".into()),
            "{label}"
        );
    }
}

#[test]
fn native_owner_forms_match_five_static_client_observations_without_writes() {
    for case in observations()["cases"].as_array().unwrap() {
        let (directory, mut workspace, node) = setup_owner(case);
        let before = bytes(&directory.0);
        let preview = owner_project(&mut workspace, case)
            .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
            .unwrap();
        assert_owner_case(case, &preview);
        assert_eq!(bytes(&directory.0), before);
    }
}

#[test]
fn native_owner_borrowing_other_extensions_metadata_cannot_look_like_valid_composition() {
    for case in observations()["rejectedCases"].as_array().unwrap() {
        let (directory, mut workspace, node) = setup_owner(case);
        let before = bytes(&directory.0);
        let result = owner_project(&mut workspace, case).form_preview(
            &node,
            Some("native"),
            FormPreviewInput::default(),
            None,
        );
        assert!(
            matches!(
                result,
                Err(FormPreviewError::Form(
                    FormContextError::InvalidMetadata { .. }
                ))
            ),
            "{}: invalid borrowing was not rejected",
            case["id"]
        );
        assert_eq!(bytes(&directory.0), before);
    }
}

#[test]
fn native_owner_dirty_borrowing_is_validated_and_recovers_without_replacing_identity() {
    let data = observations();
    let case = data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "extension-own-object")
        .unwrap();
    let (directory, mut workspace, node) = setup_owner(case);
    let before = bytes(&directory.0);
    let project = owner_project(&mut workspace, case);
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    let dependency = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("first")
                && input.path == Path::new("Catalogs/F_OwnCatalog.xml")
        })
        .unwrap();
    let original = fs::read_to_string(dependency.root.join(&dependency.path)).unwrap();
    let dirty = FormOverlay {
        source: dependency.id.clone(),
        text: original.replace(
            "<Name>F_OwnCatalog</Name>",
            "<ObjectBelonging>Adopted</ObjectBelonging><Name>F_OwnCatalog</Name>",
        ),
        version: "1".into(),
    };
    assert_ne!(dirty.text, original);
    assert!(
        matches!(project.form_preview(&node, Some("native"), FormPreviewInput { overlays: &[dirty], previous: &preview.dependencies, documents: Vec::new(), ..Default::default() }, None), Err(FormPreviewError::Form(FormContextError::InvalidMetadata { source })) if source == dependency.id)
    );
    // A foreign extension fragment must not acquire Designer ownership semantics.
    let dirty = FormOverlay { source: dependency.id.clone(), text: original.replace("<Name>F_OwnCatalog</Name>", "<ObjectBelonging xmlns=\"urn:vendor\">Adopted</ObjectBelonging><Name>F_OwnCatalog</Name>"), version: "2".into() };
    let recovered = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: &[dirty],
                previous: &preview.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_eq!(recovered.form.root.identity, preview.form.root.identity);
    assert_owner_case(case, &recovered);
    assert_eq!(bytes(&directory.0), before);
}
