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
    let preview = project.form_preview(&node, Some("app"), &[]).unwrap();
    assert_eq!(
        preview
            .dependencies
            .iter()
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
    let changed = project.form_preview(&node, Some("app"), &[dirty]).unwrap();
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
        project.form_preview(&node, Some("other"), &[]),
        Err(FormPreviewError::Form(FormContextError::MemberMismatch))
    ));
    let overlay = FormOverlay {
        source: "unrelated".into(),
        text: form("", None),
        version: "1".into(),
    };
    assert!(matches!(
        project.form_preview(&node, Some("app"), &[overlay]),
        Err(FormPreviewError::Form(FormContextError::InvalidOverlay))
    ));
    assert!(matches!(
        project.form_preview(project.root(), Some("app"), &[]),
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
    let preview = project.form_preview(&node, None, &[]).unwrap();
    assert_eq!(
        preview.form.diagnostics[0].issue,
        crate::backend::project::forms::CompositionIssue::MissingBase
    );
}
