use super::*;
use crate::backend::project::{designer_source::PicturePreview, forms::FormPicture};
use eska_project_core::project::forms::FormPreviewInput;

fn picture_inputs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-picture-composition")
}

fn matrix() -> Value {
    serde_json::from_str(&fs::read_to_string(picture_inputs().join("metrics.json")).unwrap())
        .unwrap()
}

fn picture_project<'a>(
    workspace: &'a mut MetadataWorkspace,
    member: &str,
) -> &'a mut crate::backend::project::metadata_workspace::ProjectSession {
    workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse(member.into()).unwrap(),
        ))
        .unwrap()
}

fn setup_picture(case: &Value) -> (TestDir, MetadataWorkspace, NodeId) {
    let directory = TestDir::new();
    for member in ["base", "first", "second"] {
        let root = directory.0.join(member);
        copy(&picture_inputs().join(member), &root.join("src"));
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
    let target = |path: &str| {
        let (member, path) = path.split_once('/').unwrap();
        directory.0.join(member).join("src").join(path)
    };
    for remove in case["remove"].as_array().unwrap() {
        fs::remove_dir_all(target(remove.as_str().unwrap())).unwrap();
    }
    for input in case["inputs"].as_array().unwrap() {
        let output = target(input["output"].as_str().unwrap());
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::copy(
            picture_inputs().join(input["input"].as_str().unwrap()),
            output,
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
    let node = picture_project(&mut workspace, case["selectedMember"].as_str().unwrap())
        .reveal_source(Path::new(case["payload"].as_str().unwrap()))
        .unwrap()
        .unwrap()
        .ancestry
        .last()
        .unwrap()
        .clone();
    (directory, workspace, node)
}

fn assert_picture(preview: &FormPreview, name: &str, expected: &Value) {
    let identity = &named(&preview.form.root, name).unwrap().identity;
    let picture = &preview
        .presentation
        .elements
        .iter()
        .find(|item| &item.identity == identity)
        .unwrap()
        .picture;
    if expected.is_null() {
        assert!(picture.is_none(), "{name}: {picture:?}");
        return;
    }
    let Some(FormPicture::Common { source }) = picture else {
        panic!("{name}: {picture:?}")
    };
    let resource = preview
        .resources
        .iter()
        .find(|resource| &resource.source == source)
        .unwrap();
    let Some(PicturePreview::Ready(image)) = &resource.picture else {
        panic!("{resource:?}")
    };
    assert!(String::from_utf8_lossy(&image.bytes).contains(expected["color"].as_str().unwrap()));
    assert!(
        preview
            .dependencies
            .iter()
            .any(|input| &input.id == source
                && input.member.as_deref() == expected["member"].as_str())
    );
}

#[test]
fn native_picture_layers_match_static_client_without_writes() {
    for case in matrix()["cases"].as_array().unwrap() {
        let (directory, mut workspace, node) = setup_picture(case);
        let before = bytes(&directory.0);
        let preview = picture_project(&mut workspace, case["selectedMember"].as_str().unwrap())
            .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
            .unwrap();
        assert!(preview.form.diagnostics.is_empty(), "{}", case["id"]);
        assert!(
            preview.presentation.diagnostics.is_empty(),
            "{}: {:?}",
            case["id"],
            preview.presentation.diagnostics
        );
        for (name, expected) in case["expected"].as_object().unwrap() {
            assert_picture(&preview, name, expected);
        }
        assert_eq!(bytes(&directory.0), before);
    }
}

#[test]
fn native_picture_dirty_property_state_clears_and_recovers_without_identity_changes() {
    let matrix = matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "pictures-borrow-only")
        .unwrap();
    let (directory, mut workspace, node) = setup_picture(case);
    let before = bytes(&directory.0);
    let project = picture_project(&mut workspace, "base");
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    let input = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("second")
                && input.path == Path::new("CommonPictures/ProbeIcon.xml")
        })
        .unwrap();
    let extended =
        fs::read_to_string(picture_inputs().join("second/CommonPictures/ProbeIcon.xml")).unwrap();
    for (version, text, expected) in [
        ("1", extended.clone(), Value::Null),
        (
            "2",
            extended.replace(
                "<xr:State>Extended</xr:State>",
                "<wrong:State xmlns:wrong=\"urn:other\">Extended</wrong:State>",
            ),
            case["expected"]["Logo"].clone(),
        ),
        (
            "3",
            fs::read_to_string(picture_inputs().join("variants/SecondBorrowedPicture.xml"))
                .unwrap(),
            case["expected"]["Logo"].clone(),
        ),
    ] {
        let overlay = FormOverlay {
            source: input.id.clone(),
            version: version.into(),
            text,
        };
        let changed = project
            .form_preview(
                &node,
                Some("native"),
                FormPreviewInput {
                    overlays: &[overlay],
                    previous: &preview.dependencies,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert!(changed.presentation.diagnostics.is_empty());
        assert_picture(&changed, "Logo", &expected);
        assert_eq!(
            changed.form.root.children[0].identity,
            preview.form.root.children[0].identity
        );
        assert_eq!(
            changed
                .dependencies
                .iter()
                .find(|dependency| dependency.id == input.id)
                .unwrap()
                .version
                .as_deref(),
            Some(version)
        );
    }
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn native_empty_picture_payload_is_watched_and_missing_payload_is_diagnosed() {
    let matrix = matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "pictures-clear-empty-file")
        .unwrap();
    let (directory, mut workspace, node) = setup_picture(case);
    let project = picture_project(&mut workspace, "base");
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    assert_picture(&preview, "Logo", &Value::Null);
    let binary = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("second") && input.role == FormDependencyRole::Picture
        })
        .unwrap();
    assert!(binary.path.ends_with("Picture.zip"));
    assert_eq!(
        binary.snapshot,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(bytes(&directory.0), before);
    let payload = directory.0.join("second/src").join(&binary.path);
    fs::write(
        &payload,
        fs::read(picture_inputs().join("second/CommonPictures/ProbeIcon/Ext/Picture/Picture.svg"))
            .unwrap(),
    )
    .unwrap();
    let changed = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                previous: &preview.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert_picture(
        &changed,
        "Logo",
        &json!({"member":"second","color":"#2050EE"}),
    );
    assert_ne!(
        changed
            .dependencies
            .iter()
            .find(|input| input.id == binary.id)
            .unwrap()
            .snapshot,
        binary.snapshot
    );
    fs::remove_file(payload).unwrap();
    let missing = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                previous: &changed.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert!(
        missing
            .presentation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "picture_unavailable")
    );
    assert!(matches!(
        missing
            .presentation
            .elements
            .iter()
            .find(|item| item.identity == preview.form.root.children[0].identity)
            .unwrap()
            .picture,
        Some(FormPicture::Missing { .. })
    ));
}

#[test]
fn native_picture_dirty_empty_property_is_distinct_from_broken_reference() {
    let matrix = matrix();
    let case = matrix["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "pictures-base-both")
        .unwrap();
    let (directory, mut workspace, node) = setup_picture(case);
    let before = bytes(&directory.0);
    let project = picture_project(&mut workspace, "base");
    let preview = project
        .form_preview(&node, Some("native"), FormPreviewInput::default(), None)
        .unwrap();
    let input = preview
        .dependencies
        .iter()
        .find(|input| {
            input.member.as_deref() == Some("second")
                && input.path == Path::new("CommonPictures/ProbeIcon/Ext/Picture.xml")
        })
        .unwrap();
    let empty = FormOverlay {
        source: input.id.clone(),
        version: "9".into(),
        text: "<ExtPicture xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\"><Picture/></ExtPicture>"
            .into(),
    };
    let changed = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: &[empty],
                previous: &preview.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert!(changed.presentation.diagnostics.is_empty());
    assert_picture(&changed, "Logo", &Value::Null);
    let broken = FormOverlay {
        source: input.id.clone(),
        version: "10".into(),
        text: picture_description("Missing.svg"),
    };
    let missing = project
        .form_preview(
            &node,
            Some("native"),
            FormPreviewInput {
                overlays: &[broken],
                previous: &changed.dependencies,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    assert!(
        missing
            .presentation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "picture_unavailable")
    );
    assert_eq!(
        missing.form.root.children[0].identity,
        preview.form.root.children[0].identity
    );
    assert_eq!(bytes(&directory.0), before);
}
