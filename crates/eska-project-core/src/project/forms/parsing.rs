//! Validate the XML envelope before walking only schema-defined element/definition sections.

use std::{collections::HashSet, fmt::Write};

use roxmltree::{Document, Node, ParsingOptions};
use sha2::{Digest, Sha256};

use super::{
    ElementFamily, ElementIdentity, ElementKey, ElementKind, FORM_NAMESPACE, FormDefinition,
    FormDiagnostic, FormDocument, FormElement, FormIssue, FormParseError, MAX_FORM_BYTES,
    MAX_FORM_ELEMENTS,
};
use crate::project::{
    metadata_model::PropertyKey,
    metadata_parser::{self, LocatedProperty, ParseError},
};

/// Parse one current UTF-8 buffer, retaining BOM, comments and original byte coordinates.
///
/// # Errors
/// Rejects invalid source identities, oversized/deep XML and non-managed-form roots.
pub fn parse(source: &str, input: String) -> Result<FormDocument, FormParseError> {
    if source.is_empty() || source.contains('\0') {
        return Err(FormParseError::InvalidSource);
    }
    if input.len() > MAX_FORM_BYTES {
        return Err(FormParseError::TooLarge);
    }
    metadata_parser::check_envelope(&input).map_err(|error| match error {
        ParseError::TooLarge => FormParseError::TooLarge,
        ParseError::TooDeep => FormParseError::TooDeep,
        _ => FormParseError::InvalidXml,
    })?;
    let document = Document::parse_with_options(
        &input,
        ParsingOptions {
            nodes_limit: 262_144,
            ..Default::default()
        },
    )
    .map_err(|_| FormParseError::InvalidXml)?;
    let node = document.root_element();
    if !node.has_tag_name((FORM_NAMESPACE, "Form")) {
        return Err(FormParseError::UnsupportedRoot);
    }
    let mut parser = Parser {
        source,
        identities: HashSet::new(),
        elements: 0,
        diagnostics: Vec::new(),
        attributes: Vec::new(),
        attribute_properties: Vec::new(),
        commands: Vec::new(),
        parameters: Vec::new(),
    };
    let root = parser.element(node, true)?;
    Ok(FormDocument {
        source: source.to_owned(),
        version: node.attribute("version").map(str::to_owned),
        revision: Sha256::digest(input.as_bytes()).iter().fold(
            String::with_capacity(64),
            |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            },
        ),
        root,
        attributes: parser.attributes,
        attribute_properties: parser.attribute_properties,
        commands: parser.commands,
        parameters: parser.parameters,
        diagnostics: parser.diagnostics,
        input,
    })
}

struct Parser<'a> {
    source: &'a str,
    identities: HashSet<i64>,
    elements: usize,
    diagnostics: Vec<FormDiagnostic>,
    attributes: Vec<FormDefinition>,
    attribute_properties: Vec<LocatedProperty>,
    commands: Vec<FormDefinition>,
    parameters: Vec<FormDefinition>,
}

impl Parser<'_> {
    fn issue(&mut self, node: Node<'_, '_>, issue: FormIssue) {
        self.diagnostics.push(FormDiagnostic {
            issue,
            range: node.range(),
            key: key(node),
        });
    }

    fn element(&mut self, node: Node<'_, '_>, root: bool) -> Result<FormElement, FormParseError> {
        self.elements += 1;
        if self.elements > MAX_FORM_ELEMENTS {
            return Err(FormParseError::TooLarge);
        }
        let kind = (node.tag_name().namespace() == Some(FORM_NAMESPACE))
            .then(|| ElementKind::from_xml_name(node.tag_name().name()))
            .flatten();
        if kind.is_none() || (!root && kind == Some(ElementKind::Form)) {
            self.issue(node, FormIssue::UnknownElement);
        }
        let local = if root {
            Some(ElementKey::Root)
        } else if let Some(id) = node.attribute("id").and_then(|id| id.parse::<i64>().ok()) {
            if !self.identities.insert(id) {
                self.issue(node, FormIssue::DuplicateIdentity);
            }
            Some(ElementKey::Xml(id))
        } else {
            self.issue(node, FormIssue::InvalidIdentity);
            None
        };
        let name = node.attribute("name").map(str::to_owned);
        if !root
            && kind != Some(ElementKind::AutoCommandBar)
            && name.as_ref().is_none_or(String::is_empty)
        {
            self.issue(node, FormIssue::MissingName);
        }
        let mut element = FormElement {
            identity: local.map(|key| ElementIdentity {
                source: self.source.to_owned(),
                key,
            }),
            kind,
            xml_name: key(node),
            name,
            range: node.range(),
            qualifiers: qualifiers(node),
            properties: Vec::new(),
            children: Vec::new(),
            auxiliary: Vec::new(),
        };
        let mut sections = HashSet::new();
        self.check_structural_text(node);
        for child in node.children().filter(Node::is_element) {
            let tag = child.tag_name();
            let own = tag.namespace() == Some(FORM_NAMESPACE);
            if own && tag.name() == "ChildItems" {
                self.check_structural_text(child);
                if !sections.insert("ChildItems") {
                    self.issue(child, FormIssue::RepeatedSection);
                }
                for item in child.children().filter(Node::is_element) {
                    let item = self.element(item, false)?;
                    if item.kind.is_some_and(|kind| {
                        matches!(
                            kind.family(),
                            ElementFamily::Auxiliary | ElementFamily::Addition
                        )
                    }) {
                        element.auxiliary.push(item);
                    } else {
                        element.children.push(item);
                    }
                }
            } else if own && root && matches!(tag.name(), "Attributes" | "Commands" | "Parameters")
            {
                if !sections.insert(tag.name()) {
                    self.issue(child, FormIssue::RepeatedSection);
                }
                self.definitions(child);
            } else if own
                && ElementKind::from_xml_name(tag.name()).is_some_and(|kind| {
                    matches!(
                        kind.family(),
                        ElementFamily::Auxiliary | ElementFamily::Addition
                    )
                })
            {
                element.auxiliary.push(self.element(child, false)?);
            } else {
                element.properties.push(self.property(child));
            }
        }
        Ok(element)
    }

    fn property(&mut self, node: Node<'_, '_>) -> LocatedProperty {
        let mut diagnostics = Vec::new();
        let property = metadata_parser::values::property(node, &mut diagnostics);
        for diagnostic in diagnostics {
            self.diagnostics.push(FormDiagnostic {
                issue: FormIssue::UnsupportedProperty,
                range: diagnostic.range,
                key: key(node),
            });
        }
        LocatedProperty {
            property,
            range: node.range(),
        }
    }

    fn definitions(&mut self, section: Node<'_, '_>) {
        self.check_structural_text(section);
        let expected = match section.tag_name().name() {
            "Attributes" => "Attribute",
            "Commands" => "Command",
            "Parameters" => "Parameter",
            _ => return,
        };
        for node in section.children().filter(Node::is_element) {
            if expected == "Attribute"
                && node.has_tag_name((FORM_NAMESPACE, "ConditionalAppearance"))
            {
                let property = self.property(node);
                self.attribute_properties.push(property);
                continue;
            }
            if !node.has_tag_name((FORM_NAMESPACE, expected)) {
                self.issue(node, FormIssue::UnknownDefinition);
            }
            let definition = FormDefinition {
                xml_name: key(node),
                name: node.attribute("name").map(str::to_owned),
                range: node.range(),
                qualifiers: qualifiers(node),
                properties: node
                    .children()
                    .filter(Node::is_element)
                    .map(|child| self.property(child))
                    .collect(),
            };
            match expected {
                "Attribute" => self.attributes.push(definition),
                "Command" => self.commands.push(definition),
                _ => self.parameters.push(definition),
            }
        }
    }
    fn check_structural_text(&mut self, node: Node<'_, '_>) {
        if node
            .children()
            .filter(Node::is_text)
            .any(|text| text.text().is_some_and(|text| !text.trim().is_empty()))
        {
            self.issue(node, FormIssue::UnsupportedStructure);
        }
    }
}

fn key(node: Node<'_, '_>) -> PropertyKey {
    PropertyKey {
        namespace: node.tag_name().namespace().map(str::to_owned),
        name: node.tag_name().name().to_owned(),
    }
}

fn qualifiers(node: Node<'_, '_>) -> Vec<(PropertyKey, String)> {
    node.attributes()
        .map(|attribute| {
            (
                PropertyKey {
                    namespace: attribute.namespace().map(str::to_owned),
                    name: attribute.name().to_owned(),
                },
                attribute.value().to_owned(),
            )
        })
        .collect()
}
