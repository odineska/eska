//! Render only newly inserted nodes, reusing visible namespaces and surrounding whitespace style.

use std::fmt::Write;

use roxmltree::Node;

use crate::project::{
    metadata_create::{
        CreateType, CreationError, CreationKind, DateFractions, GeneratedIdentity, PreparedCreate,
        schema::{CORE, MD, XR, XS},
    },
    metadata_edit::{
        EditError, escape_xml,
        types::{CFG, indentation, newline},
    },
    metadata_model::{MetadataKind, PropertyKey},
};

/// Insertion style is local to the owner; unrelated lines never pass through a formatter.
pub(super) struct Layout<'a> {
    indent: Option<&'a str>,
    unit: &'a str,
    newline: &'a str,
}

impl<'a> Layout<'a> {
    pub fn for_owner(input: &'a str, owner: Node<'_, '_>) -> Self {
        let indent = indentation(input, owner.range().start);
        let unit = indent.map_or("", |base| {
            owner
                .children()
                .filter(Node::is_element)
                .filter_map(|child| indentation(input, child.range().start))
                .find_map(|child| child.strip_prefix(base).filter(|unit| !unit.is_empty()))
                .unwrap_or_else(|| if base.contains('\t') { "\t" } else { "  " })
        });
        Self {
            indent,
            unit,
            newline: newline(input),
        }
    }

    pub fn child_separator(&self) -> String {
        self.indent
            .map_or_default(|indent| format!("{}{indent}{}", self.newline, self.unit))
    }

    pub fn parent_separator(&self) -> String {
        self.indent
            .map_or_default(|indent| format!("{}{indent}", self.newline))
    }
}

struct Names {
    md: String,
    core: String,
    xr: String,
    xs: String,
    cfg: String,
    declarations: String,
}

impl Names {
    fn new(owner: Node<'_, '_>, prepared: &PreparedCreate) -> Self {
        let mut declarations = String::new();
        let md = if owner.default_namespace() == Some(MD) {
            String::new()
        } else {
            bind(owner, MD, "md", &mut declarations)
        };
        let core = bind(owner, CORE, "v8", &mut declarations);
        let xs = if prepared
            .request
            .types
            .iter()
            .any(|value| !matches!(value, CreateType::Reference { .. }))
        {
            bind(owner, XS, "xs", &mut declarations)
        } else {
            String::new()
        };
        let cfg = if prepared
            .request
            .types
            .iter()
            .any(|value| matches!(value, CreateType::Reference { .. }))
        {
            bind(owner, CFG, "cfg", &mut declarations)
        } else {
            String::new()
        };
        let xr = if prepared.request.kind == CreationKind::TablePart {
            bind(owner, XR, "xr", &mut declarations)
        } else {
            String::new()
        };
        Self {
            md,
            core,
            xr,
            xs,
            cfg,
            declarations,
        }
    }
}

fn bind(
    owner: Node<'_, '_>,
    namespace: &str,
    preferred: &str,
    declarations: &mut String,
) -> String {
    if let Some(prefix) = owner.lookup_prefix(namespace) {
        return format!("{prefix}:");
    }
    let mut prefix = preferred.to_owned();
    let mut index = 0;
    while owner.lookup_namespace_uri(Some(&prefix)).is_some() {
        index += 1;
        prefix = format!("{preferred}{index}");
    }
    let _ = write!(declarations, " xmlns:{prefix}=\"{namespace}\"");
    format!("{prefix}:")
}

struct Xml<'a, 'b> {
    value: String,
    depth: usize,
    layout: &'a Layout<'b>,
}

impl Xml<'_, '_> {
    fn line(&mut self, value: &str) {
        if !self.value.is_empty()
            && let Some(indent) = self.layout.indent
        {
            self.value.push_str(self.layout.newline);
            self.value.push_str(indent);
            for _ in 0..=self.depth {
                self.value.push_str(self.layout.unit);
            }
        }
        self.value.push_str(value);
    }

    fn open(&mut self, name: &str, attributes: &str) {
        self.line(&format!("<{name}{attributes}>"));
        self.depth += 1;
    }

    fn close(&mut self, name: &str) {
        self.depth -= 1;
        self.line(&format!("</{name}>"));
    }

    fn scalar(&mut self, name: &str, value: &str) -> Result<(), EditError> {
        let text = escape_xml(value)?;
        self.line(&if text.is_empty() {
            format!("<{name}/>")
        } else {
            format!("<{name}>{text}</{name}>")
        });
        Ok(())
    }
}

pub(super) fn declaration(
    owner: Node<'_, '_>,
    namespace_context: Node<'_, '_>,
    prepared: &PreparedCreate,
    type_keys: &[PropertyKey],
    layout: &Layout<'_>,
    missing_group: bool,
) -> Result<String, CreationError> {
    let names = Names::new(namespace_context, prepared);
    let mut xml = Xml {
        value: String::new(),
        depth: 0,
        layout,
    };
    let group = format!("{}ChildObjects", names.md);
    if missing_group {
        xml.open(&group, "");
    }
    let tag = format!(
        "{}{}",
        names.md,
        prepared.request.kind.metadata_kind().designer_tag()
    );
    xml.open(
        &tag,
        &format!(" uuid=\"{}\"{}", prepared.identity.uuid, names.declarations),
    );
    if prepared.request.kind == CreationKind::TablePart {
        internal_info(&mut xml, &names, owner, prepared)?;
    }
    let properties = format!("{}Properties", names.md);
    xml.open(&properties, "");
    xml.scalar(&format!("{}Name", names.md), &prepared.request.name)?;
    synonyms(&mut xml, &names, prepared)?;
    xml.scalar(&format!("{}Comment", names.md), &prepared.request.comment)?;
    if prepared.request.kind == CreationKind::Attribute {
        value_types(&mut xml, &names, prepared, type_keys)?;
    }
    xml.close(&properties);
    if prepared.request.kind == CreationKind::TablePart {
        xml.scalar(&group, "")?;
    }
    xml.close(&tag);
    if missing_group {
        xml.close(&group);
    }
    Ok(xml.value)
}

fn internal_info(
    xml: &mut Xml<'_, '_>,
    names: &Names,
    owner: Node<'_, '_>,
    prepared: &PreparedCreate,
) -> Result<(), CreationError> {
    let owner_name = owner
        .children()
        .find(|node| node.has_tag_name((MD, "Properties")))
        .and_then(|properties| {
            properties
                .children()
                .find(|node| node.has_tag_name((MD, "Name")))
        })
        .and_then(|node| node.text())
        .ok_or(EditError::InvalidXml)?;
    let table = prepared
        .identity
        .table
        .as_ref()
        .ok_or(EditError::InvalidValue)?;
    let row = prepared
        .identity
        .row
        .as_ref()
        .ok_or(EditError::InvalidValue)?;
    let internal = format!("{}InternalInfo", names.md);
    xml.open(&internal, "");
    // External roots use the ordinary DataProcessor/Report aliases for table types.
    let owner_kind =
        MetadataKind::from_xml_tag(owner.tag_name().name()).map_err(|_| EditError::InvalidXml)?;
    for (category, identity) in [("TabularSection", table), ("TabularSectionRow", row)] {
        let name = format!(
            "{}{category}.{owner_name}.{}",
            owner_kind.designer_tag(),
            prepared.request.name
        );
        generated_type(xml, names, &name, category, identity)?;
    }
    xml.close(&internal);
    Ok(())
}

fn generated_type(
    xml: &mut Xml<'_, '_>,
    names: &Names,
    name: &str,
    category: &str,
    identity: &GeneratedIdentity,
) -> Result<(), EditError> {
    let tag = format!("{}GeneratedType", names.xr);
    xml.open(
        &tag,
        &format!(" name=\"{}\" category=\"{category}\"", escape_xml(name)?),
    );
    xml.scalar(&format!("{}TypeId", names.xr), &identity.type_id)?;
    xml.scalar(&format!("{}ValueId", names.xr), &identity.value_id)?;
    xml.close(&tag);
    Ok(())
}

fn synonyms(
    xml: &mut Xml<'_, '_>,
    names: &Names,
    prepared: &PreparedCreate,
) -> Result<(), EditError> {
    let tag = format!("{}Synonym", names.md);
    if prepared.request.synonyms.is_empty() {
        return xml.scalar(&tag, "");
    }
    xml.open(&tag, "");
    for synonym in &prepared.request.synonyms {
        let item = format!("{}item", names.core);
        xml.open(&item, "");
        xml.scalar(&format!("{}lang", names.core), &synonym.language)?;
        xml.scalar(&format!("{}content", names.core), &synonym.content)?;
        xml.close(&item);
    }
    xml.close(&tag);
    Ok(())
}

fn value_types(
    xml: &mut Xml<'_, '_>,
    names: &Names,
    prepared: &PreparedCreate,
    keys: &[PropertyKey],
) -> Result<(), EditError> {
    let tag = format!("{}Type", names.md);
    xml.open(&tag, "");
    for key in keys {
        let prefix = match key.namespace.as_deref() {
            Some(XS) => &names.xs,
            Some(CFG) => &names.cfg,
            _ => return Err(EditError::UnsupportedValue),
        };
        xml.scalar(
            &format!("{}Type", names.core),
            &format!("{prefix}{}", key.name),
        )?;
    }
    // Native TypeDescription places all members before qualifiers, in number/string/date order.
    for order in 0..3 {
        for value in &prepared.request.types {
            match (order, value) {
                (
                    0,
                    CreateType::Number {
                        digits,
                        fraction_digits,
                        non_negative,
                    },
                ) => {
                    qualifier(
                        xml,
                        names,
                        "NumberQualifiers",
                        &[
                            ("Digits", digits.to_string()),
                            ("FractionDigits", fraction_digits.to_string()),
                            (
                                "AllowedSign",
                                if *non_negative { "Nonnegative" } else { "Any" }.to_owned(),
                            ),
                        ],
                    )?;
                }
                (1, CreateType::String { length, fixed }) => {
                    qualifier(
                        xml,
                        names,
                        "StringQualifiers",
                        &[
                            ("Length", length.to_string()),
                            (
                                "AllowedLength",
                                if *fixed { "Fixed" } else { "Variable" }.to_owned(),
                            ),
                        ],
                    )?;
                }
                (2, CreateType::Date { fractions }) => {
                    let value = match fractions {
                        DateFractions::Date => "Date",
                        DateFractions::Time => "Time",
                        DateFractions::DateTime => "DateTime",
                    };
                    qualifier(
                        xml,
                        names,
                        "DateQualifiers",
                        &[("DateFractions", value.to_owned())],
                    )?;
                }
                _ => (),
            }
        }
    }
    xml.close(&tag);
    Ok(())
}

fn qualifier(
    xml: &mut Xml<'_, '_>,
    names: &Names,
    name: &str,
    fields: &[(&str, String)],
) -> Result<(), EditError> {
    let tag = format!("{}{name}", names.core);
    xml.open(&tag, "");
    for (name, value) in fields {
        xml.scalar(&format!("{}{name}", names.core), value)?;
    }
    xml.close(&tag);
    Ok(())
}
