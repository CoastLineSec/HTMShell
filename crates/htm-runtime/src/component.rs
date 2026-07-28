use crate::package::{
    PackageAlias, PackageErrorKind, PackageId, PackageLoadError, PackageSchemaSource,
    PackageSnapshotGeneration, ResolvedPackage,
};
use blitz_dom::node::NodeData;
use blitz_dom::{Attribute, DocumentConfig, QualName};
use blitz_html::HtmlDocument;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

pub const MAX_COMPONENT_NAME_BYTES: usize = 64;
pub const MAX_COMPONENT_EXPORTS_PER_PACKAGE: usize = 256;
pub const MAX_COMPONENT_EXPORTS_PER_GRAPH: usize = 4_096;
pub const MAX_COMPONENT_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_COMPONENT_SOURCE_NODES: usize = 10_000;
pub const MAX_COMPONENT_INSTANCES_PER_DOCUMENT: usize = 4_096;
pub const MAX_COMPONENT_REFERENCES_PER_DOCUMENT: usize = 256;
pub const MAX_COMPONENT_NESTING_DEPTH: usize = 32;
pub const MAX_COMPONENT_EXPANDED_NODES: usize = 50_000;

const COMPONENT_ATTRIBUTE: &str = "data-htm-component";
const BUILTIN_ATTRIBUTE: &str = "data-htm-element";
const USE_ELEMENT: &str = "htm-use";
const TEMPLATE_ELEMENT: &str = "template";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ComponentName(String);

impl ComponentName {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        if value.len() < 3 || value.len() > MAX_COMPONENT_NAME_BYTES || !value.is_ascii() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentName,
                format!("component name must contain 3..={MAX_COMPONENT_NAME_BYTES} ASCII bytes"),
            ));
        }
        let bytes = value.as_bytes();
        if !bytes[0].is_ascii_lowercase()
            || !bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            || !value.contains('-')
            || value.contains("--")
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentName,
                format!(
                    "component name `{value}` must be lowercase, hyphenated, and contain only lowercase letters, digits, and single interior hyphens"
                ),
            ));
        }
        if value == USE_ELEMENT
            || value.starts_with("htm-")
            || value.starts_with("xml-")
            || value.starts_with("xlink-")
            || crate::built_in_registry_names().contains(&value)
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::ReservedComponentName,
                format!("component name `{value}` is reserved"),
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentReference {
    alias: Option<PackageAlias>,
    name: ComponentName,
}

impl ComponentReference {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        if value.is_empty() || value.trim() != value || !value.is_ascii() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentReference,
                "component reference must be nonempty ASCII without surrounding whitespace",
            ));
        }
        let mut parts = value.split('.');
        let first = parts.next().expect("nonempty split has one part");
        let second = parts.next();
        if parts.next().is_some() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentReference,
                "component reference contains more than one dot",
            ));
        }
        match second {
            Some(name) => Ok(Self {
                alias: Some(PackageAlias::parse(first).map_err(|_| {
                    PackageLoadError::new(
                        PackageErrorKind::InvalidComponentReference,
                        format!("invalid component dependency alias `{first}`"),
                    )
                })?),
                name: ComponentName::parse(name)?,
            }),
            None => Ok(Self {
                alias: None,
                name: ComponentName::parse(first)?,
            }),
        }
    }

    pub fn alias(&self) -> Option<&PackageAlias> {
        self.alias.as_ref()
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn deterministic_string(&self) -> String {
        match &self.alias {
            Some(alias) => format!("{alias}.{}", self.name),
            None => self.name.to_string(),
        }
    }
}

impl fmt::Display for ComponentReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.deterministic_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentExport {
    name: ComponentName,
    source: String,
}

impl ComponentExport {
    pub(crate) fn new(name: ComponentName, source: String) -> Self {
        Self { name, source }
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentDefinitionKey {
    package_id: PackageId,
    name: ComponentName,
}

impl ComponentDefinitionKey {
    pub(crate) fn new(package_id: PackageId, name: ComponentName) -> Self {
        Self { package_id, name }
    }

    pub fn package_id(&self) -> &PackageId {
        &self.package_id
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn deterministic_string(&self) -> String {
        format!("{}:{}", self.package_id, self.name)
    }
}

impl fmt::Display for ComponentDefinitionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.package_id, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentDefinitionId {
    pub(crate) generation: PackageSnapshotGeneration,
    pub(crate) key: ComponentDefinitionKey,
}

impl ComponentDefinitionId {
    pub fn generation(&self) -> PackageSnapshotGeneration {
        self.generation
    }

    pub fn package_id(&self) -> &PackageId {
        self.key.package_id()
    }

    pub fn name(&self) -> &ComponentName {
        self.key.name()
    }

    pub fn key(&self) -> &ComponentDefinitionKey {
        &self.key
    }

    pub fn deterministic_string(&self) -> String {
        format!("{}@{}", self.key, self.generation.get())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComponentTemplateNode {
    Element {
        name: QualName,
        attributes: Vec<Attribute>,
        children: Arc<[ComponentTemplateNode]>,
        source_ordinal: u32,
    },
    Text {
        value: String,
        source_ordinal: u32,
    },
    Comment {
        source_ordinal: u32,
    },
    Host {
        reference: ComponentReference,
        target: ComponentDefinitionKey,
        source_ordinal: u32,
    },
}

#[derive(Debug, Clone)]
pub struct ComponentDefinition {
    key: ComponentDefinitionKey,
    logical_source: String,
    nodes: Arc<[ComponentTemplateNode]>,
    source_node_count: usize,
    dependencies: Arc<[ComponentDefinitionKey]>,
    resolved_references: Arc<[(ComponentReference, ComponentDefinitionKey)]>,
}

impl ComponentDefinition {
    pub fn key(&self) -> &ComponentDefinitionKey {
        &self.key
    }

    pub fn logical_source(&self) -> &str {
        &self.logical_source
    }

    pub fn source_node_count(&self) -> usize {
        self.source_node_count
    }

    pub fn dependencies(&self) -> &[ComponentDefinitionKey] {
        &self.dependencies
    }

    pub fn resolved_references(&self) -> &[(ComponentReference, ComponentDefinitionKey)] {
        &self.resolved_references
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentValidationTotals {
    pub export_count: usize,
    pub source_document_count: usize,
    pub source_read_count: usize,
    pub source_parse_count: usize,
    pub source_node_count: usize,
}

#[derive(Debug, Clone)]
pub struct ComponentCatalog {
    definitions: Arc<[Arc<ComponentDefinition>]>,
    definition_indices: BTreeMap<ComponentDefinitionKey, usize>,
    dependency_first_order: Arc<[ComponentDefinitionKey]>,
    totals: ComponentValidationTotals,
}

impl ComponentCatalog {
    pub(crate) fn empty() -> Self {
        Self {
            definitions: Arc::from([]),
            definition_indices: BTreeMap::new(),
            dependency_first_order: Arc::from([]),
            totals: ComponentValidationTotals::default(),
        }
    }

    pub fn definitions(&self) -> &[Arc<ComponentDefinition>] {
        &self.definitions
    }

    pub fn dependency_first_order(&self) -> &[ComponentDefinitionKey] {
        &self.dependency_first_order
    }

    pub fn totals(&self) -> &ComponentValidationTotals {
        &self.totals
    }

    pub fn definition(&self, key: &ComponentDefinitionKey) -> Option<&ComponentDefinition> {
        self.definition_indices
            .get(key)
            .map(|index| self.definitions[*index].as_ref())
    }
}

#[derive(Debug)]
pub(crate) struct UnresolvedComponentDefinition {
    pub key: ComponentDefinitionKey,
    pub logical_source: String,
    pub nodes: Vec<UnresolvedTemplateNode>,
    pub source_node_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnresolvedTemplateNode {
    Element {
        name: QualName,
        attributes: Vec<Attribute>,
        children: Vec<UnresolvedTemplateNode>,
        source_ordinal: u32,
    },
    Text {
        value: String,
        source_ordinal: u32,
    },
    Comment {
        source_ordinal: u32,
    },
    Use {
        reference: ComponentReference,
        source_ordinal: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDocument {
    logical_path: String,
    nodes: Arc<[ComponentTemplateNode]>,
    stats: PreparedDocumentStats,
    logical_instance_paths: Arc<[String]>,
}

impl PreparedDocument {
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    pub fn stats(&self) -> PreparedDocumentStats {
        self.stats
    }

    pub fn logical_instance_paths(&self) -> &[String] {
        &self.logical_instance_paths
    }

    pub(crate) fn instantiate(
        &self,
        catalog: &ComponentCatalog,
        generation: PackageSnapshotGeneration,
        document_serial: u64,
        config: DocumentConfig,
    ) -> Result<InstantiatedDocument, PackageLoadError> {
        let mut document = HtmlDocument::from_html("", config);
        document.mutate().remove_and_drop_all_children(0);
        let mut state = InstantiationState {
            catalog,
            generation,
            document_serial,
            instances: Vec::with_capacity(self.stats.component_instances),
            descendants: Vec::new(),
        };
        let children = instantiate_nodes(&mut document, &self.nodes, None, &[], &mut state)?;
        document.mutate().append_children(0, &children);
        Ok(InstantiatedDocument {
            document,
            instances: state.instances,
            descendants: state.descendants,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreparedDocumentStats {
    pub component_instances: usize,
    pub referenced_definitions: usize,
    pub expanded_nodes: usize,
    pub maximum_nesting_depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentInstanceId {
    snapshot_generation: PackageSnapshotGeneration,
    definition: ComponentDefinitionKey,
    document_serial: u64,
    invocation_path: Arc<[u32]>,
}

impl ComponentInstanceId {
    pub fn snapshot_generation(&self) -> PackageSnapshotGeneration {
        self.snapshot_generation
    }

    pub fn definition(&self) -> &ComponentDefinitionKey {
        &self.definition
    }

    pub fn document_serial(&self) -> u64 {
        self.document_serial
    }

    pub fn invocation_path(&self) -> &[u32] {
        &self.invocation_path
    }

    pub fn deterministic_string(&self) -> String {
        let path = self
            .invocation_path
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        format!(
            "{}@{}#{}:{path}",
            self.definition,
            self.snapshot_generation.get(),
            self.document_serial
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInstanceRecord {
    id: ComponentInstanceId,
    definition_id: ComponentDefinitionId,
    reference: ComponentReference,
    logical_path: String,
    top_level_slots: Arc<[usize]>,
}

impl ComponentInstanceRecord {
    pub fn id(&self) -> &ComponentInstanceId {
        &self.id
    }

    pub fn definition_id(&self) -> &ComponentDefinitionId {
        &self.definition_id
    }

    pub fn reference(&self) -> &ComponentReference {
        &self.reference
    }

    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    pub fn top_level_slots(&self) -> &[usize] {
        &self.top_level_slots
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDescendantProvenance {
    instance_id: ComponentInstanceId,
    template_source_ordinal: u32,
    dom_slot: usize,
    dom_slot_generation: u64,
}

impl ComponentDescendantProvenance {
    pub fn instance_id(&self) -> &ComponentInstanceId {
        &self.instance_id
    }

    pub fn template_source_ordinal(&self) -> u32 {
        self.template_source_ordinal
    }

    pub fn dom_slot(&self) -> usize {
        self.dom_slot
    }

    pub fn dom_slot_generation(&self) -> u64 {
        self.dom_slot_generation
    }
}

pub(crate) struct InstantiatedDocument {
    pub document: HtmlDocument,
    pub instances: Vec<ComponentInstanceRecord>,
    pub descendants: Vec<ComponentDescendantProvenance>,
}

struct InstantiationState<'a> {
    catalog: &'a ComponentCatalog,
    generation: PackageSnapshotGeneration,
    document_serial: u64,
    instances: Vec<ComponentInstanceRecord>,
    descendants: Vec<ComponentDescendantProvenance>,
}

pub(crate) fn parse_component_source(
    html: &str,
    owner: &PackageId,
    logical_source: &str,
    expected: &BTreeSet<ComponentName>,
) -> Result<BTreeMap<ComponentName, UnresolvedComponentDefinition>, PackageLoadError> {
    reject_duplicate_control_attributes(html, logical_source)?;
    let document = HtmlDocument::from_html(html, parser_config());
    let body = find_html_element(&document, "body").ok_or_else(|| {
        component_error(
            PackageErrorKind::ComponentSourceParse,
            owner,
            logical_source,
            "component source did not produce an HTML body",
        )
    })?;
    let head = find_html_element(&document, "head").ok_or_else(|| {
        component_error(
            PackageErrorKind::ComponentSourceParse,
            owner,
            logical_source,
            "component source did not produce an HTML head",
        )
    })?;
    let head_children = document
        .get_node(head)
        .expect("located head node remains live")
        .children
        .clone();
    let body_children = document
        .get_node(body)
        .expect("located body node remains live")
        .children
        .clone();
    let mut definitions = BTreeMap::new();
    for (child, in_head) in head_children
        .into_iter()
        .map(|child| (child, true))
        .chain(body_children.into_iter().map(|child| (child, false)))
    {
        let node = document
            .get_node(child)
            .expect("body child remains live during source validation");
        match &node.data {
            NodeData::Comment => {}
            NodeData::Text(text) if text.content.chars().all(char::is_whitespace) => {}
            NodeData::Element(element)
                if in_head && matches!(element.name.local.as_ref(), "meta" | "title") => {}
            NodeData::Element(element) if element.name.local.as_ref() == TEMPLATE_ELEMENT => {
                if element.attrs().len() != 1 {
                    return Err(component_error(
                        PackageErrorKind::InvalidComponentExport,
                        owner,
                        logical_source,
                        "component template accepts only `data-htm-component`",
                    ));
                }
                let Some(value) = element_attr(element, COMPONENT_ATTRIBUTE) else {
                    return Err(component_error(
                        PackageErrorKind::InvalidComponentExport,
                        owner,
                        logical_source,
                        "top-level template is missing `data-htm-component`",
                    ));
                };
                let name = ComponentName::parse(value)
                    .map_err(|error| error.in_package(owner.to_string()).at(logical_source))?;
                if !expected.contains(&name) {
                    return Err(component_error(
                        PackageErrorKind::ComponentTemplateUnexported,
                        owner,
                        logical_source,
                        format!("template `{name}` is absent from the manifest export table"),
                    ));
                }
                if definitions.contains_key(&name) {
                    return Err(component_error(
                        PackageErrorKind::ComponentTemplateDuplicate,
                        owner,
                        logical_source,
                        format!("component template `{name}` is declared more than once"),
                    ));
                }
                let mut ordinal = 0u32;
                let mut source_node_count = 0usize;
                let children = node
                    .children
                    .iter()
                    .map(|child| {
                        normalize_component_node(
                            &document,
                            *child,
                            owner,
                            logical_source,
                            &name,
                            &mut ordinal,
                            &mut source_node_count,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if source_node_count > MAX_COMPONENT_SOURCE_NODES {
                    return Err(component_error(
                        PackageErrorKind::ComponentSourceNodeLimit,
                        owner,
                        logical_source,
                        format!(
                            "component `{name}` contains {source_node_count} source nodes; limit is {MAX_COMPONENT_SOURCE_NODES}"
                        ),
                    ));
                }
                definitions.insert(
                    name.clone(),
                    UnresolvedComponentDefinition {
                        key: ComponentDefinitionKey::new(owner.clone(), name),
                        logical_source: logical_source.to_owned(),
                        nodes: children,
                        source_node_count,
                    },
                );
            }
            _ => {
                return Err(component_error(
                    PackageErrorKind::ComponentSourceRenderedContent,
                    owner,
                    logical_source,
                    "component source contains renderable content outside declaration templates",
                ));
            }
        }
    }
    for name in expected {
        if !definitions.contains_key(name) {
            return Err(component_error(
                PackageErrorKind::ComponentTemplateMissing,
                owner,
                logical_source,
                format!("manifest export `{name}` has no matching template"),
            ));
        }
    }
    Ok(definitions)
}

pub(crate) fn build_component_catalog(
    packages: &[Arc<ResolvedPackage>],
    unresolved: Vec<UnresolvedComponentDefinition>,
    totals: ComponentValidationTotals,
) -> Result<ComponentCatalog, PackageLoadError> {
    if unresolved.len() > MAX_COMPONENT_EXPORTS_PER_GRAPH {
        return Err(PackageLoadError::new(
            PackageErrorKind::ComponentGraphExportLimit,
            format!(
                "component graph contains {} exports; limit is {MAX_COMPONENT_EXPORTS_PER_GRAPH}",
                unresolved.len()
            ),
        ));
    }
    let package_by_id: BTreeMap<_, _> = packages
        .iter()
        .map(|package| (package.id().clone(), Arc::clone(package)))
        .collect();
    let available: BTreeSet<_> = unresolved
        .iter()
        .map(|definition| definition.key.clone())
        .collect();
    let mut definitions = Vec::with_capacity(unresolved.len());
    let mut indices = BTreeMap::new();
    for unresolved in unresolved {
        let mut dependencies = Vec::new();
        let mut dependency_set = BTreeSet::new();
        let mut references = Vec::new();
        let nodes = resolve_nodes(
            unresolved.nodes,
            &unresolved.key.package_id,
            &package_by_id,
            &available,
            &mut dependencies,
            &mut dependency_set,
            &mut references,
        )?;
        let index = definitions.len();
        indices.insert(unresolved.key.clone(), index);
        definitions.push(Arc::new(ComponentDefinition {
            key: unresolved.key,
            logical_source: unresolved.logical_source,
            nodes: nodes.into(),
            source_node_count: unresolved.source_node_count,
            dependencies: dependencies.into(),
            resolved_references: references.into(),
        }));
    }
    let order = component_dependency_order(&definitions, &indices)?;
    Ok(ComponentCatalog {
        definitions: definitions.into(),
        definition_indices: indices,
        dependency_first_order: order.into(),
        totals,
    })
}

pub(crate) fn prepare_root_document(
    html: &str,
    logical_path: &str,
    owner: &ResolvedPackage,
    catalog: &ComponentCatalog,
) -> Result<PreparedDocument, PackageLoadError> {
    reject_duplicate_control_attributes(html, logical_path)?;
    let document = HtmlDocument::from_html(html, parser_config());
    let mut ordinal = 0u32;
    let mut inside_template = false;
    let nodes = document
        .get_node(0)
        .expect("Blitz document root exists")
        .children
        .iter()
        .map(|child| {
            normalize_root_node(
                &document,
                *child,
                logical_path,
                owner,
                catalog,
                &mut ordinal,
                &mut inside_template,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (stats, logical_instance_paths) =
        validate_prepared_expansion(&nodes, catalog, logical_path)?;
    Ok(PreparedDocument {
        logical_path: logical_path.to_owned(),
        nodes: nodes.into(),
        stats,
        logical_instance_paths: logical_instance_paths.into(),
    })
}

fn normalize_component_node(
    document: &HtmlDocument,
    slot: usize,
    owner: &PackageId,
    logical_source: &str,
    definition_name: &ComponentName,
    ordinal: &mut u32,
    count: &mut usize,
) -> Result<UnresolvedTemplateNode, PackageLoadError> {
    *count = count.checked_add(1).ok_or_else(|| {
        component_error(
            PackageErrorKind::ComponentSourceNodeLimit,
            owner,
            logical_source,
            "component source node count overflowed",
        )
    })?;
    let source_ordinal = next_ordinal(ordinal, owner, logical_source)?;
    let node = document
        .get_node(slot)
        .expect("component source slot remains live");
    match &node.data {
        NodeData::Text(text) => Ok(UnresolvedTemplateNode::Text {
            value: text.content.clone(),
            source_ordinal,
        }),
        NodeData::Comment => Ok(UnresolvedTemplateNode::Comment { source_ordinal }),
        NodeData::Document | NodeData::AnonymousBlock(_) => Err(component_error(
            PackageErrorKind::ComponentSourceParse,
            owner,
            logical_source,
            format!("component `{definition_name}` contains an unsupported parser node"),
        )),
        NodeData::Element(element) => {
            let tag = element.name.local.as_ref();
            if tag == TEMPLATE_ELEMENT {
                return Err(component_error(
                    PackageErrorKind::ComponentFeatureNotSupported,
                    owner,
                    logical_source,
                    format!("component `{definition_name}` contains a nested template"),
                ));
            }
            if tag == USE_ELEMENT {
                validate_use_element(element, &node.children, document, owner, logical_source)?;
                let value = element_attr(element, "component")
                    .expect("validated htm-use has component attribute");
                let reference = ComponentReference::parse(value)
                    .map_err(|error| error.in_package(owner.to_string()).at(logical_source))?;
                return Ok(UnresolvedTemplateNode::Use {
                    reference,
                    source_ordinal,
                });
            }
            validate_static_component_element(element, owner, logical_source)?;
            let children = node
                .children
                .iter()
                .map(|child| {
                    normalize_component_node(
                        document,
                        *child,
                        owner,
                        logical_source,
                        definition_name,
                        ordinal,
                        count,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(UnresolvedTemplateNode::Element {
                name: element.name.clone(),
                attributes: element.attrs().to_vec(),
                children,
                source_ordinal,
            })
        }
    }
}

fn normalize_root_node(
    document: &HtmlDocument,
    slot: usize,
    logical_path: &str,
    owner: &ResolvedPackage,
    catalog: &ComponentCatalog,
    ordinal: &mut u32,
    inside_template: &mut bool,
) -> Result<ComponentTemplateNode, PackageLoadError> {
    let source_ordinal = next_ordinal(ordinal, owner.id(), logical_path)?;
    let node = document
        .get_node(slot)
        .expect("root source slot remains live");
    match &node.data {
        NodeData::Text(text) => Ok(ComponentTemplateNode::Text {
            value: text.content.clone(),
            source_ordinal,
        }),
        NodeData::Comment => Ok(ComponentTemplateNode::Comment { source_ordinal }),
        NodeData::Document | NodeData::AnonymousBlock(_) => Err(component_error(
            PackageErrorKind::EntryDocument,
            owner.id(),
            logical_path,
            "root document contains an unsupported parser node",
        )),
        NodeData::Element(element) => {
            let tag = element.name.local.as_ref();
            if element_attr(element, COMPONENT_ATTRIBUTE).is_some() {
                return Err(component_error(
                    PackageErrorKind::ComponentFeatureNotSupported,
                    owner.id(),
                    logical_path,
                    "root entry documents cannot declare component templates",
                ));
            }
            if tag == USE_ELEMENT {
                if owner.schema_source() != PackageSchemaSource::SchemaV2 {
                    return Err(component_error(
                        PackageErrorKind::ComponentFeatureNotSupported,
                        owner.id(),
                        logical_path,
                        "`htm-use` requires a schema-v2 shell package",
                    ));
                }
                if *inside_template {
                    return Err(component_error(
                        PackageErrorKind::ComponentRepeatNotSupported,
                        owner.id(),
                        logical_path,
                        "`htm-use` is not supported inside root document templates or repeats",
                    ));
                }
                validate_use_element(element, &node.children, document, owner.id(), logical_path)?;
                let reference = ComponentReference::parse(
                    element_attr(element, "component")
                        .expect("validated htm-use has component attribute"),
                )
                .map_err(|error| error.in_package(owner.id().to_string()).at(logical_path))?;
                let target = resolve_reference_from_package(owner, &reference, catalog)?;
                return Ok(ComponentTemplateNode::Host {
                    reference,
                    target,
                    source_ordinal,
                });
            }
            let previous = *inside_template;
            if tag == TEMPLATE_ELEMENT || element_attr(element, BUILTIN_ATTRIBUTE) == Some("repeat")
            {
                *inside_template = true;
            }
            let children = node
                .children
                .iter()
                .map(|child| {
                    normalize_root_node(
                        document,
                        *child,
                        logical_path,
                        owner,
                        catalog,
                        ordinal,
                        inside_template,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            *inside_template = previous;
            Ok(ComponentTemplateNode::Element {
                name: element.name.clone(),
                attributes: element.attrs().to_vec(),
                children: children.into(),
                source_ordinal,
            })
        }
    }
}

fn validate_static_component_element(
    element: &blitz_dom::ElementData,
    owner: &PackageId,
    logical_source: &str,
) -> Result<(), PackageLoadError> {
    let tag = element.name.local.as_ref();
    if matches!(
        tag,
        "script"
            | "style"
            | "link"
            | "img"
            | "image"
            | "iframe"
            | "object"
            | "embed"
            | "audio"
            | "video"
            | "source"
            | "track"
            | "canvas"
    ) {
        let kind = if matches!(tag, "style" | "link") {
            PackageErrorKind::ComponentResourceNotSupported
        } else if tag == "script" {
            PackageErrorKind::ComponentFeatureNotSupported
        } else {
            PackageErrorKind::ComponentResourceNotSupported
        };
        return Err(component_error(
            kind,
            owner,
            logical_source,
            format!("component element `<{tag}>` is not supported in the static profile"),
        ));
    }
    if tag == "slot" {
        return Err(component_error(
            PackageErrorKind::ComponentFeatureNotSupported,
            owner,
            logical_source,
            "component slots are not supported",
        ));
    }
    for attribute in element.attrs() {
        let name = attribute.name.local.as_ref();
        let value = attribute.value.as_str();
        if name == "id"
            || name == "for"
            || name == "slot"
            || matches!(
                name,
                "aria-labelledby"
                    | "aria-describedby"
                    | "aria-controls"
                    | "aria-owns"
                    | "aria-activedescendant"
                    | "list"
                    | "form"
                    | "headers"
            )
            || (name == "href" && value.starts_with('#'))
        {
            return Err(component_error(
                PackageErrorKind::ComponentFeatureNotSupported,
                owner,
                logical_source,
                format!("component-local reference attribute `{name}` is not supported"),
            ));
        }
        if name.starts_with("data-htm-") {
            let kind = if name == BUILTIN_ATTRIBUTE
                || name.contains("bind")
                || name.contains("action")
                || name.contains("state")
                || name.contains("source")
            {
                PackageErrorKind::ComponentStateActionNotSupported
            } else {
                PackageErrorKind::ComponentFeatureNotSupported
            };
            return Err(component_error(
                kind,
                owner,
                logical_source,
                format!("component runtime attribute `{name}` is not supported"),
            ));
        }
        if matches!(
            name,
            "src"
                | "srcset"
                | "href"
                | "xlink:href"
                | "poster"
                | "data"
                | "background"
                | "action"
                | "formaction"
        ) {
            return Err(component_error(
                PackageErrorKind::ComponentResourceNotSupported,
                owner,
                logical_source,
                format!("component resource attribute `{name}` is not supported"),
            ));
        }
        if name == "style" {
            let lowercase = value.to_ascii_lowercase();
            if lowercase.contains("url") || lowercase.contains("@import") || value.contains('\\') {
                return Err(component_error(
                    PackageErrorKind::ComponentResourceNotSupported,
                    owner,
                    logical_source,
                    "component inline style must not load external resources",
                ));
            }
        }
        if name.starts_with("input.") {
            return Err(component_error(
                PackageErrorKind::ComponentFeatureNotSupported,
                owner,
                logical_source,
                "component inputs are not supported",
            ));
        }
    }
    Ok(())
}

fn validate_use_element(
    element: &blitz_dom::ElementData,
    children: &[usize],
    document: &HtmlDocument,
    owner: &PackageId,
    logical_source: &str,
) -> Result<(), PackageLoadError> {
    if element.attrs().len() != 1 || element_attr(element, "component").is_none() {
        return Err(component_error(
            PackageErrorKind::ComponentInvocationAttributes,
            owner,
            logical_source,
            "`htm-use` requires exactly one `component` attribute",
        ));
    }
    if element_attr(element, "component").is_some_and(str::is_empty) {
        return Err(component_error(
            PackageErrorKind::InvalidComponentReference,
            owner,
            logical_source,
            "`htm-use` component reference must not be empty",
        ));
    }
    for child in children {
        let child = document
            .get_node(*child)
            .expect("htm-use child remains live");
        match &child.data {
            NodeData::Comment => {}
            NodeData::Text(text) if text.content.chars().all(char::is_whitespace) => {}
            _ => {
                return Err(component_error(
                    PackageErrorKind::ComponentInvocationChildren,
                    owner,
                    logical_source,
                    "`htm-use` accepts only whitespace and comments as children",
                ));
            }
        }
    }
    Ok(())
}

fn resolve_nodes(
    nodes: Vec<UnresolvedTemplateNode>,
    owner: &PackageId,
    packages: &BTreeMap<PackageId, Arc<ResolvedPackage>>,
    available: &BTreeSet<ComponentDefinitionKey>,
    dependencies: &mut Vec<ComponentDefinitionKey>,
    dependency_set: &mut BTreeSet<ComponentDefinitionKey>,
    references: &mut Vec<(ComponentReference, ComponentDefinitionKey)>,
) -> Result<Vec<ComponentTemplateNode>, PackageLoadError> {
    nodes
        .into_iter()
        .map(|node| match node {
            UnresolvedTemplateNode::Text {
                value,
                source_ordinal,
            } => Ok(ComponentTemplateNode::Text {
                value,
                source_ordinal,
            }),
            UnresolvedTemplateNode::Comment { source_ordinal } => {
                Ok(ComponentTemplateNode::Comment { source_ordinal })
            }
            UnresolvedTemplateNode::Element {
                name,
                attributes,
                children,
                source_ordinal,
            } => Ok(ComponentTemplateNode::Element {
                name,
                attributes,
                children: resolve_nodes(
                    children,
                    owner,
                    packages,
                    available,
                    dependencies,
                    dependency_set,
                    references,
                )?
                .into(),
                source_ordinal,
            }),
            UnresolvedTemplateNode::Use {
                reference,
                source_ordinal,
            } => {
                let package = packages.get(owner).ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component owner package `{owner}` is absent"),
                    )
                })?;
                let target = resolve_reference_key(package, &reference)?;
                if !available.contains(&target) {
                    return Err(PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component reference `{reference}` resolves to missing `{target}`"),
                    )
                    .in_package(owner.to_string()));
                }
                if dependency_set.insert(target.clone()) {
                    dependencies.push(target.clone());
                }
                references.push((reference.clone(), target.clone()));
                Ok(ComponentTemplateNode::Host {
                    reference,
                    target,
                    source_ordinal,
                })
            }
        })
        .collect()
}

fn resolve_reference_key(
    owner: &ResolvedPackage,
    reference: &ComponentReference,
) -> Result<ComponentDefinitionKey, PackageLoadError> {
    let package_id = match reference.alias() {
        None => owner.id().clone(),
        Some(alias) => owner
            .dependencies()
            .iter()
            .find(|dependency| dependency.alias() == alias)
            .map(|dependency| dependency.target().clone())
            .ok_or_else(|| {
                PackageLoadError::new(
                    PackageErrorKind::ComponentAliasUnknown,
                    format!(
                        "component reference `{reference}` uses unknown direct dependency alias `{alias}`"
                    ),
                )
                .in_package(owner.id().to_string())
            })?,
    };
    Ok(ComponentDefinitionKey::new(
        package_id,
        reference.name().clone(),
    ))
}

fn resolve_reference_from_package(
    owner: &ResolvedPackage,
    reference: &ComponentReference,
    catalog: &ComponentCatalog,
) -> Result<ComponentDefinitionKey, PackageLoadError> {
    let key = resolve_reference_key(owner, reference)?;
    if catalog.definition(&key).is_none() {
        return Err(PackageLoadError::new(
            PackageErrorKind::ComponentExportUnknown,
            format!("component reference `{reference}` resolves to missing `{key}`"),
        )
        .in_package(owner.id().to_string()));
    }
    Ok(key)
}

fn component_dependency_order(
    definitions: &[Arc<ComponentDefinition>],
    indices: &BTreeMap<ComponentDefinitionKey, usize>,
) -> Result<Vec<ComponentDefinitionKey>, PackageLoadError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Visiting,
        Resolved,
    }
    fn visit(
        key: &ComponentDefinitionKey,
        definitions: &[Arc<ComponentDefinition>],
        indices: &BTreeMap<ComponentDefinitionKey, usize>,
        states: &mut BTreeMap<ComponentDefinitionKey, State>,
        stack: &mut Vec<ComponentDefinitionKey>,
        order: &mut Vec<ComponentDefinitionKey>,
        depth: usize,
    ) -> Result<(), PackageLoadError> {
        if depth > MAX_COMPONENT_NESTING_DEPTH {
            return Err(PackageLoadError::new(
                PackageErrorKind::ComponentNestingLimit,
                format!("component dependency depth exceeds {MAX_COMPONENT_NESTING_DEPTH}"),
            ));
        }
        match states.get(key) {
            Some(State::Resolved) => return Ok(()),
            Some(State::Visiting) => {
                let start = stack.iter().position(|item| item == key).unwrap_or(0);
                let mut cycle = stack[start..]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                cycle.push(key.to_string());
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentDependencyCycle,
                    format!("component dependency cycle: {}", cycle.join(" -> ")),
                ));
            }
            None => {}
        }
        states.insert(key.clone(), State::Visiting);
        stack.push(key.clone());
        let index = indices.get(key).ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::ComponentExportUnknown,
                format!("component definition `{key}` is absent"),
            )
        })?;
        for dependency in definitions[*index].dependencies() {
            visit(
                dependency,
                definitions,
                indices,
                states,
                stack,
                order,
                depth + 1,
            )?;
        }
        stack.pop();
        states.insert(key.clone(), State::Resolved);
        order.push(key.clone());
        Ok(())
    }

    let mut states = BTreeMap::new();
    let mut stack = Vec::new();
    let mut order = Vec::with_capacity(definitions.len());
    for definition in definitions {
        visit(
            definition.key(),
            definitions,
            indices,
            &mut states,
            &mut stack,
            &mut order,
            1,
        )?;
    }
    Ok(order)
}

fn validate_prepared_expansion(
    nodes: &[ComponentTemplateNode],
    catalog: &ComponentCatalog,
    logical_path: &str,
) -> Result<(PreparedDocumentStats, Vec<String>), PackageLoadError> {
    struct Expansion<'a> {
        catalog: &'a ComponentCatalog,
        instances: usize,
        referenced: BTreeSet<ComponentDefinitionKey>,
        expanded: usize,
        maximum_depth: usize,
        paths: Vec<String>,
    }
    fn visit(
        nodes: &[ComponentTemplateNode],
        state: &mut Expansion<'_>,
        depth: usize,
        path: &mut Vec<u32>,
    ) -> Result<(), PackageLoadError> {
        for node in nodes {
            state.expanded = state.expanded.checked_add(1).ok_or_else(|| {
                PackageLoadError::new(
                    PackageErrorKind::ComponentExpandedNodeLimit,
                    "expanded component node count overflowed",
                )
            })?;
            if state.expanded > MAX_COMPONENT_EXPANDED_NODES {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentExpandedNodeLimit,
                    format!("expanded document exceeds {MAX_COMPONENT_EXPANDED_NODES} nodes"),
                ));
            }
            match node {
                ComponentTemplateNode::Element { children, .. } => {
                    visit(children, state, depth, path)?;
                }
                ComponentTemplateNode::Host {
                    reference,
                    target,
                    source_ordinal,
                } => {
                    let next_depth = depth.checked_add(1).ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentNestingLimit,
                            "component nesting depth overflowed",
                        )
                    })?;
                    if next_depth > MAX_COMPONENT_NESTING_DEPTH {
                        return Err(PackageLoadError::new(
                            PackageErrorKind::ComponentNestingLimit,
                            format!(
                                "component nesting depth exceeds {MAX_COMPONENT_NESTING_DEPTH}"
                            ),
                        ));
                    }
                    state.maximum_depth = state.maximum_depth.max(next_depth);
                    state.instances = state.instances.checked_add(1).ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentInstanceLimit,
                            "component instance count overflowed",
                        )
                    })?;
                    if state.instances > MAX_COMPONENT_INSTANCES_PER_DOCUMENT {
                        return Err(PackageLoadError::new(
                            PackageErrorKind::ComponentInstanceLimit,
                            format!(
                                "document exceeds {MAX_COMPONENT_INSTANCES_PER_DOCUMENT} component instances"
                            ),
                        ));
                    }
                    state.referenced.insert(target.clone());
                    if state.referenced.len() > MAX_COMPONENT_REFERENCES_PER_DOCUMENT {
                        return Err(PackageLoadError::new(
                            PackageErrorKind::ComponentReferencedDefinitionLimit,
                            format!(
                                "document references more than {MAX_COMPONENT_REFERENCES_PER_DOCUMENT} component definitions"
                            ),
                        ));
                    }
                    path.push(*source_ordinal);
                    state.paths.push(format!(
                        "{}[{}]",
                        reference,
                        path.iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(".")
                    ));
                    let definition = state.catalog.definition(target).ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentExportUnknown,
                            format!("prepared component target `{target}` is absent"),
                        )
                    })?;
                    visit(&definition.nodes, state, next_depth, path)?;
                    path.pop();
                }
                ComponentTemplateNode::Text { .. } | ComponentTemplateNode::Comment { .. } => {}
            }
        }
        Ok(())
    }

    let mut state = Expansion {
        catalog,
        instances: 0,
        referenced: BTreeSet::new(),
        expanded: 0,
        maximum_depth: 0,
        paths: Vec::new(),
    };
    visit(nodes, &mut state, 0, &mut Vec::new()).map_err(|error| error.at(logical_path))?;
    Ok((
        PreparedDocumentStats {
            component_instances: state.instances,
            referenced_definitions: state.referenced.len(),
            expanded_nodes: state.expanded,
            maximum_nesting_depth: state.maximum_depth,
        },
        state.paths,
    ))
}

fn instantiate_nodes(
    document: &mut HtmlDocument,
    nodes: &[ComponentTemplateNode],
    current_instance: Option<&ComponentInstanceId>,
    invocation_path: &[u32],
    state: &mut InstantiationState<'_>,
) -> Result<Vec<usize>, PackageLoadError> {
    let mut created = Vec::new();
    for node in nodes {
        match node {
            ComponentTemplateNode::Text {
                value,
                source_ordinal,
            } => {
                let slot = document.mutate().create_text_node(value);
                record_descendant(current_instance, *source_ordinal, slot, state);
                created.push(slot);
            }
            ComponentTemplateNode::Comment { source_ordinal } => {
                let slot = document.mutate().create_comment_node();
                record_descendant(current_instance, *source_ordinal, slot, state);
                created.push(slot);
            }
            ComponentTemplateNode::Element {
                name,
                attributes,
                children,
                source_ordinal,
            } => {
                let slot = document
                    .mutate()
                    .create_element(name.clone(), attributes.clone());
                record_descendant(current_instance, *source_ordinal, slot, state);
                let child_slots = instantiate_nodes(
                    document,
                    children,
                    current_instance,
                    invocation_path,
                    state,
                )?;
                document.mutate().append_children(slot, &child_slots);
                created.push(slot);
            }
            ComponentTemplateNode::Host {
                reference,
                target,
                source_ordinal,
            } => {
                let mut path = invocation_path.to_vec();
                path.push(*source_ordinal);
                let instance_id = ComponentInstanceId {
                    snapshot_generation: state.generation,
                    definition: target.clone(),
                    document_serial: state.document_serial,
                    invocation_path: path.clone().into(),
                };
                let definition = state.catalog.definition(target).ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component target `{target}` disappeared during instantiation"),
                    )
                })?;
                let record_index = state.instances.len();
                state.instances.push(ComponentInstanceRecord {
                    id: instance_id.clone(),
                    definition_id: ComponentDefinitionId {
                        generation: state.generation,
                        key: target.clone(),
                    },
                    reference: reference.clone(),
                    logical_path: format!(
                        "{}[{}]",
                        reference,
                        path.iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(".")
                    ),
                    top_level_slots: Arc::from([]),
                });
                let child_slots = instantiate_nodes(
                    document,
                    &definition.nodes,
                    Some(&instance_id),
                    &path,
                    state,
                )?;
                state.instances[record_index].top_level_slots = child_slots.clone().into();
                created.extend(child_slots);
            }
        }
    }
    Ok(created)
}

fn record_descendant(
    current_instance: Option<&ComponentInstanceId>,
    source_ordinal: u32,
    slot: usize,
    state: &mut InstantiationState<'_>,
) {
    if let Some(instance_id) = current_instance {
        state.descendants.push(ComponentDescendantProvenance {
            instance_id: instance_id.clone(),
            template_source_ordinal: source_ordinal,
            dom_slot: slot,
            dom_slot_generation: 0,
        });
    }
}

fn find_html_element(document: &HtmlDocument, local: &str) -> Option<usize> {
    let mut stack = vec![0usize];
    while let Some(slot) = stack.pop() {
        let node = document.get_node(slot)?;
        if node
            .element_data()
            .is_some_and(|element| element.name.local.as_ref() == local)
        {
            return Some(slot);
        }
        stack.extend(node.children.iter().rev().copied());
    }
    None
}

fn parser_config() -> DocumentConfig {
    DocumentConfig {
        base_url: Some("htm-local://package/root/index.html".to_owned()),
        ..Default::default()
    }
}

fn element_attr<'a>(element: &'a blitz_dom::ElementData, name: &str) -> Option<&'a str> {
    element
        .attrs()
        .iter()
        .find(|attribute| attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_str())
}

fn next_ordinal(
    ordinal: &mut u32,
    owner: &PackageId,
    logical_source: &str,
) -> Result<u32, PackageLoadError> {
    let current = *ordinal;
    *ordinal = ordinal.checked_add(1).ok_or_else(|| {
        component_error(
            PackageErrorKind::ComponentSourceNodeLimit,
            owner,
            logical_source,
            "component source ordinal overflowed",
        )
    })?;
    Ok(current)
}

fn reject_duplicate_control_attributes(
    source: &str,
    logical_source: &str,
) -> Result<(), PackageLoadError> {
    fn attribute_count(fragment: &str, tag: &str, attribute: &str) -> usize {
        let bytes = fragment.as_bytes();
        let mut index = 1 + tag.len();
        let mut count = 0usize;
        while index < bytes.len() {
            while index < bytes.len()
                && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/')
            {
                index += 1;
            }
            if index >= bytes.len() || bytes[index] == b'>' {
                break;
            }
            let name_start = index;
            while index < bytes.len()
                && !bytes[index].is_ascii_whitespace()
                && !matches!(bytes[index], b'=' | b'>' | b'/')
            {
                index += 1;
            }
            if fragment[name_start..index].eq_ignore_ascii_case(attribute) {
                count = count.saturating_add(1);
            }
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index >= bytes.len() || bytes[index] != b'=' {
                continue;
            }
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index >= bytes.len() {
                break;
            }
            if matches!(bytes[index], b'\'' | b'"') {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                index = index.saturating_add(1);
            } else {
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
            }
        }
        count
    }

    let lowercase = source.to_ascii_lowercase();
    for tag in [TEMPLATE_ELEMENT, USE_ELEMENT] {
        let needle = format!("<{tag}");
        let mut offset = 0usize;
        while let Some(relative) = lowercase[offset..].find(&needle) {
            let start = offset + relative;
            let boundary = lowercase.as_bytes().get(start + needle.len()).copied();
            if boundary
                .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'/' | b'>'))
            {
                offset = start.saturating_add(needle.len());
                continue;
            }
            let Some(end_relative) = source[start..].find('>') else {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentSourceParse,
                    format!("unterminated `<{tag}>` start tag"),
                )
                .at(logical_source));
            };
            let end = start + end_relative;
            let fragment = &source[start..=end];
            let attribute = if tag == TEMPLATE_ELEMENT {
                COMPONENT_ATTRIBUTE
            } else {
                "component"
            };
            let count = attribute_count(fragment, tag, attribute);
            if count > 1 {
                return Err(PackageLoadError::new(
                    if tag == TEMPLATE_ELEMENT {
                        PackageErrorKind::InvalidComponentExport
                    } else {
                        PackageErrorKind::ComponentInvocationAttributes
                    },
                    format!("`<{tag}>` repeats the `{attribute}` attribute"),
                )
                .at(logical_source));
            }
            offset = end.saturating_add(1);
        }
    }
    Ok(())
}

fn component_error(
    kind: PackageErrorKind,
    owner: &PackageId,
    logical_source: &str,
    message: impl Into<String>,
) -> PackageLoadError {
    PackageLoadError::new(kind, message)
        .in_package(owner.to_string())
        .at(logical_source)
}
