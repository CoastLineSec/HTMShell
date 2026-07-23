use crate::identity::{IdentityRegistry, author_slots};
use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity, RuntimeError};
use blitz_dom::{LocalName, local_name};
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;

const ELEMENT_ATTRIBUTE: &str = "data-htm-element";
const BIND_ATTRIBUTE: &str = "data-htm-bind";
const ACTION_ATTRIBUTE: &str = "data-htm-action";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuiltInElementKind {
    StateText,
    ActionButton,
}

impl BuiltInElementKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateText => "state-text",
            Self::ActionButton => "action-button",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "state-text" => Some(Self::StateText),
            "action-button" => Some(Self::ActionButton),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBindingKey {
    OutputLabel,
    OutputScale,
    SurfaceTemplateId,
    OverlayStatus,
    OverlayActivationCount,
    ShellLastAction,
}

impl StateBindingKey {
    pub const ALL: [Self; 6] = [
        Self::OutputLabel,
        Self::OutputScale,
        Self::SurfaceTemplateId,
        Self::OverlayStatus,
        Self::OverlayActivationCount,
        Self::ShellLastAction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OutputLabel => "output.label",
            Self::OutputScale => "output.scale",
            Self::SurfaceTemplateId => "surface.template_id",
            Self::OverlayStatus => "overlay.status",
            Self::OverlayActivationCount => "overlay.activation_count",
            Self::ShellLastAction => "shell.last_action",
        }
    }
}

impl std::str::FromStr for StateBindingKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "output.label" => Ok(Self::OutputLabel),
            "output.scale" => Ok(Self::OutputScale),
            "surface.template_id" => Ok(Self::SurfaceTemplateId),
            "overlay.status" => Ok(Self::OverlayStatus),
            "overlay.activation_count" => Ok(Self::OverlayActivationCount),
            "shell.last_action" => Ok(Self::ShellLastAction),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellAction {
    OverlayToggle,
    OverlayClose,
    OverlayActivate,
}

impl ShellAction {
    pub const ALL: [Self; 3] = [
        Self::OverlayToggle,
        Self::OverlayClose,
        Self::OverlayActivate,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OverlayToggle => "overlay.toggle",
            Self::OverlayClose => "overlay.close",
            Self::OverlayActivate => "overlay.activate",
        }
    }
}

impl std::str::FromStr for ShellAction {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "overlay.toggle" => Ok(Self::OverlayToggle),
            "overlay.close" => Ok(Self::OverlayClose),
            "overlay.activate" => Ok(Self::OverlayActivate),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltInSurfaceKind {
    SingleOverlay,
    Panel,
    Overlay,
}

impl BuiltInSurfaceKind {
    pub(crate) fn permits(self, action: ShellAction) -> bool {
        matches!(
            (self, action),
            (Self::Panel, ShellAction::OverlayToggle)
                | (
                    Self::Overlay,
                    ShellAction::OverlayClose | ShellAction::OverlayActivate
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementInstanceId {
    pub document_generation: ExperimentalDocumentIdentity,
    pub html_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDeclaration {
    pub id: ElementInstanceId,
    pub kind: BuiltInElementKind,
    pub binding: Option<StateBindingKey>,
    pub action: Option<ShellAction>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuiltInElementSummary {
    pub registered_elements: usize,
    pub bindings: usize,
    pub actions: usize,
    pub discovery_scans: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BindingUpdate {
    pub changed_keys: usize,
    pub changed_elements: usize,
    pub suppressed_keys: usize,
}

#[derive(Debug, Clone, Copy)]
struct BuiltInElementDefinition {
    name: &'static str,
    allowed_tags: &'static [&'static str],
    required_attribute: &'static str,
}

const DEFINITIONS: [BuiltInElementDefinition; 2] = [
    BuiltInElementDefinition {
        name: "state-text",
        allowed_tags: &["span", "p", "output"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "action-button",
        allowed_tags: &["button"],
        required_attribute: ACTION_ATTRIBUTE,
    },
];

static REGISTRY_VALIDATION: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct IndexedElement {
    declaration: ElementDeclaration,
    node: ExperimentalNodeIdentity,
    depth: usize,
    order: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionTarget {
    pub(crate) id: ElementInstanceId,
    pub(crate) action: ShellAction,
    pub(crate) node: ExperimentalNodeIdentity,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltInElementIndex {
    elements: BTreeMap<String, IndexedElement>,
    bindings: BTreeMap<StateBindingKey, Vec<String>>,
    actions: Vec<String>,
    applied_values: BTreeMap<StateBindingKey, String>,
    surface_kind: BuiltInSurfaceKind,
    summary: BuiltInElementSummary,
}

impl BuiltInElementIndex {
    pub(crate) fn discover(
        document: &HtmlDocument,
        identities: &IdentityRegistry,
        document_generation: ExperimentalDocumentIdentity,
        surface_kind: BuiltInSurfaceKind,
        source: &str,
    ) -> Result<Self, RuntimeError> {
        ensure_registry_valid()?;
        let mut elements = BTreeMap::new();
        let mut bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut actions = Vec::new();
        let slots = author_slots(document);
        let mut id_counts: BTreeMap<String, usize> = BTreeMap::new();
        for slot in &slots {
            if let Some(id) = document
                .get_node(*slot)
                .and_then(|node| node.element_data())
                .and_then(|element| element.attr(local_name!("id")))
                .filter(|id| !id.is_empty())
            {
                *id_counts.entry(id.to_owned()).or_default() += 1;
            }
        }

        for (order, slot) in slots.into_iter().enumerate() {
            let Some(node) = document.get_node(slot) else {
                continue;
            };
            let Some(element) = node.element_data() else {
                continue;
            };
            let Some(kind_name) = element.attr(LocalName::from(ELEMENT_ATTRIBUTE)) else {
                continue;
            };
            let context = declaration_context(source, element.attr(local_name!("id")));
            let kind = BuiltInElementKind::parse(kind_name).ok_or_else(|| {
                invalid_declaration(&context, format!("unknown built-in element `{kind_name}`"))
            })?;
            let definition = definition(kind);
            let tag = element.name.local.as_ref();
            if !definition.allowed_tags.contains(&tag) {
                return Err(invalid_declaration(
                    &context,
                    format!(
                        "`{}` requires one of [{}], not <{tag}>",
                        definition.name,
                        definition.allowed_tags.join(", ")
                    ),
                ));
            }
            let html_id = element
                .attr(local_name!("id"))
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid_declaration(&context, "registered element requires `id`"))?
                .to_owned();
            if id_counts.get(&html_id).copied().unwrap_or_default() != 1 {
                return Err(invalid_declaration(
                    &context,
                    format!("registered id `{html_id}` is not unique in the document"),
                ));
            }
            for attribute in element.attrs() {
                let name = attribute.name.local.as_ref();
                if name.starts_with("data-htm-")
                    && !allowed_behavior_attributes(kind).contains(&name)
                {
                    return Err(invalid_declaration(
                        &context,
                        format!("unsupported HTMShell behavior attribute `{name}`"),
                    ));
                }
            }
            let required = element.attr(LocalName::from(definition.required_attribute));
            let required = required.filter(|value| !value.is_empty()).ok_or_else(|| {
                invalid_declaration(
                    &context,
                    format!(
                        "`{}` requires `{}`",
                        definition.name, definition.required_attribute
                    ),
                )
            })?;
            let (binding, action) = match kind {
                BuiltInElementKind::StateText => {
                    validate_state_text_children(document, slot, &context)?;
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    (Some(binding), None)
                }
                BuiltInElementKind::ActionButton => {
                    let action = required.parse::<ShellAction>().map_err(|()| {
                        invalid_declaration(&context, format!("unsupported action `{required}`"))
                    })?;
                    if !surface_kind.permits(action) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "action `{}` is not permitted from this surface kind",
                                action.as_str()
                            ),
                        ));
                    }
                    (None, Some(action))
                }
            };
            let instance_id = ElementInstanceId {
                document_generation,
                html_id: html_id.clone(),
            };
            let declaration = ElementDeclaration {
                id: instance_id,
                kind,
                binding,
                action,
                disabled: element.has_attr(local_name!("disabled")),
            };
            let indexed = IndexedElement {
                declaration,
                node: identities.identity_for_slot(document, slot)?,
                depth: node_depth(document, slot),
                order,
            };
            if let Some(binding) = binding {
                bindings.entry(binding).or_default().push(html_id.clone());
            }
            if action.is_some() {
                actions.push(html_id.clone());
            }
            elements.insert(html_id, indexed);
        }

        actions.sort_by_key(|id| {
            let element = &elements[id];
            (
                std::cmp::Reverse(element.depth),
                std::cmp::Reverse(element.order),
            )
        });
        for ids in bindings.values_mut() {
            ids.sort();
        }
        let summary = BuiltInElementSummary {
            registered_elements: elements.len(),
            bindings: bindings.values().map(Vec::len).sum(),
            actions: actions.len(),
            discovery_scans: 1,
        };
        Ok(Self {
            elements,
            bindings,
            actions,
            applied_values: BTreeMap::new(),
            surface_kind,
            summary,
        })
    }

    pub(crate) fn summary(&self) -> BuiltInElementSummary {
        self.summary
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub(crate) fn declarations(&self) -> Vec<ElementDeclaration> {
        self.elements
            .values()
            .map(|element| element.declaration.clone())
            .collect()
    }

    pub(crate) fn element(&self, html_id: &str) -> Option<&ElementDeclaration> {
        self.elements.get(html_id).map(|entry| &entry.declaration)
    }

    pub(crate) fn binding_targets(&self, key: StateBindingKey) -> &[String] {
        self.bindings.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(crate) fn binding_is_unchanged(&self, key: StateBindingKey, value: &str) -> bool {
        self.applied_values
            .get(&key)
            .is_some_and(|old| old == value)
    }

    pub(crate) fn record_binding(&mut self, key: StateBindingKey, value: String) {
        self.applied_values.insert(key, value);
    }

    pub(crate) fn indexed_node(&self, html_id: &str) -> Option<ExperimentalNodeIdentity> {
        self.elements.get(html_id).map(|entry| entry.node)
    }

    pub(crate) fn action_candidates(&self) -> impl Iterator<Item = &str> {
        self.actions.iter().map(String::as_str)
    }

    pub(crate) fn action_target(
        &self,
        html_id: &str,
        document: &HtmlDocument,
        identities: &IdentityRegistry,
    ) -> Result<Option<ActionTarget>, RuntimeError> {
        let Some(entry) = self.elements.get(html_id) else {
            return Ok(None);
        };
        let Some(action) = entry.declaration.action else {
            return Ok(None);
        };
        if !self.surface_kind.permits(action) {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "action `{}` is not permitted for the current surface",
                action.as_str()
            )));
        }
        let slot = identities.resolve(document, entry.node)?;
        let disabled = document
            .get_node(slot)
            .and_then(|node| node.element_data())
            .is_some_and(|element| element.has_attr(local_name!("disabled")));
        if disabled {
            return Ok(None);
        }
        Ok(Some(ActionTarget {
            id: entry.declaration.id.clone(),
            action,
            node: entry.node,
        }))
    }
}

pub fn built_in_registry_names() -> &'static [&'static str] {
    &["state-text", "action-button"]
}

pub(crate) fn ensure_registry_valid() -> Result<(), RuntimeError> {
    REGISTRY_VALIDATION
        .get_or_init(|| validate_definitions(&DEFINITIONS))
        .clone()
        .map_err(|message| RuntimeError::InvalidPackage(format!("built-in registry: {message}")))
}

fn validate_definitions(definitions: &[BuiltInElementDefinition]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for definition in definitions {
        if definition.name.is_empty() || !names.insert(definition.name) {
            return Err(format!(
                "registry entry `{}` is empty or duplicated",
                definition.name
            ));
        }
        if definition.allowed_tags.is_empty() {
            return Err(format!(
                "registry entry `{}` has no allowed HTML tags",
                definition.name
            ));
        }
    }
    Ok(())
}

fn definition(kind: BuiltInElementKind) -> &'static BuiltInElementDefinition {
    match kind {
        BuiltInElementKind::StateText => &DEFINITIONS[0],
        BuiltInElementKind::ActionButton => &DEFINITIONS[1],
    }
}

fn allowed_behavior_attributes(kind: BuiltInElementKind) -> &'static [&'static str] {
    match kind {
        BuiltInElementKind::StateText => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE],
        BuiltInElementKind::ActionButton => &[ELEMENT_ATTRIBUTE, ACTION_ATTRIBUTE],
    }
}

fn declaration_context(source: &str, id: Option<&str>) -> String {
    match id {
        Some(id) if !id.is_empty() => format!("{source} element `#{id}`"),
        _ => format!("{source} registered element"),
    }
}

fn invalid_declaration(context: &str, message: impl fmt::Display) -> RuntimeError {
    RuntimeError::InvalidPackage(format!("{context}: {message}"))
}

fn validate_state_text_children(
    document: &HtmlDocument,
    slot: usize,
    context: &str,
) -> Result<(), RuntimeError> {
    let node = document
        .get_node(slot)
        .ok_or_else(|| invalid_declaration(context, "runtime node lookup failed"))?;
    if node.children.iter().any(|child| {
        document
            .get_node(*child)
            .is_some_and(|node| node.element_data().is_some())
    }) {
        return Err(invalid_declaration(
            context,
            "state-text content must not contain child elements",
        ));
    }
    Ok(())
}

fn node_depth(document: &HtmlDocument, slot: usize) -> usize {
    let mut depth = 0usize;
    let mut current = document.get_node(slot).and_then(|node| node.parent);
    while let Some(parent) = current {
        depth = depth.saturating_add(1);
        current = document.get_node(parent).and_then(|node| node.parent);
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::HtmlProvider;
    use std::sync::Arc;

    fn discover(body: &str, kind: BuiltInSurfaceKind) -> Result<BuiltInElementIndex, RuntimeError> {
        let document = HtmlDocument::from_html(
            &format!("<!doctype html><html><body>{body}</body></html>"),
            DocumentConfig {
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        let identities = IdentityRegistry::from_document(&document);
        BuiltInElementIndex::discover(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 9 },
            kind,
            "fixture.html",
        )
    }

    #[test]
    fn registry_is_exact_deterministic_and_duplicate_safe() {
        assert_eq!(built_in_registry_names(), &["state-text", "action-button"]);
        assert!(validate_definitions(&DEFINITIONS).is_ok());
        let duplicate = [DEFINITIONS[0], DEFINITIONS[0]];
        assert!(validate_definitions(&duplicate).is_err());
        assert_eq!(
            StateBindingKey::ALL.map(StateBindingKey::as_str),
            [
                "output.label",
                "output.scale",
                "surface.template_id",
                "overlay.status",
                "overlay.activation_count",
                "shell.last_action",
            ]
        );
        assert_eq!(
            ShellAction::ALL.map(ShellAction::as_str),
            ["overlay.toggle", "overlay.close", "overlay.activate",]
        );
        for key in StateBindingKey::ALL {
            assert_eq!(key.as_str().parse::<StateBindingKey>(), Ok(key));
        }
        assert!("unknown.key".parse::<StateBindingKey>().is_err());
        for action in ShellAction::ALL {
            assert_eq!(action.as_str().parse::<ShellAction>(), Ok(action));
        }
        assert!("unknown.action".parse::<ShellAction>().is_err());
    }

    #[test]
    fn valid_declarations_are_typed_and_indexed_once() {
        let index = discover(
            r#"<span id="status" data-htm-element="state-text" data-htm-bind="overlay.status"></span>
               <button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle"><span>Toggle</span></button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index.summary(),
            BuiltInElementSummary {
                registered_elements: 2,
                bindings: 1,
                actions: 1,
                discovery_scans: 1,
            }
        );
        assert_eq!(
            index.element("status").unwrap().binding,
            Some(StateBindingKey::OverlayStatus)
        );
        assert_eq!(
            index.element("toggle").unwrap().action,
            Some(ShellAction::OverlayToggle)
        );
    }

    #[test]
    fn invalid_declarations_are_rejected_without_affecting_plain_html() {
        for body in [
            r#"<span data-htm-element="state-text" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="same" data-htm-element="state-text" data-htm-bind="overlay.status"></span><span id="same" data-htm-element="state-text" data-htm-bind="output.label"></span>"#,
            r#"<div id="same"></div><span id="same" data-htm-element="state-text" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="x" data-htm-element="unknown" data-htm-bind="overlay.status"></span>"#,
            r#"<span id="x" data-htm-element="action-button" data-htm-action="overlay.toggle"></span>"#,
            r#"<span id="x" data-htm-element="state-text"></span>"#,
            r#"<button id="x" data-htm-element="action-button"></button>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="unknown.key"></span>"#,
            r#"<button id="x" data-htm-element="action-button" data-htm-action="unknown.action"></button>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="overlay.status" data-htm-action="overlay.close"></span>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="overlay.status"><b>nested</b></span>"#,
        ] {
            assert!(discover(body, BuiltInSurfaceKind::Panel).is_err(), "{body}");
        }
        let plain = discover(
            r#"<div data-example="allowed">ordinary</div>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert!(plain.is_empty());
    }

    #[test]
    fn action_sources_are_validated() {
        assert!(discover(
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_ok());
        assert!(discover(
            r#"<button id="close" data-htm-element="action-button" data-htm-action="overlay.close"></button>"#,
            BuiltInSurfaceKind::Overlay,
        ).is_ok());
        assert!(discover(
            r#"<button id="activate" data-htm-element="action-button" data-htm-action="overlay.activate"></button>"#,
            BuiltInSurfaceKind::Overlay,
        ).is_ok());
        assert!(discover(
            r#"<button id="close" data-htm-element="action-button" data-htm-action="overlay.close"></button>"#,
            BuiltInSurfaceKind::Panel,
        ).is_err());
        let disabled = discover(
            r#"<button id="toggle" disabled="false" data-htm-element="action-button" data-htm-action="overlay.toggle"></button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert!(disabled.element("toggle").unwrap().disabled);
    }
}
