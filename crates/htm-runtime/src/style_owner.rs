use crate::component::{ComponentDefinitionId, ComponentInstanceId};
use crate::stylesheet::prepare_author_stylesheet;
use crate::{PackageSnapshotGeneration, RuntimeError};
use blitz_dom::{SelectorRelations, SelectorScopeError};
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use stylo::stylesheets::DocumentStyleSheet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StyleOwnerId {
    RootDocument {
        snapshot_generation: PackageSnapshotGeneration,
        document_serial: u64,
    },
    ComponentInstance(ComponentInstanceId),
}

impl StyleOwnerId {
    pub(crate) fn root(
        snapshot_generation: PackageSnapshotGeneration,
        document_serial: u64,
    ) -> Self {
        Self::RootDocument {
            snapshot_generation,
            document_serial,
        }
    }

    pub(crate) fn component(instance: &ComponentInstanceId) -> Self {
        Self::ComponentInstance(instance.clone())
    }

    pub(crate) fn definition(&self) -> StylesheetOwnerId {
        match self {
            Self::RootDocument { .. } => StylesheetOwnerId::RootDocument,
            Self::ComponentInstance(instance) => {
                StylesheetOwnerId::ComponentDefinition(ComponentDefinitionId {
                    generation: instance.snapshot_generation(),
                    key: instance.definition().clone(),
                })
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn deterministic_string(&self) -> String {
        match self {
            Self::RootDocument {
                snapshot_generation,
                document_serial,
            } => format!(
                "root-style-owner@{}#{document_serial}",
                snapshot_generation.get()
            ),
            Self::ComponentInstance(instance) => {
                format!("component-style-owner:{}", instance.deterministic_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StyleOwnedNodeKind {
    RootDocument,
    ComponentDefinition,
    ComponentFallback,
    CallerProjected,
    NestedComponent,
}

impl StyleOwnedNodeKind {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RootDocument => "root-document",
            Self::ComponentDefinition => "component-definition",
            Self::ComponentFallback => "component-fallback",
            Self::CallerProjected => "caller-projected",
            Self::NestedComponent => "nested-component",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyleOwnedNode {
    dom_slot: usize,
    dom_slot_generation: u64,
    owner: StyleOwnerId,
    kind: StyleOwnedNodeKind,
}

impl StyleOwnedNode {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn dom_slot(&self) -> usize {
        self.dom_slot
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn dom_slot_generation(&self) -> u64 {
        self.dom_slot_generation
    }

    pub(crate) const fn owner(&self) -> &StyleOwnerId {
        &self.owner
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn kind(&self) -> StyleOwnedNodeKind {
        self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyleOwnership {
    root_owner: StyleOwnerId,
    nodes: BTreeMap<usize, StyleOwnedNode>,
}

impl StyleOwnership {
    pub(crate) fn new(
        snapshot_generation: PackageSnapshotGeneration,
        document_serial: u64,
        document_root_slot: usize,
    ) -> Self {
        let root_owner = StyleOwnerId::root(snapshot_generation, document_serial);
        let mut ownership = Self {
            root_owner: root_owner.clone(),
            nodes: BTreeMap::new(),
        };
        ownership.record(
            document_root_slot,
            root_owner,
            StyleOwnedNodeKind::RootDocument,
        );
        ownership
    }

    pub(crate) fn root_owner(&self) -> &StyleOwnerId {
        &self.root_owner
    }

    pub(crate) fn record(
        &mut self,
        dom_slot: usize,
        owner: StyleOwnerId,
        kind: StyleOwnedNodeKind,
    ) {
        let replaced = self.nodes.insert(
            dom_slot,
            StyleOwnedNode {
                dom_slot,
                dom_slot_generation: 0,
                owner,
                kind,
            },
        );
        assert!(
            replaced.is_none(),
            "one materialized DOM node cannot receive two style owners"
        );
    }

    pub(crate) fn node(&self, dom_slot: usize) -> Option<&StyleOwnedNode> {
        self.nodes.get(&dom_slot)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn nodes(&self) -> impl ExactSizeIterator<Item = &StyleOwnedNode> {
        self.nodes.values()
    }

    pub(crate) fn validate_complete(&self, document: &HtmlDocument) -> Result<(), RuntimeError> {
        let live = document
            .tree()
            .iter()
            .map(|(slot, _)| slot)
            .collect::<BTreeSet<_>>();
        let owned = self.nodes.keys().copied().collect::<BTreeSet<_>>();
        if live != owned {
            return Err(RuntimeError::InvalidPackage(format!(
                "style ownership is incomplete: {} live DOM nodes, {} owned nodes",
                live.len(),
                owned.len()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StylesheetOwnerId {
    RootDocument,
    ComponentDefinition(ComponentDefinitionId),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OwnedStylesheetSourceId(Arc<str>);

impl OwnedStylesheetSourceId {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedStylesheetSource {
    id: OwnedStylesheetSourceId,
    logical_name: Arc<str>,
    css: Arc<str>,
}

impl OwnedStylesheetSource {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        id: OwnedStylesheetSourceId,
        logical_name: impl Into<Arc<str>>,
        css: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            id,
            logical_name: logical_name.into(),
            css: css.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StylesheetOwnerAssociation {
    owner: StylesheetOwnerId,
    order: u16,
    source: Arc<OwnedStylesheetSource>,
}

impl StylesheetOwnerAssociation {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        owner: StylesheetOwnerId,
        order: u16,
        source: Arc<OwnedStylesheetSource>,
    ) -> Self {
        Self {
            owner,
            order,
            source,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OwnedAuthorStyles {
    associations: Arc<[StylesheetOwnerAssociation]>,
}

impl OwnedAuthorStyles {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        mut associations: Vec<StylesheetOwnerAssociation>,
    ) -> Result<Self, RuntimeError> {
        associations.sort_by(|left, right| {
            (&left.owner, left.order, &left.source.id).cmp(&(
                &right.owner,
                right.order,
                &right.source.id,
            ))
        });
        for duplicate in associations.windows(2) {
            if duplicate[0].owner == duplicate[1].owner && duplicate[0].order == duplicate[1].order
            {
                return Err(RuntimeError::InvalidPackage(
                    "one stylesheet owner cannot have duplicate cascade ordinals".into(),
                ));
            }
        }
        let mut source_text = BTreeMap::<&OwnedStylesheetSourceId, &str>::new();
        for association in &associations {
            if let Some(existing) =
                source_text.insert(&association.source.id, &association.source.css)
                && existing != &*association.source.css
            {
                return Err(RuntimeError::InvalidPackage(
                    "one stylesheet source identity cannot name different CSS".into(),
                ));
            }
        }
        Ok(Self {
            associations: associations.into(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum StyleActivationMode {
    #[default]
    LegacyDocumentGlobal,
    #[cfg_attr(not(test), allow(dead_code))]
    OwnershipAware(OwnedAuthorStyles),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StyleActivationEvidence {
    pub(crate) parsed_stylesheets: usize,
    pub(crate) stylesheet_associations: usize,
    pub(crate) scope_definitions: usize,
    pub(crate) scope_instances: usize,
    pub(crate) scoped_elements: usize,
}

pub(crate) fn activate_style_ownership(
    document: &mut HtmlDocument,
    ownership: &StyleOwnership,
    mode: &StyleActivationMode,
) -> Result<StyleActivationEvidence, RuntimeError> {
    ownership.validate_complete(document)?;
    let StyleActivationMode::OwnershipAware(author_styles) = mode else {
        return Ok(StyleActivationEvidence::default());
    };

    let mut parsed_sources = BTreeMap::<OwnedStylesheetSourceId, DocumentStyleSheet>::new();
    for association in author_styles.associations.iter() {
        if !parsed_sources.contains_key(&association.source.id) {
            let sheet = prepare_author_stylesheet(
                document,
                &association.source.css,
                &association.source.logical_name,
            )?;
            parsed_sources.insert(association.source.id.clone(), sheet);
        }
    }

    let mut grouped = BTreeMap::<StylesheetOwnerId, Vec<&StylesheetOwnerAssociation>>::new();
    for association in author_styles.associations.iter() {
        grouped
            .entry(association.owner.clone())
            .or_default()
            .push(association);
    }

    if let Some(root) = grouped.get(&StylesheetOwnerId::RootDocument) {
        let owner_node = first_owned_element(document, ownership, ownership.root_owner())
            .ok_or_else(|| {
                RuntimeError::InvalidPackage(
                    "root stylesheet ownership has no root-owned element".into(),
                )
            })?;
        for association in root {
            let sheet = parsed_sources
                .get(&association.source.id)
                .expect("every associated source was parsed")
                .clone();
            document.add_stylesheet_for_node(sheet, owner_node);
        }
    }

    let element_order = element_slots_in_tree_order(document);
    let relations = selector_relations(document, ownership, &element_order)?;
    for (slot, relation) in &relations {
        document
            .set_node_selector_relations(*slot, *relation)
            .map_err(selector_scope_error)?;
    }

    let mut elements_by_owner = BTreeMap::<StyleOwnerId, Vec<usize>>::new();
    for slot in element_order {
        let owner = ownership.node(slot).ok_or_else(|| {
            RuntimeError::InvalidPackage(format!("element slot {slot} has no style owner"))
        })?;
        elements_by_owner
            .entry(owner.owner.clone())
            .or_default()
            .push(slot);
    }

    let mut scope_definitions = BTreeMap::new();
    let component_definitions = elements_by_owner
        .keys()
        .filter_map(|owner| match owner.definition() {
            StylesheetOwnerId::RootDocument => None,
            component @ StylesheetOwnerId::ComponentDefinition(_) => Some(component),
        })
        .collect::<BTreeSet<_>>();
    for definition in component_definitions {
        let sheets = grouped
            .get(&definition)
            .into_iter()
            .flatten()
            .map(|association| {
                parsed_sources
                    .get(&association.source.id)
                    .expect("every associated source was parsed")
                    .clone()
            })
            .collect::<Vec<_>>();
        let scope = document
            .create_selector_scope(sheets)
            .map_err(selector_scope_error)?;
        scope_definitions.insert(definition, scope);
    }

    let mut scope_instances = 0usize;
    let mut scoped_elements = 0usize;
    for (owner, elements) in elements_by_owner {
        let definition = owner.definition();
        if definition == StylesheetOwnerId::RootDocument {
            continue;
        }
        let scope = scope_definitions
            .get(&definition)
            .expect("component elements have one scope definition");
        let instance = document
            .create_selector_scope_instance(scope)
            .map_err(selector_scope_error)?;
        let root = elements
            .iter()
            .copied()
            .find(|slot| {
                relations
                    .get(slot)
                    .is_some_and(|entry| entry.parent.is_none())
            })
            .unwrap_or(elements[0]);
        for slot in elements {
            document
                .set_node_selector_scope(slot, root, &instance)
                .map_err(selector_scope_error)?;
            scoped_elements = scoped_elements.saturating_add(1);
        }
        scope_instances = scope_instances.saturating_add(1);
    }

    Ok(StyleActivationEvidence {
        parsed_stylesheets: parsed_sources.len(),
        stylesheet_associations: author_styles.associations.len(),
        scope_definitions: scope_definitions.len(),
        scope_instances,
        scoped_elements,
    })
}

fn selector_scope_error(error: SelectorScopeError) -> RuntimeError {
    RuntimeError::InvalidPackage(format!("selector ownership scope is invalid: {error}"))
}

fn first_owned_element(
    document: &HtmlDocument,
    ownership: &StyleOwnership,
    owner: &StyleOwnerId,
) -> Option<usize> {
    element_slots_in_tree_order(document)
        .into_iter()
        .find(|slot| {
            ownership
                .node(*slot)
                .is_some_and(|node| node.owner() == owner)
        })
}

fn element_slots_in_tree_order(document: &HtmlDocument) -> Vec<usize> {
    let mut result = Vec::new();
    let mut stack = vec![0usize];
    while let Some(slot) = stack.pop() {
        let Some(node) = document.get_node(slot) else {
            continue;
        };
        if node.element_data().is_some() {
            result.push(slot);
        }
        stack.extend(node.children.iter().rev().copied());
    }
    result
}

fn selector_relations(
    document: &HtmlDocument,
    ownership: &StyleOwnership,
    element_order: &[usize],
) -> Result<BTreeMap<usize, SelectorRelations>, RuntimeError> {
    let mut parent_by_slot = BTreeMap::new();
    let mut children = BTreeMap::<(StyleOwnerId, Option<usize>), Vec<usize>>::new();
    for slot in element_order {
        let owned = ownership.node(*slot).ok_or_else(|| {
            RuntimeError::InvalidPackage(format!("element slot {slot} has no style owner"))
        })?;
        let mut parent = document.get_node(*slot).and_then(|node| node.parent);
        let selector_parent = loop {
            let Some(parent_slot) = parent else {
                break None;
            };
            let parent_node = document.get_node(parent_slot).ok_or_else(|| {
                RuntimeError::InvalidPackage(format!(
                    "element slot {slot} has a missing rendered ancestor"
                ))
            })?;
            if parent_node.element_data().is_some()
                && ownership
                    .node(parent_slot)
                    .is_some_and(|candidate| candidate.owner() == owned.owner())
            {
                break Some(parent_slot);
            }
            parent = parent_node.parent;
        };
        parent_by_slot.insert(*slot, selector_parent);
        children
            .entry((owned.owner.clone(), selector_parent))
            .or_default()
            .push(*slot);
    }

    let mut result = element_order
        .iter()
        .map(|slot| {
            (
                *slot,
                SelectorRelations {
                    parent: *parent_by_slot
                        .get(slot)
                        .expect("every element was classified"),
                    previous_sibling: None,
                    next_sibling: None,
                    first_child: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for ((_, parent), siblings) in children {
        for (index, slot) in siblings.iter().copied().enumerate() {
            let relation = result
                .get_mut(&slot)
                .expect("every element relation was initialized");
            relation.parent = parent;
            relation.previous_sibling = index.checked_sub(1).map(|before| siblings[before]);
            relation.next_sibling = siblings.get(index + 1).copied();
        }
        if let Some(parent) = parent {
            result
                .get_mut(&parent)
                .expect("selector parent is an element in the same owner")
                .first_child = siblings.first().copied();
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "gpu-renderer")]
    use crate::ExperimentalDocumentIdentity;
    #[cfg(feature = "gpu-renderer")]
    use crate::identity::IdentityRegistry;
    #[cfg(feature = "gpu-renderer")]
    use crate::model::ViewportSpec;
    #[cfg(feature = "gpu-renderer")]
    use crate::render::{CpuRenderSession, FrameReasonSet, RenderSurfaceId};
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::HtmlProvider;
    use blitz_paint::paint_scene;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    const WIDTH: u32 = 560;
    const HEIGHT: u32 = 720;
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "htmshell-style-owner-{label}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn ownership_fixture() -> Fixture {
        let fixture = Fixture::new("scope");
        fixture.write(
            "shell.json",
            r#"{
              "version":2,
              "package":{"id":"org.example.scope","kind":"shell","version":"1.0.0"},
              "dependencies":[],
              "components":[
                {"name":"frame-a","source":"components/all.html","slots":[{"name":"default","required":false}]},
                {"name":"frame-b","source":"components/all.html"},
                {"name":"inner-view","source":"components/all.html","slots":[{"name":"default","required":false}]}
              ],
              "surfaces":[
                {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":80,"reserveSpace":true},
                {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
              ]
            }"#,
        );
        fixture.write(
            "index.html",
            r#"<!doctype html><html><head><style>
              html, body { margin: 0; }
              body { display: flex; flex-wrap: wrap; }
              .shared { width: 28px; height: 28px; background: rgb(255, 0, 0); }
              .root-wrapper > .root-projected { background: rgb(255, 64, 64); }
              .a-node, .child-internal { background: rgb(255, 32, 32); }
            </style></head><body><main class="root-wrapper">
              <div class="shared root-associated" data-case="root"></div>
              <htm-use component="frame-a"></htm-use>
              <htm-use component="frame-a"><div class="shared root-projected" data-case="root-projected"></div></htm-use>
              <htm-use component="frame-b"></htm-use>
            </main></body></html>"#,
        );
        fixture.write(
            "components/all.html",
            r#"<!doctype html>
            <template data-htm-component="frame-a">
              <section class="a-wrapper">
                <div class="shared a-node" data-case="a-node"></div>
                <aside class="type-target" data-case="type"></aside>
                <div class="shared compound" data-case="compound"></div>
                <div class="shared source-order" data-case="source-order"></div>
                <div class="shared sheet-order" data-case="sheet-order"></div>
                <htm-use component="inner-view"><span class="shared projected" data-case="a-projected"></span></htm-use>
                <button class="shared interactive" data-case="interactive"></button>
                <slot><span class="shared fallback" data-case="fallback"></span></slot>
              </section>
            </template>
            <template data-htm-component="frame-b">
              <section class="b-wrapper">
                <div class="shared b-node" data-case="b-node"></div>
                <div class="shared inline" data-case="inline" style="background:rgb(255,255,0)"></div>
              </section>
            </template>
            <template data-htm-component="inner-view">
              <article class="inner-wrapper">
                <div class="shared child-internal" data-case="child"></div>
                <slot></slot>
              </article>
            </template>"#,
        );
        fixture.write("panel.html", "<!doctype html><html><body></body></html>");
        fixture.write("overlay.html", "<!doctype html><html><body></body></html>");
        fixture
    }

    fn inheritance_fixture() -> Fixture {
        let fixture = Fixture::new("inheritance");
        fixture.write(
            "shell.json",
            r#"{
              "version":2,
              "package":{"id":"org.example.inheritance","kind":"shell","version":"1.0.0"},
              "dependencies":[],
              "components":[
                {"name":"leaf-root","source":"components/all.html"},
                {"name":"leaf-nested","source":"components/all.html"},
                {"name":"outer-frame","source":"components/all.html"},
                {"name":"content-projector","source":"components/all.html","slots":[{"name":"default","required":false}]}
              ],
              "surfaces":[
                {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":80,"reserveSpace":true},
                {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
              ]
            }"#,
        );
        fixture.write(
            "index.html",
            r#"<!doctype html><html><body>
              <div style="color:rgb(1,2,3);opacity:0.25"><htm-use component="leaf-root"></htm-use></div>
              <htm-use component="outer-frame"></htm-use>
              <htm-use component="content-projector"><span class="leak-target" data-case="projected-inherited" style="width:20px;height:20px"></span></htm-use>
              <htm-use component="content-projector"></htm-use>
            </body></html>"#,
        );
        fixture.write(
            "components/all.html",
            r#"<!doctype html>
            <template data-htm-component="leaf-root">
              <div class="leak-target" data-case="root-inherited" style="width:20px;height:20px"></div>
            </template>
            <template data-htm-component="leaf-nested">
              <div class="leak-target" data-case="nested-inherited" style="width:20px;height:20px"></div>
            </template>
            <template data-htm-component="outer-frame">
              <section style="color:rgb(1,2,3);opacity:0.25"><htm-use component="leaf-nested"></htm-use></section>
            </template>
            <template data-htm-component="content-projector">
              <section style="color:rgb(1,2,3);opacity:0.25"><slot><span class="leak-target" data-case="fallback-inherited" style="width:20px;height:20px"></span></slot></section>
            </template>"#,
        );
        fixture.write("panel.html", "<!doctype html><html><body></body></html>");
        fixture.write("overlay.html", "<!doctype html><html><body></body></html>");
        fixture
    }

    fn repeated_scope_fixture(instance_count: usize) -> Fixture {
        let fixture = Fixture::new("stress");
        fixture.write(
            "shell.json",
            r#"{
              "version":2,
              "package":{"id":"org.example.stress","kind":"shell","version":"1.0.0"},
              "dependencies":[],
              "components":[{"name":"scope-item","source":"components/item.html"}],
              "surfaces":[
                {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":80,"reserveSpace":true},
                {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
              ]
            }"#,
        );
        let invocations = "<htm-use component=\"scope-item\"></htm-use>".repeat(instance_count);
        fixture.write(
            "index.html",
            format!("<!doctype html><html><body>{invocations}</body></html>"),
        );
        fixture.write(
            "components/item.html",
            r#"<!doctype html><template data-htm-component="scope-item"><button class="stress-node" data-case="stress"></button></template>"#,
        );
        fixture.write("panel.html", "<!doctype html><html><body></body></html>");
        fixture.write("overlay.html", "<!doctype html><html><body></body></html>");
        fixture
    }

    fn nested_scope_fixture(depth: usize) -> Fixture {
        let fixture = Fixture::new("nested");
        let exports = (0..depth)
            .map(|level| format!(r#"{{"name":"level-{level}","source":"components/levels.html"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let definitions = (0..depth)
            .map(|level| {
                let child = if level + 1 < depth {
                    format!(r#"<htm-use component="level-{}"></htm-use>"#, level + 1)
                } else {
                    String::new()
                };
                format!(
                    r#"<template data-htm-component="level-{level}"><section class="level">{child}</section></template>"#
                )
            })
            .collect::<String>();
        fixture.write(
            "shell.json",
            format!(
                r#"{{
                  "version":2,
                  "package":{{"id":"org.example.nested","kind":"shell","version":"1.0.0"}},
                  "dependencies":[],
                  "components":[{exports}],
                  "surfaces":[
                    {{"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":80,"reserveSpace":true}},
                    {{"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}}
                  ]
                }}"#
            ),
        );
        fixture.write(
            "index.html",
            r#"<!doctype html><html><body><htm-use component="level-0"></htm-use></body></html>"#,
        );
        fixture.write("components/levels.html", definitions);
        fixture.write("panel.html", "<!doctype html><html><body></body></html>");
        fixture.write("overlay.html", "<!doctype html><html><body></body></html>");
        fixture
    }

    fn instantiate_snapshot(
        snapshot: &Arc<crate::PackageSnapshot>,
        document_serial: u64,
    ) -> crate::component::InstantiatedDocument {
        let prepared = snapshot
            .headless_entry()
            .unwrap()
            .prepared_document()
            .unwrap();
        snapshot
            .instantiate_document(
                prepared,
                document_serial,
                DocumentConfig {
                    viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Dark)),
                    html_parser_provider: Some(Arc::new(HtmlProvider)),
                    style_threading: StyleThreading::Sequential,
                    ..DocumentConfig::default()
                },
            )
            .unwrap()
    }

    fn instantiate(
        fixture: &Fixture,
        document_serial: u64,
    ) -> crate::component::InstantiatedDocument {
        let snapshot = crate::PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .unwrap();
        instantiate_snapshot(&snapshot, document_serial)
    }

    fn definition(
        instances: &[crate::ComponentInstanceRecord],
        name: &str,
    ) -> ComponentDefinitionId {
        instances
            .iter()
            .find(|instance| instance.reference().name().as_str() == name)
            .unwrap_or_else(|| panic!("component {name} was not instantiated"))
            .definition_id()
            .clone()
    }

    fn source(id: &str, css: &str) -> Arc<OwnedStylesheetSource> {
        Arc::new(OwnedStylesheetSource::new(
            OwnedStylesheetSourceId::new(id),
            format!("synthetic/{id}.css"),
            css,
        ))
    }

    fn association(
        owner: StylesheetOwnerId,
        order: u16,
        source: Arc<OwnedStylesheetSource>,
    ) -> StylesheetOwnerAssociation {
        StylesheetOwnerAssociation::new(owner, order, source)
    }

    fn case_slots(document: &HtmlDocument, value: &str) -> Vec<usize> {
        document
            .tree()
            .iter()
            .filter_map(|(slot, node)| {
                node.element_data()
                    .and_then(|element| {
                        element
                            .attrs()
                            .iter()
                            .find(|attribute| attribute.name.local.as_ref() == "data-case")
                    })
                    .is_some_and(|attribute| attribute.value.as_str() == value)
                    .then_some(slot)
            })
            .collect()
    }

    fn case_slot(document: &HtmlDocument, value: &str) -> usize {
        let slots = case_slots(document, value);
        assert_eq!(slots.len(), 1, "case {value} should identify one node");
        slots[0]
    }

    fn center(document: &HtmlDocument, slot: usize) -> (f32, f32) {
        let node = document.get_node(slot).unwrap();
        let position = node.absolute_position(0.0, 0.0);
        (
            position.x + node.final_layout.size.width / 2.0,
            position.y + node.final_layout.size.height / 2.0,
        )
    }

    fn render(document: &mut HtmlDocument) -> Vec<u8> {
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, document.as_mut(), 1.0, WIDTH, HEIGHT, 0, 0),
            WIDTH,
            HEIGHT,
        )
    }

    fn pixel(document: &HtmlDocument, pixels: &[u8], slot: usize) -> [u8; 4] {
        let (x, y) = center(document, slot);
        let index = (y as usize * WIDTH as usize + x as usize) * 4;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    }

    fn color(document: &HtmlDocument, slot: usize) -> String {
        use style_traits::ToCss;

        document
            .get_node(slot)
            .unwrap()
            .primary_styles()
            .map(|styles| styles.clone_color().to_css_string())
            .unwrap()
    }

    fn opacity(document: &HtmlDocument, slot: usize) -> f32 {
        document
            .get_node(slot)
            .unwrap()
            .primary_styles()
            .map(|styles| styles.get_effects().opacity)
            .unwrap()
    }

    fn process_resource_counts() -> (usize, usize, Option<u64>) {
        let file_descriptors = fs::read_dir("/proc/self/fd")
            .map(|entries| entries.count())
            .unwrap_or(0);
        let threads = fs::read_dir("/proc/self/task")
            .map(|entries| entries.count())
            .unwrap_or(0);
        let rss_kib = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    line.strip_prefix("VmRSS:")
                        .and_then(|value| value.split_whitespace().next())
                        .and_then(|value| value.parse().ok())
                })
            });
        (file_descriptors, threads, rss_kib)
    }

    #[cfg(feature = "gpu-renderer")]
    struct OwnershipGpuProof {
        gpu_used: bool,
        info: Option<crate::render::BackendInfo>,
        max_difference: u8,
        samples: Vec<([u8; 4], [u8; 4])>,
    }

    #[cfg(feature = "gpu-renderer")]
    fn ownership_gpu_proof(force_software_adapter: bool) -> OwnershipGpuProof {
        let fixture = ownership_fixture();
        let mut instantiated = instantiate(&fixture, 901);
        let frame_a = definition(&instantiated.instances, "frame-a");
        let frame_b = definition(&instantiated.instances, "frame-b");
        let inner = definition(&instantiated.instances, "inner-view");
        let styles = OwnedAuthorStyles::new(vec![
            association(
                StylesheetOwnerId::ComponentDefinition(frame_a),
                0,
                source(
                    "gpu-a",
                    ".shared { width:28px; height:28px; background:rgb(0,0,255); }",
                ),
            ),
            association(
                StylesheetOwnerId::ComponentDefinition(frame_b),
                0,
                source(
                    "gpu-b",
                    ".shared { width:28px; height:28px; background:rgb(128,0,128); }",
                ),
            ),
            association(
                StylesheetOwnerId::ComponentDefinition(inner),
                0,
                source(
                    "gpu-inner",
                    ".shared { width:28px; height:28px; background:rgb(0,255,0); }",
                ),
            ),
        ])
        .unwrap();
        activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::OwnershipAware(styles),
        )
        .unwrap();
        instantiated.document.resolve(0.0);

        let identities = IdentityRegistry::from_document(&instantiated.document);
        let mut session = CpuRenderSession::default();
        let prepared = session
            .prepare_document(
                &mut instantiated.document,
                &identities,
                ExperimentalDocumentIdentity { serial: 901 },
                ViewportSpec {
                    logical_width: WIDTH,
                    logical_height: HEIGHT,
                    ..ViewportSpec::default()
                },
                RenderSurfaceId {
                    instance: 901,
                    generation: 1,
                },
                WIDTH,
                HEIGHT,
                1,
                1,
                FrameReasonSet::new(),
                true,
            )
            .unwrap()
            .unwrap();
        let cpu = session.render_prepared_cpu(&prepared).unwrap().pixels;
        let (gpu, gpu_used, info) =
            crate::render::render_prepared_for_test(&prepared, force_software_adapter).unwrap();
        let cases = [
            "root",
            "root-projected",
            "a-node",
            "fallback",
            "a-projected",
            "child",
            "b-node",
        ];
        let samples = cases
            .into_iter()
            .map(|case| {
                let slot = case_slots(&instantiated.document, case)[0];
                (
                    pixel(&instantiated.document, &cpu, slot),
                    pixel(&instantiated.document, &gpu, slot),
                )
            })
            .collect::<Vec<_>>();
        let max_difference = samples
            .iter()
            .flat_map(|(cpu, gpu)| cpu.iter().zip(gpu))
            .map(|(cpu, gpu)| cpu.abs_diff(*gpu))
            .max()
            .unwrap_or(0);
        OwnershipGpuProof {
            gpu_used,
            info,
            max_difference,
            samples,
        }
    }

    #[test]
    fn prepared_component_nodes_receive_generation_safe_style_owners() {
        let fixture = ownership_fixture();
        let instantiated = instantiate(&fixture, 41);
        let ownership = &instantiated.style_ownership;

        assert_eq!(ownership.nodes().len(), instantiated.document.tree().len());
        assert_eq!(
            ownership
                .node(case_slot(&instantiated.document, "root"))
                .unwrap()
                .kind(),
            StyleOwnedNodeKind::RootDocument
        );
        assert_eq!(
            ownership
                .node(case_slot(&instantiated.document, "fallback"))
                .unwrap()
                .kind(),
            StyleOwnedNodeKind::ComponentFallback
        );
        assert_eq!(
            ownership
                .node(case_slot(&instantiated.document, "root-projected"))
                .unwrap()
                .kind(),
            StyleOwnedNodeKind::CallerProjected
        );
        for slot in case_slots(&instantiated.document, "a-projected") {
            assert_eq!(
                ownership.node(slot).unwrap().kind(),
                StyleOwnedNodeKind::CallerProjected
            );
        }
        for slot in case_slots(&instantiated.document, "child") {
            assert_eq!(
                ownership.node(slot).unwrap().kind(),
                StyleOwnedNodeKind::NestedComponent
            );
        }
        for node in ownership.nodes() {
            assert_eq!(node.dom_slot_generation(), 0);
            assert_eq!(ownership.node(node.dom_slot()), Some(node));
            assert!(!node.owner().deterministic_string().is_empty());
            assert!(!node.kind().as_str().is_empty());
        }

        let replacement = instantiate(&fixture, 42);
        assert_ne!(
            ownership.root_owner(),
            replacement.style_ownership.root_owner()
        );
        let first_owner = ownership
            .node(case_slots(&instantiated.document, "a-node")[0])
            .unwrap()
            .owner();
        let replacement_owner = replacement
            .style_ownership
            .node(case_slots(&replacement.document, "a-node")[0])
            .unwrap()
            .owner();
        assert_ne!(first_owner, replacement_owner);
    }

    #[test]
    fn ownership_aware_associations_isolate_real_component_trees() {
        let fixture = ownership_fixture();
        let mut instantiated = instantiate(&fixture, 77);
        let frame_a = definition(&instantiated.instances, "frame-a");
        let frame_b = definition(&instantiated.instances, "frame-b");
        let inner = definition(&instantiated.instances, "inner-view");
        let common = source("common", ".common-marker { border: 1px solid white; }");
        let root = source("root", ".root-associated { background: rgb(255, 0, 0); }");
        let a = source(
            "a",
            r#"
            * { min-width: 28px; min-height: 28px; }
            .shared { display: block; width: 28px; height: 28px; background: rgb(0, 0, 255); }
            .a-wrapper > .a-node { background: rgb(0, 128, 255); }
            aside { background: rgb(10, 20, 30); }
            div.compound[data-case="compound"] { background: rgb(40, 50, 60); }
            .a-wrapper .fallback { background: rgb(0, 64, 255); }
            [data-case="a-projected"] { background: rgb(0, 192, 255); }
            .a-wrapper .child-internal { background: rgb(255, 255, 0); }
            .root-associated, .root-projected { background: rgb(0, 0, 255); }
            .source-order { background: rgb(17, 17, 17); }
            .source-order { background: rgb(34, 34, 34); }
            .sheet-order { background: rgb(0, 0, 255); }
            .interactive:hover { background: rgb(255, 128, 0); }
            .interactive:active { background: rgb(128, 0, 128); }
            "#,
        );
        let a_override = source(
            "a-override",
            ".sheet-order { background: rgb(0, 200, 200); }",
        );
        let b = source(
            "b",
            r#"
            * { min-width: 28px; min-height: 28px; }
            .shared { display: block; width: 28px; height: 28px; background: rgb(128, 0, 128); }
            "#,
        );
        let inner_source = source(
            "inner",
            r#"
            * { min-width: 28px; min-height: 28px; }
            .shared { display: block; width: 28px; height: 28px; background: rgb(0, 255, 0); }
            .inner-wrapper .projected { background: rgb(255, 255, 0); }
            "#,
        );
        let styles = OwnedAuthorStyles::new(vec![
            association(StylesheetOwnerId::RootDocument, 0, root),
            association(
                StylesheetOwnerId::ComponentDefinition(frame_a.clone()),
                0,
                Arc::clone(&common),
            ),
            association(StylesheetOwnerId::ComponentDefinition(frame_a), 1, a),
            association(
                StylesheetOwnerId::ComponentDefinition(definition(
                    &instantiated.instances,
                    "frame-a",
                )),
                2,
                a_override,
            ),
            association(
                StylesheetOwnerId::ComponentDefinition(frame_b.clone()),
                0,
                common,
            ),
            association(StylesheetOwnerId::ComponentDefinition(frame_b), 1, b),
            association(
                StylesheetOwnerId::ComponentDefinition(inner),
                0,
                inner_source,
            ),
        ])
        .unwrap();
        let evidence = activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::OwnershipAware(styles),
        )
        .unwrap();
        instantiated.document.resolve(0.0);

        assert_eq!(evidence.parsed_stylesheets, 6);
        assert_eq!(evidence.stylesheet_associations, 7);
        assert_eq!(evidence.scope_definitions, 3);
        assert_eq!(evidence.scope_instances, 5);
        assert!(evidence.scoped_elements >= 20);

        let pixels = render(&mut instantiated.document);
        assert_eq!(
            pixel(
                &instantiated.document,
                &pixels,
                case_slot(&instantiated.document, "root")
            ),
            [255, 0, 0, 255]
        );
        assert_eq!(
            pixel(
                &instantiated.document,
                &pixels,
                case_slot(&instantiated.document, "root-projected")
            ),
            [255, 64, 64, 255]
        );
        for slot in case_slots(&instantiated.document, "a-node") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [0, 128, 255, 255]
            );
        }
        for slot in case_slots(&instantiated.document, "type") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [10, 20, 30, 255]
            );
        }
        for slot in case_slots(&instantiated.document, "compound") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [40, 50, 60, 255]
            );
        }
        assert_eq!(
            pixel(
                &instantiated.document,
                &pixels,
                case_slot(&instantiated.document, "fallback")
            ),
            [0, 64, 255, 255]
        );
        for slot in case_slots(&instantiated.document, "a-projected") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [0, 192, 255, 255]
            );
        }
        for slot in case_slots(&instantiated.document, "child") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [0, 255, 0, 255]
            );
        }
        assert_eq!(
            pixel(
                &instantiated.document,
                &pixels,
                case_slot(&instantiated.document, "b-node")
            ),
            [128, 0, 128, 255]
        );
        assert_eq!(
            pixel(
                &instantiated.document,
                &pixels,
                case_slot(&instantiated.document, "inline")
            ),
            [255, 255, 0, 255]
        );
        for slot in case_slots(&instantiated.document, "source-order") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [34, 34, 34, 255]
            );
        }
        for slot in case_slots(&instantiated.document, "sheet-order") {
            assert_eq!(
                pixel(&instantiated.document, &pixels, slot),
                [0, 200, 200, 255]
            );
        }

        let interactive = case_slots(&instantiated.document, "interactive");
        let (x, y) = center(&instantiated.document, interactive[0]);
        assert!(instantiated.document.set_hover_to(x, y));
        instantiated.document.resolve(0.1);
        let pixels = render(&mut instantiated.document);
        assert_eq!(
            pixel(&instantiated.document, &pixels, interactive[0]),
            [255, 128, 0, 255]
        );
        assert_eq!(
            pixel(&instantiated.document, &pixels, interactive[1]),
            [0, 0, 255, 255]
        );
        assert!(instantiated.document.active_node());
        instantiated.document.resolve(0.2);
        let pixels = render(&mut instantiated.document);
        assert_eq!(
            pixel(&instantiated.document, &pixels, interactive[0]),
            [128, 0, 128, 255]
        );
        assert_eq!(
            pixel(&instantiated.document, &pixels, interactive[1]),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn ownership_aware_matching_preserves_rendered_inheritance_and_hit_testing() {
        let fixture = inheritance_fixture();
        let mut baseline = instantiate(&fixture, 90);
        activate_style_ownership(
            &mut baseline.document,
            &baseline.style_ownership,
            &StyleActivationMode::LegacyDocumentGlobal,
        )
        .unwrap();
        baseline.document.resolve(0.0);
        let mut instantiated = instantiate(&fixture, 91);
        let evidence = activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::OwnershipAware(OwnedAuthorStyles::default()),
        )
        .unwrap();
        instantiated.document.resolve(0.0);

        assert_eq!(evidence.parsed_stylesheets, 0);
        assert_eq!(evidence.scope_definitions, 4);
        assert_eq!(evidence.scope_instances, 5);
        for case in [
            "root-inherited",
            "nested-inherited",
            "projected-inherited",
            "fallback-inherited",
        ] {
            let slot = case_slot(&instantiated.document, case);
            let parent = instantiated
                .document
                .get_node(slot)
                .unwrap()
                .parent
                .unwrap();
            assert_eq!(
                color(&instantiated.document, slot),
                color(&instantiated.document, parent)
            );
            assert_eq!(opacity(&instantiated.document, slot), 1.0);
            let baseline_slot = case_slot(&baseline.document, case);
            let (baseline_x, baseline_y) = center(&baseline.document, baseline_slot);
            let baseline_hit = baseline
                .document
                .hit(baseline_x, baseline_y)
                .map(|hit| hit.node_id);
            let (x, y) = center(&instantiated.document, slot);
            assert_eq!(
                instantiated.document.hit(x, y).map(|hit| hit.node_id),
                baseline_hit
            );
            assert!(baseline_hit.is_some());
        }
    }

    #[test]
    fn legacy_activation_preserves_document_global_component_matching() {
        let fixture = ownership_fixture();
        let mut instantiated = instantiate(&fixture, 111);
        let evidence = activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::LegacyDocumentGlobal,
        )
        .unwrap();
        instantiated.document.resolve(0.0);

        assert_eq!(evidence, StyleActivationEvidence::default());
        let pixels = render(&mut instantiated.document);
        for case in ["root", "b-node"] {
            for slot in case_slots(&instantiated.document, case) {
                assert_eq!(
                    pixel(&instantiated.document, &pixels, slot),
                    [255, 0, 0, 255]
                );
            }
        }
        for case in ["a-node", "child"] {
            for slot in case_slots(&instantiated.document, case) {
                assert_eq!(
                    pixel(&instantiated.document, &pixels, slot),
                    [255, 32, 32, 255]
                );
            }
        }
    }

    #[test]
    fn output_local_scope_instances_share_sources_without_identity_aliasing() {
        let fixture = ownership_fixture();
        let mut first = instantiate(&fixture, 501);
        let mut second = instantiate(&fixture, 502);
        let first_definition = definition(&first.instances, "frame-a");
        let second_definition = definition(&second.instances, "frame-a");
        assert_eq!(first_definition, second_definition);
        let shared = source(
            "shared",
            r#"
            .shared { width: 28px; height: 28px; background: blue; }
            .interactive:hover { background: orange; }
            "#,
        );
        let first_styles = OwnedAuthorStyles::new(vec![association(
            StylesheetOwnerId::ComponentDefinition(first_definition),
            0,
            Arc::clone(&shared),
        )])
        .unwrap();
        let second_styles = OwnedAuthorStyles::new(vec![association(
            StylesheetOwnerId::ComponentDefinition(second_definition),
            0,
            shared,
        )])
        .unwrap();
        let first_evidence = activate_style_ownership(
            &mut first.document,
            &first.style_ownership,
            &StyleActivationMode::OwnershipAware(first_styles),
        )
        .unwrap();
        let second_evidence = activate_style_ownership(
            &mut second.document,
            &second.style_ownership,
            &StyleActivationMode::OwnershipAware(second_styles),
        )
        .unwrap();

        assert_eq!(first_evidence.parsed_stylesheets, 1);
        assert_eq!(second_evidence.parsed_stylesheets, 1);
        assert_ne!(
            first.style_ownership.root_owner(),
            second.style_ownership.root_owner()
        );
        assert_ne!(first.document.id(), second.document.id());
        first.document.resolve(0.0);
        second.document.resolve(0.0);
        let first_interactive = case_slots(&first.document, "interactive")[0];
        let second_interactive = case_slots(&second.document, "interactive")[0];
        let (x, y) = center(&first.document, first_interactive);
        assert!(first.document.set_hover_to(x, y));
        first.document.resolve(0.1);
        let first_pixels = render(&mut first.document);
        let second_pixels = render(&mut second.document);
        assert_eq!(
            pixel(&first.document, &first_pixels, first_interactive),
            [255, 165, 0, 255]
        );
        assert_eq!(
            pixel(&second.document, &second_pixels, second_interactive),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn package_and_document_replacement_allocate_fresh_owner_generations() {
        let fixture = ownership_fixture();
        let mut loader = crate::PackageSnapshotLoader::new();
        let first_snapshot = loader.load_headless(&fixture.root).unwrap();
        let second_snapshot = loader.load_headless(&fixture.root).unwrap();
        assert_ne!(first_snapshot.generation(), second_snapshot.generation());

        let first = instantiate_snapshot(&first_snapshot, 1);
        let second = instantiate_snapshot(&second_snapshot, 1);
        assert_ne!(
            first.style_ownership.root_owner(),
            second.style_ownership.root_owner()
        );
        let first_component = first
            .style_ownership
            .node(case_slots(&first.document, "a-node")[0])
            .unwrap()
            .owner();
        let second_component = second
            .style_ownership
            .node(case_slots(&second.document, "a-node")[0])
            .unwrap()
            .owner();
        assert_ne!(first_component, second_component);
    }

    #[test]
    fn thirty_two_nested_scope_instances_remain_distinct() {
        let fixture = nested_scope_fixture(32);
        let mut instantiated = instantiate(&fixture, 600);
        let evidence = activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::OwnershipAware(OwnedAuthorStyles::default()),
        )
        .unwrap();
        instantiated.document.resolve(0.0);

        assert_eq!(instantiated.instances.len(), 32);
        assert_eq!(evidence.scope_definitions, 32);
        assert_eq!(evidence.scope_instances, 32);
        let owners = instantiated
            .style_ownership
            .nodes()
            .filter_map(|node| match node.owner() {
                StyleOwnerId::RootDocument { .. } => None,
                StyleOwnerId::ComponentInstance(instance) => Some(instance),
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(owners.len(), 32);
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    fn ownership_aware_gpu_feature_path_matches_cpu_at_scope_boundaries() {
        let proof = ownership_gpu_proof(true);
        assert!(
            proof.max_difference <= 2,
            "CPU and GPU ownership samples diverged by {}: {:?}",
            proof.max_difference,
            proof.samples
        );
    }

    #[cfg(feature = "gpu-renderer")]
    #[test]
    #[ignore = "requires a physical Vulkan or GLES adapter"]
    fn ownership_aware_physical_gpu_scope_proof() {
        let proof = ownership_gpu_proof(false);
        let info = proof.info.expect("physical adapter information");
        assert!(
            proof.gpu_used,
            "ownership fixture used the CPU fallback: {info:?}"
        );
        assert_ne!(
            info.device_type, "Cpu",
            "software adapters are not physical hardware proof: {info:?}"
        );
        assert!(
            proof.max_difference <= 2,
            "CPU and physical GPU samples diverged by {}: {:?}",
            proof.max_difference,
            proof.samples
        );
        eprintln!("ownership-aware physical GPU adapter: {info:?}");
    }

    #[test]
    #[ignore = "release-only bounded selector ownership stress"]
    fn bounded_scope_stress_reuses_one_sheet_and_releases_generations() {
        let started = Instant::now();
        let before = process_resource_counts();
        let fixture = repeated_scope_fixture(1_000);
        let snapshot = crate::PackageSnapshotLoader::new()
            .load_headless(&fixture.root)
            .unwrap();
        let mut instantiated = instantiate_snapshot(&snapshot, 1);
        let definition = definition(&instantiated.instances, "scope-item");
        let stylesheet = source(
            "stress",
            r#"
            .stress-node { display:block; width:12px; height:12px; background:blue; }
            .stress-node:hover { background:orange; }
            .stress-node:active { background:purple; }
            "#,
        );
        let styles = OwnedAuthorStyles::new(vec![association(
            StylesheetOwnerId::ComponentDefinition(definition),
            0,
            stylesheet,
        )])
        .unwrap();
        let evidence = activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &StyleActivationMode::OwnershipAware(styles),
        )
        .unwrap();
        instantiated.document.resolve(0.0);
        assert_eq!(evidence.parsed_stylesheets, 1);
        assert_eq!(evidence.stylesheet_associations, 1);
        assert_eq!(evidence.scope_definitions, 1);
        assert_eq!(evidence.scope_instances, 1_000);
        let sibling_owners = instantiated
            .style_ownership
            .nodes()
            .filter_map(|node| match node.owner() {
                StyleOwnerId::RootDocument { .. } => None,
                StyleOwnerId::ComponentInstance(instance) => Some(instance),
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(sibling_owners.len(), 1_000);
        for pair in sibling_owners.iter().collect::<Vec<_>>().windows(2) {
            assert_ne!(pair[0], pair[1]);
        }

        let interaction_started = Instant::now();
        let targets = case_slots(&instantiated.document, "stress");
        for cycle in 0..500 {
            let target = targets[cycle % 2];
            let (x, y) = center(&instantiated.document, target);
            instantiated.document.clear_hover();
            instantiated.document.set_hover_to(x, y);
            instantiated.document.resolve(cycle as f64);
            instantiated.document.active_node();
            instantiated.document.resolve(cycle as f64 + 0.25);
            instantiated.document.unactive_node();
        }
        let interaction_elapsed = interaction_started.elapsed();

        let teardown_started = Instant::now();
        for serial in 2..=501 {
            let mut output = instantiate_snapshot(&snapshot, serial);
            activate_style_ownership(
                &mut output.document,
                &output.style_ownership,
                &StyleActivationMode::LegacyDocumentGlobal,
            )
            .unwrap();
            assert_eq!(
                output.style_ownership.nodes().len(),
                output.document.tree().len()
            );
        }
        let teardown_elapsed = teardown_started.elapsed();
        let after = process_resource_counts();
        eprintln!(
            "ownership stress: total={:?} interactions={interaction_elapsed:?} \
             generations={teardown_elapsed:?} before_fd={} after_fd={} before_threads={} \
             after_threads={} before_rss_kib={:?} after_rss_kib={:?}",
            started.elapsed(),
            before.0,
            after.0,
            before.1,
            after.1,
            before.2,
            after.2
        );
        assert!(
            after.0 <= before.0.saturating_add(4),
            "file descriptor growth exceeded the bounded allowance: {before:?} -> {after:?}"
        );
        assert!(
            after.1 <= before.1.saturating_add(1),
            "thread growth exceeded the bounded allowance: {before:?} -> {after:?}"
        );
    }
}
