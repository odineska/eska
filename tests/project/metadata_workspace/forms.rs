use super::*;
use crate::backend::project::{
    forms::{FormContextError, FormOverlay},
    metadata_workspace::FormPreviewError,
};

mod native;

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
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), None)
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
        .form_preview(
            &node,
            Some("app"),
            &[dirty],
            &preview.dependencies,
            Vec::new(),
            None,
        )
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
fn form_palette_preserves_each_extensions_declared_property_and_attribute() {
    use crate::backend::project::forms::PaletteValue;

    let (directory, mut workspace) = setup();
    let baseline = r#"<ChildItems><UsualGroup name="Group" id="1" DisplayImportance="Low"><Group>Vertical</Group></UsualGroup></ChildItems>"#;
    let first = baseline
        .replace("Low", "High")
        .replace("</UsualGroup>", "<United>false</United></UsualGroup>");
    let second = baseline.replace(r#"id="1""#, r#"id="1" xmlns:f="urn:extra" f:hint="second""#);
    let first_xml = form(&first, Some(baseline));
    for (member, xml) in [
        ("base", form(baseline, None)),
        ("one", first_xml.clone()),
        ("two", form(&second, Some(baseline))),
    ] {
        fs::write(directory.0.join(member).join("src").join(PATH), xml).unwrap();
    }
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("base".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), None)
        .unwrap();
    let element = &preview.form.root.children[0];
    let palette = preview.property_palette(&element.identity).unwrap();
    let group = preview
        .presentation
        .elements
        .iter()
        .find(|group| group.identity == element.identity)
        .unwrap();
    assert_eq!(group.united, Some(false));
    let rows = palette.rows.unwrap();
    let first_source = preview
        .dependencies
        .iter()
        .find(|dependency| {
            dependency.member.as_deref() == Some("one") && dependency.path == Path::new(PATH)
        })
        .unwrap();
    let value = |id| {
        &rows
            .iter()
            .find(|row| row.definition.is_some_and(|definition| definition.id == id))
            .unwrap()
            .value
    };
    let PaletteValue::Qualifier {
        attribute,
        extension,
    } = value("DisplayImportance")
    else {
        panic!("missing declared DisplayImportance attribute");
    };
    assert!(*extension);
    assert_eq!(attribute.text, "High");
    assert_eq!(attribute.origin.source, first_source.id);
    assert_eq!(
        &first_xml[attribute.origin.range.clone()],
        r#"DisplayImportance="High""#
    );
    let PaletteValue::Declared {
        index, extension, ..
    } = value("United")
    else {
        panic!("missing declared United property");
    };
    assert!(*extension);
    assert_eq!(element.properties[*index].origin.source, first_source.id);
}

#[test]
fn handler_modules_follow_effective_property_provenance_and_recheck_context() {
    let (directory, mut workspace) = setup();
    let base = BODY.replace(
        "</InputField>",
        "<Events><Event name=\"OnChange\">Changed</Event></Events></InputField>",
    );
    for member in ["base", "one", "two"] {
        let body = if member == "one" {
            base.replace("Changed</Event>", "ExtensionChanged</Event>")
        } else {
            base.clone()
        };
        fs::write(
            directory.0.join(member).join("src").join(PATH),
            form(&body, (member != "base").then_some(base.as_str())),
        )
        .unwrap();
        let module = directory
            .0
            .join(member)
            .join("src/Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form/Module.bsl");
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        fs::write(module, "// existing module").unwrap();
    }
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("one".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), None)
        .unwrap();
    let source = project
        .form_handler_source(&node, Some("app"), &preview, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(source.name, "ExtensionChanged");
    assert_eq!(source.root, directory.0.join("one/src"));
    assert_eq!(
        source.path,
        std::path::Path::new("Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form/Module.bsl")
    );
    assert!(
        project
            .form_handler_source(&node, Some("app"), &preview, 99, None)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        project.form_handler_source(&node, Some("other"), &preview, 0, None),
        Err(FormPreviewError::Form(FormContextError::MemberMismatch))
    ));
    assert_eq!(bytes(&directory.0), before);
    fs::remove_file(source.root.join(&source.path)).unwrap();
    assert!(
        project
            .form_handler_source(&node, Some("app"), &preview, 0, None)
            .unwrap()
            .is_none()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            directory.0.join("base/src").join(&source.path),
            source.root.join(&source.path),
        )
        .unwrap();
        assert!(
            project
                .form_handler_source(&node, Some("app"), &preview, 0, None)
                .is_err()
        );
    }
}

#[test]
fn form_preparation_keeps_checked_input_locations_when_payloads_are_invalid() {
    let (directory, mut workspace) = setup();
    let project = workspace
        .project_mut(&ProjectScope::Member(
            crate::backend::project::ProjectName::parse("one".into()).unwrap(),
        ))
        .unwrap();
    let node = node(project);
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), None)
        .unwrap();
    let expected = preview
        .dependencies
        .iter()
        .filter(|input| input.role == crate::backend::project::forms::FormDependencyRole::Form)
        .map(|input| (&input.id, &input.root, &input.path))
        .collect::<Vec<_>>();
    for (_, root, path) in &expected {
        fs::write(root.join(path), "<broken").unwrap();
    }
    let before = bytes(&directory.0);
    let prepared = project.prepare_form(&node, Some("app"), None).unwrap();
    assert_eq!(
        prepared
            .forms
            .iter()
            .map(|input| (&input.id, &input.root, &input.path))
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(prepared.roots.len(), 3);
    assert_eq!(prepared.context_fingerprint, preview.context_fingerprint);
    assert!(matches!(
        project.form_preview(&node, Some("app"), &[], &[], Vec::new(), None),
        Err(FormPreviewError::Form(FormContextError::Parse { .. }))
    ));
    let inputs = prepared
        .forms
        .into_iter()
        .map(|input| crate::backend::project::forms::FormInput {
            text: preview
                .documents
                .iter()
                .find(|document| document.source == input.id)
                .unwrap()
                .input()
                .into(),
            root: input.root,
            path: input.path,
            version: "1".into(),
        })
        .collect();
    let recovered = project
        .form_preview(&node, Some("app"), &[], &[], inputs, None)
        .unwrap();
    assert_eq!(recovered.documents.len(), 3);
    assert!(
        recovered
            .dependencies
            .iter()
            .filter(|input| input.role == crate::backend::project::forms::FormDependencyRole::Form)
            .all(|input| input.version.as_deref() == Some("1"))
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
        project.form_preview(&node, Some("other"), &[], &[], Vec::new(), None),
        Err(FormPreviewError::Form(FormContextError::MemberMismatch))
    ));
    let overlay = FormOverlay {
        source: "unrelated".into(),
        text: form("", None),
        version: "1".into(),
    };
    assert!(matches!(
        project.form_preview(&node, Some("app"), &[overlay], &[], Vec::new(), None),
        Err(FormPreviewError::Form(FormContextError::InvalidOverlay))
    ));
    assert!(matches!(
        project.form_preview(project.root(), Some("app"), &[], &[], Vec::new(), None),
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
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
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
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
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
        .form_preview(
            &node,
            None,
            &[overlay],
            &preview.dependencies,
            Vec::new(),
            None,
        )
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

fn reference_path_fixture(root: &Path) {
    use std::fmt::Write as _;

    fixture(root, "configuration");
    let source = root.join("src");
    let attribute = |id: u16, name: &str, title: &str, xml_type: &str| {
        format!(
            r#"<Attribute uuid="305a3340-34ad-4e84-85b5-{id:012x}"><Properties><Name>{name}</Name><Synonym><v8:item xmlns:v8="http://v8.1c.ru/8.1/data/core"><v8:lang>ru</v8:lang><v8:content>{title}</v8:content></v8:item></Synonym><Type xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" xmlns:xs="http://www.w3.org/2001/XMLSchema"><v8:Type>{xml_type}</v8:Type></Type></Properties></Attribute>"#
        )
    };
    let owner = source.join("Catalogs/Контрагенты.xml");
    let original = fs::read_to_string(&owner).unwrap();
    let (children, tail) = original.rsplit_once("</ChildObjects>").unwrap();
    fs::write(
        owner,
        format!(
            "{children}{}{}</ChildObjects>{tail}",
            attribute(1, "Link", "Договор", "cfg:CatalogRef.Agreement"),
            attribute(2, "NotUsed", "Без поля формы", "cfg:CatalogRef.Unused"),
        ),
    )
    .unwrap();

    let mut declarations = String::new();
    for (id, name, fields) in [
        (
            6,
            "Agreement",
            attribute(3, "Company", "Организация", "cfg:CatalogRef.Company"),
        ),
        (
            7,
            "Company",
            attribute(4, "Value", "Сокращённое имя", "xs:string"),
        ),
        (
            8,
            "Alternate",
            attribute(5, "Value", "Активна", "xs:boolean"),
        ),
        (9, "Unused", String::new()),
    ] {
        write!(declarations, "<Catalog>{name}</Catalog>").unwrap();
        fs::write(
            source.join(format!("Catalogs/{name}.xml")),
            format!(
                r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Catalog uuid="305a3340-34ad-4e84-85b5-{id:012x}"><Properties><Name>{name}</Name></Properties><ChildObjects>{fields}</ChildObjects></Catalog></MetaDataObject>"#
            ),
        )
        .unwrap();
    }
    let configuration = source.join("Configuration.xml");
    fs::write(
        &configuration,
        fs::read_to_string(&configuration)
            .unwrap()
            .replace("</ChildObjects>", &format!("{declarations}</ChildObjects>")),
    )
    .unwrap();
    fs::write(
        source.join(PATH),
        form(
            r#"<ChildItems><LabelField name="Value" id="1"><DataPath>Object.Link.Company.Value</DataPath></LabelField></ChildItems><Attributes><Attribute name="Object" id="1"><Type xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config"><v8:Type>cfg:CatalogObject.Контрагенты</v8:Type></Type></Attribute></Attributes>"#,
            None,
        ),
    )
    .unwrap();
}

#[test]
fn form_reference_paths_load_only_used_descriptors_and_rebind_on_dirty_type_change() {
    use crate::backend::project::forms::{FieldValueKind, PresentationText};

    let directory = TestDir::new();
    reference_path_fixture(&directory.0);
    let source = directory.0.join("src");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    let paths = |preview: &crate::backend::project::forms::FormPreview| {
        preview
            .dependencies
            .iter()
            .map(|input| input.path.clone())
            .collect::<Vec<_>>()
    };
    assert!(paths(&preview).contains(&PathBuf::from("Catalogs/Company.xml")));
    assert!(!paths(&preview).contains(&PathBuf::from("Catalogs/Unused.xml")));
    let field = &preview.presentation.elements[1];
    assert_eq!(field.value_type.kind, FieldValueKind::String);
    assert!(
        matches!(&field.caption, PresentationText::Literal { items } if items[0].content == "Сокращённое имя")
    );
    let agreement = preview
        .dependencies
        .iter()
        .find(|input| input.path == Path::new("Catalogs/Agreement.xml"))
        .unwrap();
    let overlay = FormOverlay {
        source: agreement.id.clone(),
        text: fs::read_to_string(source.join(&agreement.path))
            .unwrap()
            .replace("CatalogRef.Company", "CatalogRef.Alternate"),
        version: "5".into(),
    };
    let changed = project
        .form_preview(
            &node,
            None,
            &[overlay],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert!(!paths(&changed).contains(&PathBuf::from("Catalogs/Company.xml")));
    let alternate = changed
        .dependencies
        .iter()
        .find(|input| input.path == Path::new("Catalogs/Alternate.xml"))
        .unwrap();
    let new_field = &changed.presentation.elements[1];
    assert_eq!(new_field.identity, field.identity);
    assert_eq!(new_field.value_type.kind, FieldValueKind::Boolean);
    assert_eq!(new_field.binding.as_ref().unwrap().source, alternate.id);
    assert!(
        matches!(&new_field.caption, PresentationText::Literal { items } if items[0].content == "Активна")
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
        .form_preview(&form_node, Some("app"), &[], &[], Vec::new(), None)
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
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), Some(&state))
        .unwrap();
    assert_eq!(preview.documents.len(), 1);
    let base_fingerprint = preview.context_fingerprint;
    state.disabled.remove(&path_key(&directory.0.join("two")));
    let preview = project
        .form_preview(&node, Some("app"), &[], &[], Vec::new(), Some(&state))
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
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
            None
        ),
        Err(FormPreviewError::Form(
            FormContextError::InvalidMetadata { .. }
        ))
    ));
    assert_eq!(bytes(&directory.0), before);
}

fn write_native_style(directory: &Path) -> String {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-style-values");
    let root = directory.join("src");
    for folder in ["Styles", "StyleItems"] {
        copy(&fixture.join(folder), &root.join(folder));
    }
    let config = root.join("Configuration.xml");
    let text = fs::read_to_string(&config).unwrap().replacen(
        "</Properties>",
        "<DefaultStyle>Style.ProbeStyleValues</DefaultStyle></Properties>",
        1,
    );
    fs::write(config, text).unwrap();
    fs::write(root.join(PATH), form("<ChildItems xmlns:s=\"http://v8.1c.ru/8.1/data/ui/style\"><InputField name=\"Styled\" id=\"1\"><Font ref=\"s:ProbeFont\" italic=\"true\"/><TextColor>s:ProbeColor</TextColor></InputField></ChildItems>", None)).unwrap();
    fs::read_to_string(root.join("Styles/ProbeStyleValues/Ext/Style.xml")).unwrap()
}

#[test]
fn native_style_payload_is_a_dirty_tracked_dependency_without_source_writes() {
    use crate::backend::project::forms::{FormDependencyRole, ResolvedColor};
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let style_text = write_native_style(&directory.0);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    let dependency = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml"))
        .unwrap();
    assert_eq!(dependency.role, FormDependencyRole::Metadata);
    assert_eq!(
        preview.presentation.elements[1].foreground,
        ResolvedColor::Rgb("#9800ff".into())
    );
    assert!((preview.presentation.elements[1].font.points - 18.0).abs() < f64::EPSILON);
    assert!(preview.presentation.elements[1].font.italic);
    assert_eq!(preview.presentation.elements[1].font.weight, 700);
    assert_eq!(
        preview.presentation.elements[1].font.origins[0].source,
        dependency.id
    );
    let changed = project
        .form_preview(
            &node,
            None,
            &[FormOverlay {
                source: dependency.id.clone(),
                version: "88".into(),
                text: style_text
                    .replace("#9800FF", "#008000")
                    .replace("height=\"18\"", "height=\"20\""),
            }],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        changed.presentation.elements[1].foreground,
        ResolvedColor::Rgb("#008000".into())
    );
    assert!((changed.presentation.elements[1].font.points - 20.0).abs() < f64::EPSILON);
    let next = changed
        .dependencies
        .iter()
        .find(|next| next.id == dependency.id)
        .unwrap();
    assert_eq!(next.version.as_deref(), Some("88"));
    assert_ne!(next.snapshot, dependency.snapshot);
    assert_eq!(
        changed.form.root.children[0].identity,
        preview.form.root.children[0].identity
    );
    assert!(matches!(project.form_preview(&node, None, &[FormOverlay {
        source: dependency.id.clone(), version: "89".into(), text: "<Style>".into(),
    }], &preview.dependencies, Vec::new(), None), Err(FormPreviewError::Form(
        FormContextError::InvalidMetadata { source })
    ) if source == dependency.id));
    assert!(matches!(
        project.form_preview(
            &node,
            None,
            &[FormOverlay {
                source: format!("foreign:{}", dependency.id),
                version: "89".into(),
                text: style_text,
            }],
            &preview.dependencies,
            Vec::new(),
            None
        ),
        Err(FormPreviewError::Form(FormContextError::InvalidOverlay))
    ));
    assert_eq!(bytes(&directory.0), before);
}

fn write_native_palette(directory: &Path) -> String {
    write_native_style(directory);
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-style-palette");
    let root = directory.join("src");
    let style_text = fs::read_to_string(fixtures.join("Style.xml")).unwrap();
    fs::write(
        root.join("Styles/ProbeStyleValues/Ext/Style.xml"),
        &style_text,
    )
    .unwrap();
    fs::copy(fixtures.join("Group.xml"), root.join(PATH)).unwrap();
    style_text
}

#[test]
fn taxi_palette_tracks_dirty_style_values_and_configuration_selection() {
    use crate::backend::project::forms::ResolvedColor;
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let style_text = write_native_palette(&directory.0);
    let root = directory.0.join("src");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    let style = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml"))
        .unwrap();
    let config = preview
        .dependencies
        .iter()
        .find(|dependency| dependency.path == Path::new("Configuration.xml"))
        .unwrap();
    let config_text = fs::read_to_string(root.join("Configuration.xml")).unwrap();
    assert_eq!(
        preview.presentation.elements[1].title_color,
        ResolvedColor::Rgb("#9800ff".into())
    );
    let style_overlay = FormOverlay {
        source: style.id.clone(),
        version: "90".into(),
        text: style_text.replace("#9800FF", "#008000"),
    };
    let changed = project
        .form_preview(
            &node,
            None,
            std::slice::from_ref(&style_overlay),
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        changed.presentation.elements[1].title_color,
        ResolvedColor::Rgb("#008000".into())
    );
    let disabled = FormOverlay {
        source: config.id.clone(),
        version: "91".into(),
        text: config_text.replace("<DefaultStyle>Style.ProbeStyleValues</DefaultStyle>", ""),
    };
    let fallback = project
        .form_preview(
            &node,
            None,
            &[disabled],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        fallback.presentation.elements[1].title_color,
        ResolvedColor::Role("GroupTitleTextColor".into())
    );
    assert!(
        !fallback
            .dependencies
            .iter()
            .any(|dependency| dependency.id == style.id)
    );
    let restored = project
        .form_preview(
            &node,
            None,
            &[
                style_overlay,
                FormOverlay {
                    source: config.id.clone(),
                    version: "92".into(),
                    text: config_text,
                },
            ],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        restored.presentation.elements[1].title_color,
        ResolvedColor::Rgb("#008000".into())
    );
    assert_eq!(
        restored.form.root.children[0].identity,
        preview.form.root.children[0].identity
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn style_item_named_like_internal_profile_color_loads_and_tracks_dirty_selected_values() {
    use crate::backend::project::forms::{ElementKey, ResolvedColor};
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    write_native_style(&directory.0);
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-style-color-roles");
    let root = directory.0.join("src");
    fs::copy(
        fixtures.join("TextColor.xml"),
        root.join("StyleItems/TextColor.xml"),
    )
    .unwrap();
    fs::copy(fixtures.join("Alias.xml"), root.join(PATH)).unwrap();
    let body = fs::read_to_string(fixtures.join("Style.xml")).unwrap();
    fs::write(root.join("Styles/ProbeStyleValues/Ext/Style.xml"), &body).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    assert!(
        preview
            .dependencies
            .iter()
            .any(|value| value.path == Path::new("StyleItems/TextColor.xml"))
    );
    let index = preview
        .presentation
        .elements
        .iter()
        .position(|value| value.identity.key == ElementKey::Xml(10))
        .unwrap();
    assert_eq!(
        preview.presentation.elements[index].foreground,
        ResolvedColor::Rgb("#123456".into())
    );
    let source = preview
        .dependencies
        .iter()
        .find(|value| value.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml"))
        .unwrap();
    let changed = project
        .form_preview(
            &node,
            None,
            &[FormOverlay {
                source: source.id.clone(),
                version: "93".into(),
                text: body.replace("#123456", "#654321"),
            }],
            &preview.dependencies,
            Vec::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        changed.presentation.elements[index].identity,
        preview.presentation.elements[index].identity
    );
    assert_eq!(
        changed.presentation.elements[index].foreground,
        ResolvedColor::Rgb("#654321".into())
    );
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn named_title_font_tracks_dirty_style_and_definition_without_changing_element_identity() {
    use crate::backend::project::forms::ElementKey;
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    write_native_style(&directory.0);
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forms/native-font-roles");
    let root = directory.0.join("src");
    let definition = fs::read_to_string(fixtures.join("TitleFont.xml")).unwrap();
    fs::write(root.join("StyleItems/TitleFont.xml"), &definition).unwrap();
    fs::copy(fixtures.join("AliasTitle.xml"), root.join(PATH)).unwrap();
    let body = fs::read_to_string(fixtures.join("Style.xml")).unwrap();
    fs::write(root.join("Styles/ProbeStyleValues/Ext/Style.xml"), &body).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    let index = preview
        .presentation
        .elements
        .iter()
        .position(|value| value.identity.key == ElementKey::Xml(10))
        .unwrap();
    assert_eq!(preview.presentation.elements[index].font.points, 18.0);
    assert_eq!(preview.presentation.elements[index].font.weight, 700);
    let dependency = |path: &str| {
        preview
            .dependencies
            .iter()
            .find(|value| value.path == Path::new(path))
            .unwrap()
            .id
            .clone()
    };
    let style_source = dependency("Styles/ProbeStyleValues/Ext/Style.xml");
    let definition_source = dependency("StyleItems/TitleFont.xml");
    let start = body.find("<Item name=\"StyleItem.TitleFont\">").unwrap();
    let end = start + body[start..].find("</Item>").unwrap() + "</Item>".len();
    let fallback = format!("{}{}", &body[..start], &body[end..]);
    for (version, style_text, definition_text, points) in [
        (
            "94",
            body.replace("height=\"18\"", "height=\"16\""),
            definition.clone(),
            16.0,
        ),
        ("95", fallback.clone(), definition.clone(), 12.0),
        (
            "96",
            fallback,
            definition.replace("height=\"12\"", "height=\"14\""),
            14.0,
        ),
        ("97", body.clone(), definition.clone(), 18.0),
    ] {
        let changed = project
            .form_preview(
                &node,
                None,
                &[
                    FormOverlay {
                        source: style_source.clone(),
                        version: version.into(),
                        text: style_text,
                    },
                    FormOverlay {
                        source: definition_source.clone(),
                        version: version.into(),
                        text: definition_text,
                    },
                ],
                &preview.dependencies,
                Vec::new(),
                None,
            )
            .unwrap();
        let element = &changed.presentation.elements[index];
        assert_eq!(
            element.identity,
            preview.presentation.elements[index].identity
        );
        assert_eq!(element.font.points, points);
        assert_eq!(element.font.weight, 700);
        let expected_source = if version == "95" || version == "96" {
            &definition_source
        } else {
            &style_source
        };
        assert!(
            element
                .font
                .origins
                .iter()
                .any(|origin| &origin.source == expected_source)
        );
    }
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn style_payload_discovers_its_own_references_and_missing_payload_keeps_defaults() {
    use crate::backend::project::forms::ResolvedColor;
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let body = write_native_style(&directory.0);
    let root = directory.0.join("src");
    let extra = fs::read_to_string(root.join("StyleItems/ProbeColor.xml"))
        .unwrap()
        .replace("<Name>ProbeColor</Name>", "<Name>FromStyleOnly</Name>")
        .replace(
            "9b0b36c8-cfee-4969-899e-22349996c7f8",
            "11111111-2222-3333-4444-555555555555",
        )
        .replace("web:DarkOrange", "#223344");
    fs::write(root.join("StyleItems/FromStyleOnly.xml"), extra).unwrap();
    let path = root.join("Styles/ProbeStyleValues/Ext/Style.xml");
    fs::write(&path, body.replace("#9800FF", "style:FromStyleOnly")).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let node = node(project);
    let before = bytes(&directory.0);
    let preview = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    assert_eq!(
        preview.presentation.elements[1].foreground,
        ResolvedColor::Rgb("#223344".into())
    );
    assert!(
        preview
            .dependencies
            .iter()
            .any(|dependency| dependency.path == Path::new("StyleItems/FromStyleOnly.xml"))
    );
    assert!(preview.presentation.diagnostics.is_empty());
    assert_eq!(bytes(&directory.0), before);
    fs::remove_file(&path).unwrap();
    let before = bytes(&directory.0);
    let fallback = project
        .form_preview(&node, None, &[], &[], Vec::new(), None)
        .unwrap();
    assert!(!fallback.dependencies.iter().any(|dependency| dependency.path == Path::new("Styles/ProbeStyleValues/Ext/Style.xml")));
    assert_eq!(
        fallback.presentation.elements[1].foreground,
        ResolvedColor::Web("DarkOrange".into())
    );
    assert!((fallback.presentation.elements[1].font.points - 12.0).abs() < f64::EPSILON);
    assert!(fallback.presentation.diagnostics.is_empty());
    assert_eq!(bytes(&directory.0), before);
}
