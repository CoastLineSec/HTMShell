use crate::RuntimeError;
use crate::component::{ComponentDefinitionId, ComponentDefinitionKey};
use crate::package::{PackageId, PackageSnapshotGeneration};
use crate::style_owner::{
    OwnedAuthorStyles, OwnedStylesheetSource, OwnedStylesheetSourceId, StyleActivationMode,
    StylesheetOwnerAssociation, StylesheetOwnerId,
};
use crate::stylesheet::PreparedAuthorStylesheet;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

pub const MAX_COMPONENT_STYLESHEETS: usize = 16;
pub const MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE: usize = 64;
pub const MAX_COMPONENT_STYLESHEET_PATH_BYTES: usize = 512;
pub const MAX_COMPONENT_STYLESHEET_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentStylesheetPath(String);

impl ComponentStylesheetPath {
    pub(crate) fn new(path: String) -> Self {
        Self(path)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentStylesheetPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ComponentStylesheetSourceKey {
    package_id: PackageId,
    path: ComponentStylesheetPath,
}

impl ComponentStylesheetSourceKey {
    pub(crate) fn new(package_id: PackageId, path: ComponentStylesheetPath) -> Self {
        Self { package_id, path }
    }

    pub(crate) fn package_id(&self) -> &PackageId {
        &self.package_id
    }

    pub(crate) fn path(&self) -> &ComponentStylesheetPath {
        &self.path
    }

    fn deterministic_string(&self, generation: PackageSnapshotGeneration) -> String {
        format!(
            "component-stylesheet-source:{}:{}@{}",
            self.package_id,
            self.path,
            generation.get()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentStylesheetSemanticVersion(Arc<str>);

impl ComponentStylesheetSemanticVersion {
    pub(crate) fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }

    pub fn deterministic_string(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct ComponentStylesheetSource {
    key: ComponentStylesheetSourceKey,
    semantic_version: ComponentStylesheetSemanticVersion,
    parsed: PreparedAuthorStylesheet,
    bytes: u64,
}

impl ComponentStylesheetSource {
    pub(crate) fn new(
        key: ComponentStylesheetSourceKey,
        semantic_version: ComponentStylesheetSemanticVersion,
        parsed: PreparedAuthorStylesheet,
        bytes: u64,
    ) -> Self {
        Self {
            key,
            semantic_version,
            parsed,
            bytes,
        }
    }

    pub fn package_id(&self) -> &PackageId {
        self.key.package_id()
    }

    pub fn path(&self) -> &ComponentStylesheetPath {
        self.key.path()
    }

    pub fn semantic_version(&self) -> &ComponentStylesheetSemanticVersion {
        &self.semantic_version
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn parsed_rule_count(&self) -> usize {
        self.parsed.rule_count()
    }

    pub fn selector_count(&self) -> usize {
        self.parsed.selector_count()
    }

    pub(crate) fn deterministic_id(&self, generation: PackageSnapshotGeneration) -> String {
        self.key.deterministic_string(generation)
    }
}

#[derive(Debug, Clone)]
pub struct ComponentStylesheetAssociation {
    definition: ComponentDefinitionKey,
    source: Arc<ComponentStylesheetSource>,
    ordinal: u16,
}

impl ComponentStylesheetAssociation {
    pub(crate) fn new(
        definition: ComponentDefinitionKey,
        source: Arc<ComponentStylesheetSource>,
        ordinal: u16,
    ) -> Self {
        Self {
            definition,
            source,
            ordinal,
        }
    }

    pub fn definition(&self) -> &ComponentDefinitionKey {
        &self.definition
    }

    pub fn source(&self) -> &ComponentStylesheetSource {
        &self.source
    }

    pub fn ordinal(&self) -> u16 {
        self.ordinal
    }

    pub fn deterministic_id(&self, generation: PackageSnapshotGeneration) -> String {
        format!(
            "component-stylesheet-association:{}:{}:{}@{}",
            self.definition,
            self.source.path(),
            self.ordinal,
            generation.get()
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentStyleValidationTotals {
    pub source_count: usize,
    pub source_read_count: usize,
    pub source_parse_count: usize,
    pub association_count: usize,
    pub bytes_read: u64,
}

#[derive(Debug, Default)]
pub struct ComponentStyleCatalog {
    sources: Arc<[Arc<ComponentStylesheetSource>]>,
    associations: Arc<[ComponentStylesheetAssociation]>,
    by_definition: BTreeMap<ComponentDefinitionKey, Arc<[ComponentStylesheetAssociation]>>,
    totals: ComponentStyleValidationTotals,
}

impl ComponentStyleCatalog {
    pub(crate) fn new(
        sources: Vec<Arc<ComponentStylesheetSource>>,
        associations: Vec<ComponentStylesheetAssociation>,
        totals: ComponentStyleValidationTotals,
    ) -> Self {
        let mut grouped =
            BTreeMap::<ComponentDefinitionKey, Vec<ComponentStylesheetAssociation>>::new();
        for association in &associations {
            grouped
                .entry(association.definition.clone())
                .or_default()
                .push(association.clone());
        }
        let by_definition = grouped
            .into_iter()
            .map(|(definition, mut entries)| {
                entries.sort_by_key(ComponentStylesheetAssociation::ordinal);
                (definition, Arc::from(entries))
            })
            .collect();
        Self {
            sources: sources.into(),
            associations: associations.into(),
            by_definition,
            totals,
        }
    }

    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub fn sources(&self) -> &[Arc<ComponentStylesheetSource>] {
        &self.sources
    }

    pub fn associations(&self) -> &[ComponentStylesheetAssociation] {
        &self.associations
    }

    pub fn associations_for(
        &self,
        definition: &ComponentDefinitionKey,
    ) -> &[ComponentStylesheetAssociation] {
        self.by_definition
            .get(definition)
            .map(AsRef::as_ref)
            .unwrap_or_default()
    }

    pub fn totals(&self) -> &ComponentStyleValidationTotals {
        &self.totals
    }

    pub(crate) fn has_reachable_styles(&self, reachable: &[ComponentDefinitionKey]) -> bool {
        reachable
            .iter()
            .any(|definition| self.by_definition.contains_key(definition))
    }

    pub(crate) fn activation_mode(
        &self,
        reachable: &[ComponentDefinitionKey],
        generation: PackageSnapshotGeneration,
        ownership_aware: bool,
    ) -> Result<StyleActivationMode, RuntimeError> {
        let has_reachable_styles = self.has_reachable_styles(reachable);
        if ownership_aware != has_reachable_styles {
            return Err(RuntimeError::InvalidPackage(
                "prepared stylesheet matching mode disagrees with reachable component styles"
                    .into(),
            ));
        }
        if !ownership_aware {
            return Ok(StyleActivationMode::LegacyDocumentGlobal);
        }

        let mut owned = Vec::new();
        for definition in reachable {
            let definition_id = ComponentDefinitionId {
                generation,
                key: definition.clone(),
            };
            for association in self.associations_for(definition) {
                let source_id =
                    OwnedStylesheetSourceId::new(association.source.deterministic_id(generation));
                let source = Arc::new(OwnedStylesheetSource::from_parsed(
                    source_id,
                    association.source.path().as_str(),
                    association.source.parsed.stylesheet().clone(),
                ));
                owned.push(StylesheetOwnerAssociation::new(
                    StylesheetOwnerId::ComponentDefinition(definition_id.clone()),
                    association.ordinal,
                    source,
                ));
            }
        }
        Ok(StyleActivationMode::OwnershipAware(OwnedAuthorStyles::new(
            owned,
        )?))
    }
}
