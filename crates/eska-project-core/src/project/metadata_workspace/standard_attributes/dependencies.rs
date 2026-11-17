//! Dynamic accounting fields follow explicit metadata references, never a filesystem scan.

use super::{
    AttributeContext, FieldType, FieldValueKind, MetadataKind, ObjectId, ParsedObject,
    ProjectSession, PropertiesMode, PropertyKey, TreeError, WorkspaceError, standard,
    type_property,
};
use crate::project::metadata_parser::ParsedDescriptor;
use std::{collections::BTreeSet, sync::Arc};

impl ProjectSession {
    pub(in crate::project::metadata_workspace) fn invalid_standard_attribute_owners(
        &self,
    ) -> BTreeSet<ObjectId> {
        self.standard_attribute_dependencies
            .iter()
            .filter(|(_, dependencies)| {
                dependencies
                    .iter()
                    .any(|dependency| !self.objects.contains_key(dependency))
            })
            .map(|(owner, _)| owner.clone())
            .collect()
    }

    pub(super) fn standard_attribute_context(
        &mut self,
        owner: &ParsedObject,
    ) -> Result<AttributeContext, WorkspaceError> {
        let mut context = AttributeContext {
            names: standard::parsed_fields(owner, &self.standard_attribute_compatibility)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ..AttributeContext::default()
        };
        if owner.metadata.kind() != MetadataKind::AccountingRegister {
            return Ok(context);
        }
        let mut dependencies = BTreeSet::new();
        let result = self
            .accounting_attributes(owner, &mut context, &mut dependencies)
            .and_then(|()| {
                // An earlier file can change while a later dependency is read.
                self.verify_source(owner.metadata.id())?;
                for dependency in &dependencies {
                    self.verify_source(dependency)?;
                }
                Ok(())
            });
        // Retain dependencies even after an incomplete branch, so fixing its target enables retry.
        self.standard_attribute_dependencies
            .insert(owner.metadata.id().clone(), dependencies);
        result?;
        Ok(context)
    }

    fn accounting_attributes(
        &mut self,
        owner: &ParsedObject,
        context: &mut AttributeContext,
        dependencies: &mut BTreeSet<ObjectId>,
    ) -> Result<(), WorkspaceError> {
        let Some(chart) = self.standard_attribute_target(
            owner,
            "ChartOfAccounts",
            MetadataKind::ChartOfAccounts,
            dependencies,
        )?
        else {
            return Ok(());
        };
        let chart = chart
            .objects
            .first()
            .ok_or(WorkspaceError::Tree(TreeError::EmptyDescriptor))?;
        let count = standard::scalar(chart, "MaxExtDimensionCount").unwrap_or("0");
        let count = standard::extra_dimension_count(count).ok_or_else(|| {
            WorkspaceError::Tree(TreeError::InvalidStandardAttributeCount {
                owner: chart.metadata.id().clone(),
                value: count.into(),
            })
        })?;
        if count == 0 {
            return Ok(());
        }
        let Some(types) = self.standard_attribute_target(
            chart,
            "ExtDimensionTypes",
            MetadataKind::ChartOfCharacteristicTypes,
            dependencies,
        )?
        else {
            return Ok(());
        };
        let types = types
            .objects
            .first()
            .ok_or(WorkspaceError::Tree(TreeError::EmptyDescriptor))?;
        context.extra_dimension_type = type_property(&FieldType {
            kind: FieldValueKind::Reference,
            types: vec![PropertyKey {
                namespace: Some("http://v8.1c.ru/8.1/data/enterprise/current-config".into()),
                name: format!("ChartOfCharacteristicTypesRef.{}", types.metadata.name()),
            }],
            ..FieldType::default()
        });
        context.extra_dimension_value = standard::property(types, "Type").cloned();
        for index in 1..=count {
            context.names.extend([
                format!("ExtDimension{index}"),
                format!("ExtDimensionType{index}"),
            ]);
        }
        Ok(())
    }

    fn standard_attribute_target(
        &mut self,
        owner: &ParsedObject,
        property: &str,
        kind: MetadataKind,
        dependencies: &mut BTreeSet<ObjectId>,
    ) -> Result<Option<Arc<ParsedDescriptor>>, WorkspaceError> {
        let Some(name) = standard::scalar(owner, property)
            .and_then(|reference| reference.strip_prefix(kind.designer_tag()))
            .and_then(|suffix| suffix.strip_prefix('.'))
        else {
            return Ok(None);
        };
        let id = ObjectId::from_parts(None, kind.as_str(), name);
        dependencies.insert(id.clone());
        self.object(&id)?;
        let parsed = self.load(&id, PropertiesMode::All)?;
        self.verify_source(&id)?;
        Ok(Some(parsed))
    }
}
