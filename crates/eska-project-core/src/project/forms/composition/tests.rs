use super::*;
use crate::project::{
    forms::{FORM_NAMESPACE, FormPart, parse},
    metadata_model::MetadataValue,
};

fn form(source: &str, body: &str, base: Option<&str>) -> FormDocument {
    let base = base.map_or(String::new(), |base| format!("<BaseForm>{base}</BaseForm>"));
    parse(
        source,
        format!("<Form xmlns=\"{FORM_NAMESPACE}\">{body}{base}</Form>"),
    )
    .expect("valid fixture")
}
fn tree(items: &str) -> String {
    format!("<ChildItems>{items}</ChildItems>")
}
fn field(name: &str, id: i64, title: &str) -> String {
    format!("<InputField name=\"{name}\" id=\"{id}\"><Title>{title}</Title></InputField>")
}
fn title(value: &EffectiveElement) -> Option<&str> {
    value.properties.iter().find_map(|value| {
        if value.property.key.name == "Title" {
            if let MetadataValue::Text(text) = &value.property.value {
                Some(text.as_str())
            } else {
                None
            }
        } else {
            None
        }
    })
}

#[test]
fn baselines_have_separate_identity_and_ranges() {
    let input = tree(&field("A", 1, "original"));
    let document = form("ext", &input, Some(&input));
    assert!(!document.has_issues());
    let baseline = document.base.as_ref().expect("baseline");
    assert_eq!(
        baseline.diagnostics,
        [] as [crate::project::forms::FormDiagnostic; 0]
    );
    assert_eq!(
        baseline.root.children[0]
            .identity
            .as_ref()
            .expect("id")
            .part,
        FormPart::StoredBase
    );
    assert_ne!(
        baseline.root.children[0].identity,
        document.root.children[0].identity
    );
    assert_eq!(
        &document.input()[baseline.root.children[0].range.clone()],
        field("A", 1, "original")
    );
}

#[test]
fn independent_extension_changes_survive_later_full_snapshots() {
    let original = tree(&(field("A", 1, "a") + &field("B", 2, "b")));
    let base = form("base", &original, None);
    let first = form(
        "one",
        &tree(&(field("A", 1, "changed a") + &field("B", 2, "b"))),
        Some(&original),
    );
    let second = form(
        "two",
        &tree(&(field("A", 1, "a") + &field("B", 2, "changed b"))),
        Some(&original),
    );
    let result = compose(&base, &[&first, &second]);
    assert!(result.diagnostics.is_empty());
    assert_eq!(title(&result.root.children[0]), Some("changed a"));
    assert_eq!(title(&result.root.children[1]), Some("changed b"));
    assert_eq!(result.root.children[0].identity.source, "base");
    assert_eq!(result.root.children[0].properties[0].origin.source, "one");
    assert_eq!(result.root.children[1].properties[0].origin.source, "two");
}

#[test]
fn later_explicit_override_wins_and_unchanged_base_updates_are_retained() {
    let original = tree(&field("A", 1, "a"));
    let base = form("base", &tree(&field("A", 42, "new base")), None);
    let first = form("one", &original, Some(&original));
    let second = form("two", &tree(&field("A", 1, "second")), Some(&original));
    assert_eq!(
        title(&compose(&base, &[&first]).root.children[0]),
        Some("new base")
    );
    let result = compose(&base, &[&first, &second]);
    assert!(result.diagnostics.is_empty());
    assert_eq!(title(&result.root.children[0]), Some("second"));
    assert_eq!(result.root.children[0].identity.key, ElementKey::Xml(42));
}

#[test]
fn new_nodes_with_same_local_id_in_two_extensions_do_not_collide() {
    let original = tree(&field("A", 1, "a"));
    let base = form("base", &original, None);
    let first = form(
        "one",
        &tree(&(field("A", 1, "a") + &field("B", 2, "b"))),
        Some(&original),
    );
    let second = form(
        "two",
        &tree(&(field("A", 1, "a") + &field("C", 2, "c"))),
        Some(&original),
    );
    let result = compose(&base, &[&first, &second]);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        result
            .root
            .children
            .iter()
            .map(|node| node.name.as_deref().expect("name"))
            .collect::<Vec<_>>(),
        ["A", "C", "B"]
    );
    assert_ne!(
        result.root.children[1].identity,
        result.root.children[2].identity
    );
}

#[test]
fn reorder_keeps_foreign_nodes_and_cross_group_move_has_one_owner() {
    let original = tree(&format!(
        "<UsualGroup name=\"G\" id=\"5\">{}</UsualGroup><UsualGroup name=\"H\" id=\"6\"/>",
        tree(&field("A", 1, "a"))
    ));
    let changed = tree(&format!(
        "<UsualGroup name=\"G\" id=\"5\"/><UsualGroup name=\"H\" id=\"6\">{}</UsualGroup>",
        tree(&field("A", 1, "new"))
    ));
    let result = compose(
        &form("base", &original, None),
        &[&form("ext", &changed, Some(&original))],
    );
    assert!(result.diagnostics.is_empty());
    assert!(result.root.children[0].children.is_empty());
    assert_eq!(title(&result.root.children[1].children[0]), Some("new"));
}

#[test]
fn definitions_keep_independent_unborrowed_values_and_provenance() {
    let base = form(
        "base",
        "<Attributes><Attribute name=\"A\" id=\"1\"><Title>A</Title></Attribute><Attribute name=\"B\" id=\"2\"/></Attributes>",
        None,
    );
    let extension = form(
        "ext",
        "<Attributes><Attribute name=\"A\" id=\"1\"><Title>New</Title></Attribute></Attributes>",
        Some(
            "<Attributes><Attribute name=\"A\" id=\"1\"><Title>A</Title></Attribute></Attributes>",
        ),
    );
    let result = compose(&base, &[&extension]);
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.attributes.len(), 2);
    assert_eq!(result.attributes[0].properties[0].origin.source, "ext");
    assert_eq!(result.attributes[1].origin.source, "base");
}

#[test]
fn missing_base_and_unresolvable_borrowing_are_explicit() {
    let original = tree(&field("A", 1, "a"));
    let extension = form("ext", &original, Some(&original));
    assert_eq!(
        compose(&extension, &[]).diagnostics[0].issue,
        CompositionIssue::MissingBase
    );
    let result = compose(&form("base", "", None), &[&extension]);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|issue| issue.issue == CompositionIssue::MissingBorrowedElement)
    );
    assert!(result.root.children.is_empty());
}
