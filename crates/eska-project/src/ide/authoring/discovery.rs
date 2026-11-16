//! Bounded discovery reads declared collections, never a recursive filesystem scan.

use eska_project_core::project::{
    configurator::TreeOptions,
    metadata_model::{CollectionKind, MetadataKind, NodeId, ObjectId},
    metadata_workspace::ProjectSession,
};
use serde_json::{Value, json};

use crate::ide::{dto::Labels, editing, envelope::domain};

pub struct ListOptions<'a> {
    pub parent: Option<&'a ObjectId>,
    pub kind: Option<MetadataKind>,
    pub search: Option<&'a str>,
    pub limit: usize,
    pub after: Option<&'a str>,
}

pub fn list(
    labels: &Labels,
    project: &mut ProjectSession,
    options: &ListOptions<'_>,
) -> Result<Value, Value> {
    if !(1..=500).contains(&options.limit) {
        return Err(domain(
            "object_invalid",
            json!({"field":"limit","reason":"out_of_range"}),
        ));
    }
    let root = if let Some(parent) = options.parent {
        project
            .reveal_declared_object(parent)
            .map_err(|error| editing::failure(error.into()))?;
        NodeId::Object(parent.clone())
    } else {
        project.root().clone()
    };
    let mut pending = vec![root.clone()];
    let mut objects = Vec::new();
    let search = options.search.map(str::to_lowercase);
    while let Some(node) = pending.pop() {
        let children = project
            .children(&node, TreeOptions::default())
            .map_err(|error| editing::failure(error.into()))?;
        for child in children {
            match &child.id {
                NodeId::Collection {
                    kind: CollectionKind::Common,
                    ..
                } => pending.push(child.id.clone()),
                NodeId::Collection {
                    kind: CollectionKind::Metadata(kind),
                    ..
                } if options.kind.is_none_or(|selected| selected == *kind) => {
                    pending.push(child.id.clone());
                }
                NodeId::Object(id)
                    if options
                        .kind
                        .is_none_or(|kind| child.metadata_kind == Some(kind))
                        && options.after.is_none_or(|after| id.as_str() > after)
                        && search
                            .as_ref()
                            .is_none_or(|text| id.as_str().to_lowercase().contains(text)) =>
                {
                    objects.push((id.to_string(), labels.node(child)));
                }
                _ => {}
            }
        }
    }
    objects.sort_by(|left, right| left.0.cmp(&right.0));
    let more = objects.len() > options.limit;
    objects.truncate(options.limit);
    let next = if more {
        objects.last().map(|item| item.0.clone())
    } else {
        None
    };
    Ok(
        json!({"schemaVersion":1,"parent":crate::ide::dto::node_id(&root),
        "objects":objects.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),"nextAfter":next}),
    )
}
