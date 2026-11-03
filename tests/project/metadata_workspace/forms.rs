use super::*;
use crate::backend::project::{
    forms::{FormContextError, FormOverlay},
    metadata_workspace::FormPreviewError,
};

const PATH: &str = "Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form.xml";
const BODY: &str = "<ChildItems><InputField name=\"Поле\" id=\"1\"><Title>Исходный</Title></InputField></ChildItems>";
fn form(body: &str, base: Option<&str>) -> String {
    format!(
        "<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\">{body}{}</Form>",
        base.map_or(String::new(), |base| format!("<BaseForm>{base}</BaseForm>"))
    )
}
fn node(project: &mut crate::backend::project::metadata_workspace::ProjectSession) -> NodeId {
    let catalog = object(MetadataKind::Catalog, "Контрагенты", None);
    project
        .children(&NodeId::Object(catalog.clone()), TreeOptions::default())
        .unwrap();
    NodeId::Object(object(MetadataKind::Form, "ФормаЭлемента", Some(catalog)))
}
fn setup() -> (TestDir, MetadataWorkspace) {
    let directory = TestDir::new();
    for name in ["base", "one", "two"] {
        let root = directory.0.join(name);
        fixture(&root, "configuration");
        fs::write(
            root.join("eska.toml"),
            format!(
                "[project]\nname='{name}'\ntype='{}'\n",
                if name == "base" {
                    "configuration"
                } else {
                    "extension"
                }
            ),
        )
        .unwrap();
        if name != "base" {
            let descriptor = root.join("src/Configuration.xml");
            let text = fs::read_to_string(&descriptor).unwrap().replace("<Version>", "<ConfigurationExtensionPurpose>Customization</ConfigurationExtensionPurpose><Version>");
            fs::write(descriptor, text).unwrap();
        }
        let body = if name == "one" {
            BODY.replace("Исходный", "Первый")
        } else {
            BODY.into()
        };
        fs::write(
            root.join("src").join(PATH),
            form(&body, (name != "base").then_some(BODY)),
        )
        .unwrap();
    }
    fs::write(directory.0.join("eska.toml"), "[workspace]\n[workspace.members.app]\nconfiguration='base'\nextensions=[{path='one'},{path='two',depends_on=['one']}]\n[workspace.members.other]\nconfiguration='base'\nextensions=[{path='two'}]\n").unwrap();
    let workspace = MetadataWorkspace::open(&directory.0, &[], true).unwrap();
    (directory, workspace)
}

#[test]
fn form_context_uses_application_order_dirty_layers_and_per_input_identity_without_writes() {
    let (directory, mut workspace) = setup();
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("one".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], None)
        .unwrap();
    assert_eq!(
        preview
            .dependencies
            .iter()
            .filter(|dependency| dependency.role
                == crate::backend::project::forms::FormDependencyRole::Form)
            .map(|dependency| dependency.member.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["base", "one", "two"]
    );
    assert!(preview.form.diagnostics.is_empty());
    assert!(!preview.limited_context);
    assert_eq!(
        preview.form.root.children[0].properties[0].origin.source,
        preview.dependencies[1].id
    );
    let source = preview.dependencies[2].id.clone();
    let dirty = FormOverlay {
        source,
        text: form(&BODY.replace("Исходный", "Буфер"), Some(BODY)),
        version: "7".into(),
    };
    let changed = project
        .form_preview(&node, Some("app"), &[dirty], &preview.dependencies, None)
        .unwrap();
    assert_eq!(changed.dependencies[2].version.as_deref(), Some("7"));
    assert_eq!(
        changed.form.root.children[0].properties[0].origin.source,
        preview.dependencies[2].id
    );
    assert_eq!(
        changed.form.root.children[0].identity.source,
        preview.dependencies[0].id
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn form_context_rejects_foreign_uploads_and_application_members() {
    let (_directory, mut workspace) = setup();
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("one".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    assert!(matches!(
        project.form_preview(&node, Some("other"), &[], &[], None),
        Err(FormPreviewError::Form(FormContextError::MemberMismatch))
    ));
    let overlay = FormOverlay {
        source: "unrelated".into(),
        text: form("", None),
        version: "1".into(),
    };
    assert!(matches!(
        project.form_preview(&node, Some("app"), &[overlay], &[], None),
        Err(FormPreviewError::Form(FormContextError::InvalidOverlay))
    ));
    assert!(matches!(
        project.form_preview(project.root(), Some("app"), &[], &[], None),
        Err(FormPreviewError::NotManagedForm)
    ));
}

#[test]
fn standalone_borrowed_form_is_explicitly_incomplete() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, form(BODY, Some(BODY))).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let preview = project.form_preview(&node, None, &[], &[], None).unwrap();
    assert_eq!(
        preview.form.diagnostics[0].issue,
        crate::backend::project::forms::CompositionIssue::MissingBase
    );
}

#[test]
fn form_presentation_tracks_dirty_metadata_captions_without_changing_element_identity() {
    use crate::backend::project::forms::PresentationText;
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let text = form(
        "<ChildItems><InputField name=\"Поле\" id=\"1\"><DataPath>Объект.ИНН</DataPath></InputField></ChildItems><Attributes><Attribute name=\"Объект\" id=\"1\"><Type xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"http://v8.1c.ru/8.1/data/enterprise/current-config\"><v8:Type>cfg:CatalogObject.Контрагенты</v8:Type></Type></Attribute></Attributes>",
        None,
    );
    fs::write(directory.0.join("src").join(PATH), text).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let preview = project.form_preview(&node, None, &[], &[], None).unwrap();
    let caption = &preview.presentation.elements[1].caption;
    assert!(
        matches!(caption, PresentationText::Literal { items } if items[0].content == "ИНН для теста")
    );
    let dependency = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.path == Path::new("Catalogs/Контрагенты.xml"))
        .unwrap();
    let original = fs::read_to_string(dependency.root.join(&dependency.path)).unwrap();
    let overlay = FormOverlay {
        source: dependency.id.clone(),
        text: original.replace("ИНН для теста", "Несохранённая подпись"),
        version: "9".into(),
    };
    let before = bytes(&directory.0);
    let changed = project
        .form_preview(&node, None, &[overlay], &preview.dependencies, None)
        .unwrap();
    assert!(
        matches!(&changed.presentation.elements[1].caption, PresentationText::Literal { items } if items[0].content == "Несохранённая подпись")
    );
    assert_eq!(
        changed.form.root.children[0].identity,
        preview.form.root.children[0].identity
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn external_form_resolves_configuration_types_without_composing_an_unrelated_form() {
    use crate::backend::project::forms::{FormDependencyRole, PresentationText};
    let directory = TestDir::new();
    fixture(&directory.0.join("base"), "configuration");
    fixture(&directory.0.join("external"), "processing");
    fs::write(
        directory.0.join("base/eska.toml"),
        "[project]\nname='base'\ntype='configuration'\n",
    )
    .unwrap();
    fs::write(
        directory.0.join("external/eska.toml"),
        "[project]\nname='external'\ntype='processing'\n",
    )
    .unwrap();
    let path = directory
        .0
        .join("external/src/Выгрузка/Forms/Основная/Ext/Form.xml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, form(
        "<ChildItems><InputField name=\"ИНН\" id=\"1\"><DataPath>Контрагент.ИНН</DataPath></InputField></ChildItems><Attributes><Attribute name=\"Контрагент\" id=\"1\"><Type xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"http://v8.1c.ru/8.1/data/enterprise/current-config\"><v8:Type>cfg:CatalogObject.Контрагенты</v8:Type></Type></Attribute></Attributes>", None)).unwrap();
    fs::write(directory.0.join("eska.toml"), "[workspace]\n[workspace.members.app]\nconfiguration='base'\nexternal_objects=['external']\n").unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], true).unwrap();
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("external".into()).unwrap(),
        ))
        .unwrap();
    let root = project.root().clone();
    project.children(&root, TreeOptions::default()).unwrap();
    let NodeId::Object(root_id) = root else {
        panic!("expected root object")
    };
    let form_node = NodeId::Object(object(MetadataKind::Form, "Основная", Some(root_id)));
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&form_node, Some("app"), &[], &[], None)
        .unwrap();
    assert_eq!(preview.documents.len(), 1);
    assert!(
        preview
            .dependencies
            .iter()
            .any(|dependency| dependency.role == FormDependencyRole::Metadata
                && dependency.member.as_deref() == Some("base")
                && dependency.path == Path::new("Catalogs/Контрагенты.xml"))
    );
    assert!(
        matches!(&preview.presentation.elements[1].caption, PresentationText::Literal { items } if items[0].content == "ИНН для теста")
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn form_context_excludes_disabled_extensions_but_retains_required_dependencies() {
    use crate::backend::project::activity::{State, path_key};
    let (directory, mut workspace) = setup();
    let mut state = State::default();
    for name in ["one", "two"] {
        let scope =
            ProjectScope::Member(crate::backend::project::ProjectName::parse(name.into()).unwrap());
        let project = workspace.project(&scope).unwrap();
        state.disabled.insert(
            path_key(project.project().root()),
            project.activity_summary(),
        );
    }
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("base".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Some(&state))
        .unwrap();
    assert_eq!(preview.documents.len(), 1);
    let base_fingerprint = preview.context_fingerprint;
    state.disabled.remove(&path_key(&directory.0.join("two")));
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Some(&state))
        .unwrap();
    assert_eq!(
        preview.documents.len(),
        3,
        "the active second extension requires the first"
    );
    assert_ne!(preview.context_fingerprint, base_fingerprint);
}

fn picture_description(name: &str) -> String {
    format!(
        "<ExtPicture xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\" xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\"><Picture><xr:Abs>{name}</xr:Abs></Picture></ExtPicture>"
    )
}

fn write_common_picture(directory: &Path) {
    let root = directory.join("src");
    let pictures = root.join("CommonPictures/Icon/Ext/Picture");
    fs::create_dir_all(&pictures).unwrap();
    fs::write(root.join("CommonPictures/Icon.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><CommonPicture uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>Icon</Name></Properties></CommonPicture></MetaDataObject>").unwrap();
    fs::write(
        root.join("CommonPictures/Icon/Ext/Picture.xml"),
        picture_description("One.svg"),
    )
    .unwrap();
    let image = |color| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect width=\"16\" height=\"16\" fill=\"{color}\"/></svg>"
        )
    };
    fs::write(pictures.join("One.svg"), image("red")).unwrap();
    fs::write(pictures.join("Two.svg"), image("blue")).unwrap();
}

#[test]
fn form_common_picture_tracks_payload_dirty_descriptor_and_rejects_binary_overlays() {
    use crate::backend::project::{
        designer_source::PicturePreview,
        forms::{FormDependencyRole, FormPicture},
    };
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    write_common_picture(&directory.0);
    fs::write(directory.0.join("src").join(PATH), form("<ChildItems><PictureDecoration name=\"Logo\" id=\"1\"><Picture xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\"><xr:Ref>CommonPicture.Icon</xr:Ref><xr:LoadTransparent>false</xr:LoadTransparent></Picture></PictureDecoration></ChildItems>", None)).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project.form_preview(&node, None, &[], &[], None).unwrap();
    assert!(preview.presentation.diagnostics.is_empty());
    assert!(
        matches!(&preview.presentation.elements[1].picture, Some(FormPicture::Common { source }) if preview.resources.iter().any(|resource| &resource.source == source))
    );
    let binary = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.role == FormDependencyRole::Picture)
        .unwrap();
    assert!(binary.path.ends_with("One.svg"));
    let descriptor = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.path == Path::new("CommonPictures/Icon/Ext/Picture.xml"))
        .unwrap();
    let changed = project
        .form_preview(
            &node,
            None,
            &[FormOverlay {
                source: descriptor.id.clone(),
                version: "7".into(),
                text: picture_description("Two.svg"),
            }],
            &preview.dependencies,
            None,
        )
        .unwrap();
    let image = changed
        .resources
        .iter()
        .find_map(|resource| match &resource.picture {
            Some(PicturePreview::Ready(picture)) => Some(picture),
            _ => None,
        })
        .unwrap();
    assert_eq!(image.file_name, "Two.svg");
    assert!(String::from_utf8_lossy(&image.bytes).contains("blue"));
    assert_eq!(
        changed.form.root.children[0].identity,
        preview.form.root.children[0].identity
    );
    assert!(matches!(
        project.form_preview(
            &node,
            None,
            &[FormOverlay {
                source: binary.id.clone(),
                version: "8".into(),
                text: "invalid binary overlay".into()
            }],
            &preview.dependencies,
            None
        ),
        Err(FormPreviewError::Form(FormContextError::InvalidOverlay))
    ));
    assert!(matches!(
        project.form_preview(
            &node,
            None,
            &[FormOverlay {
                source: descriptor.id.clone(),
                version: "8".into(),
                text: "<".into()
            }],
            &preview.dependencies,
            None
        ),
        Err(FormPreviewError::Form(
            FormContextError::InvalidMetadata { .. }
        ))
    ));
    assert_eq!(bytes(&directory.0), before);
}
