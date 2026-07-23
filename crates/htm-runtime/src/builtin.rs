use crate::identity::{IdentityRegistry, author_slots};
use crate::{
    ClockFormat, ClockTimeZone, ExperimentalDocumentIdentity, ExperimentalNodeIdentity,
    MAX_CLOCK_DECLARATIONS_PER_DOCUMENT, RuntimeError,
};
use blitz_dom::{LocalName, local_name};
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;

const ELEMENT_ATTRIBUTE: &str = "data-htm-element";
const BIND_ATTRIBUTE: &str = "data-htm-bind";
const ACTION_ATTRIBUTE: &str = "data-htm-action";
const TARGET_ATTRIBUTE: &str = "data-htm-target";
const FORMAT_ATTRIBUTE: &str = "data-htm-format";
const TIME_ZONE_ATTRIBUTE: &str = "data-htm-time-zone";
const ENABLED_ATTRIBUTE: &str = "data-htm-enabled";
pub(crate) const DATETIME_ATTRIBUTE: &str = "datetime";
pub(crate) const STATE_ATTRIBUTE: &str = "data-htm-state";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuiltInElementKind {
    StateText,
    ActionButton,
    StateToken,
    ClockText,
}

impl BuiltInElementKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateText => "state-text",
            Self::ActionButton => "action-button",
            Self::StateToken => "state-token",
            Self::ClockText => "clock-text",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "state-text" => Some(Self::StateText),
            "action-button" => Some(Self::ActionButton),
            "state-token" => Some(Self::StateToken),
            "clock-text" => Some(Self::ClockText),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBindingKey {
    ClockTime,
    BatteryPercentage,
    BatteryStatus,
    BatteryWarning,
    OutputLabel,
    OutputScale,
    SurfaceTemplateId,
    SurfaceScaleProfile,
    OverlayStatus,
    OverlayActivationCount,
    ShellLastAction,
}

impl StateBindingKey {
    pub const ALL: [Self; 11] = [
        Self::ClockTime,
        Self::BatteryPercentage,
        Self::BatteryStatus,
        Self::BatteryWarning,
        Self::OutputLabel,
        Self::OutputScale,
        Self::SurfaceTemplateId,
        Self::SurfaceScaleProfile,
        Self::OverlayStatus,
        Self::OverlayActivationCount,
        Self::ShellLastAction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClockTime => "clock.time",
            Self::BatteryPercentage => "battery.percentage",
            Self::BatteryStatus => "battery.status",
            Self::BatteryWarning => "battery.warning",
            Self::OutputLabel => "output.label",
            Self::OutputScale => "output.scale",
            Self::SurfaceTemplateId => "surface.template_id",
            Self::SurfaceScaleProfile => "surface.scale_profile",
            Self::OverlayStatus => "overlay.status",
            Self::OverlayActivationCount => "overlay.activation_count",
            Self::ShellLastAction => "shell.last_action",
        }
    }

    pub const fn scope(self) -> StateBindingScope {
        match self {
            Self::ClockTime
            | Self::BatteryPercentage
            | Self::BatteryStatus
            | Self::BatteryWarning => StateBindingScope::Process,
            Self::OutputLabel
            | Self::OutputScale
            | Self::OverlayStatus
            | Self::OverlayActivationCount
            | Self::ShellLastAction => StateBindingScope::Output,
            Self::SurfaceTemplateId | Self::SurfaceScaleProfile => StateBindingScope::Surface,
        }
    }

    pub const fn supports(self, kind: StateValueKind) -> bool {
        matches!(
            (self, kind),
            (
                Self::OverlayStatus | Self::BatteryStatus,
                StateValueKind::Text | StateValueKind::Token
            ) | (
                Self::SurfaceScaleProfile | Self::BatteryWarning,
                StateValueKind::Token
            ) | (
                Self::ClockTime
                    | Self::BatteryPercentage
                    | Self::OutputLabel
                    | Self::OutputScale
                    | Self::SurfaceTemplateId
                    | Self::OverlayActivationCount
                    | Self::ShellLastAction,
                StateValueKind::Text,
            )
        )
    }
}

impl std::str::FromStr for StateBindingKey {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "clock.time" => Ok(Self::ClockTime),
            "battery.percentage" => Ok(Self::BatteryPercentage),
            "battery.status" => Ok(Self::BatteryStatus),
            "battery.warning" => Ok(Self::BatteryWarning),
            "output.label" => Ok(Self::OutputLabel),
            "output.scale" => Ok(Self::OutputScale),
            "surface.template_id" => Ok(Self::SurfaceTemplateId),
            "surface.scale_profile" => Ok(Self::SurfaceScaleProfile),
            "overlay.status" => Ok(Self::OverlayStatus),
            "overlay.activation_count" => Ok(Self::OverlayActivationCount),
            "shell.last_action" => Ok(Self::ShellLastAction),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBindingScope {
    Process,
    Output,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateValueKind {
    Text,
    Token,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateToken {
    Open,
    Closed,
    Scale1,
    Fractional,
    Unavailable,
    Absent,
    Unknown,
    Charging,
    Discharging,
    Empty,
    Full,
    PendingCharge,
    PendingDischarge,
    None,
    Low,
    Critical,
    Action,
    Enabled,
    Disabled,
}

impl StateToken {
    pub const ALL: [Self; 19] = [
        Self::Open,
        Self::Closed,
        Self::Scale1,
        Self::Fractional,
        Self::Unavailable,
        Self::Absent,
        Self::Unknown,
        Self::Charging,
        Self::Discharging,
        Self::Empty,
        Self::Full,
        Self::PendingCharge,
        Self::PendingDischarge,
        Self::None,
        Self::Low,
        Self::Critical,
        Self::Action,
        Self::Enabled,
        Self::Disabled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Scale1 => "scale-1",
            Self::Fractional => "fractional",
            Self::Unavailable => "unavailable",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Empty => "empty",
            Self::Full => "full",
            Self::PendingCharge => "pending-charge",
            Self::PendingDischarge => "pending-discharge",
            Self::None => "none",
            Self::Low => "low",
            Self::Critical => "critical",
            Self::Action => "action",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }

    pub const fn valid_for(self, key: StateBindingKey) -> bool {
        matches!(
            (key, self),
            (StateBindingKey::OverlayStatus, Self::Open | Self::Closed)
                | (
                    StateBindingKey::SurfaceScaleProfile,
                    Self::Scale1 | Self::Fractional
                )
                | (
                    StateBindingKey::BatteryStatus,
                    Self::Unavailable
                        | Self::Absent
                        | Self::Unknown
                        | Self::Charging
                        | Self::Discharging
                        | Self::Empty
                        | Self::Full
                        | Self::PendingCharge
                        | Self::PendingDischarge
                )
                | (
                    StateBindingKey::BatteryWarning,
                    Self::Unknown
                        | Self::None
                        | Self::Discharging
                        | Self::Low
                        | Self::Critical
                        | Self::Action
                )
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellAction {
    OverlayToggle,
    OverlayClose,
    OverlayActivate,
    ClockEnable,
    ClockDisable,
    ClockToggle,
}

impl ShellAction {
    pub const ALL: [Self; 6] = [
        Self::OverlayToggle,
        Self::OverlayClose,
        Self::OverlayActivate,
        Self::ClockEnable,
        Self::ClockDisable,
        Self::ClockToggle,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OverlayToggle => "overlay.toggle",
            Self::OverlayClose => "overlay.close",
            Self::OverlayActivate => "overlay.activate",
            Self::ClockEnable => "clock.enable",
            Self::ClockDisable => "clock.disable",
            Self::ClockToggle => "clock.toggle",
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
            "clock.enable" => Ok(Self::ClockEnable),
            "clock.disable" => Ok(Self::ClockDisable),
            "clock.toggle" => Ok(Self::ClockToggle),
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
                    Self::Panel | Self::Overlay,
                    ShellAction::ClockEnable | ShellAction::ClockDisable | ShellAction::ClockToggle
                )
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
    pub binding_kind: Option<StateValueKind>,
    pub action: Option<ShellAction>,
    pub action_target: Option<ElementInstanceId>,
    pub clock: Option<ClockDeclaration>,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockDeclaration {
    pub id: ElementInstanceId,
    pub format: ClockFormat,
    pub time_zone: ClockTimeZone,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuiltInElementSummary {
    pub registered_elements: usize,
    pub bindings: usize,
    pub text_bindings: usize,
    pub token_bindings: usize,
    pub actions: usize,
    pub clock_declarations: usize,
    pub discovery_scans: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BindingUpdate {
    pub changed_keys: usize,
    pub changed_elements: usize,
    pub changed_text_elements: usize,
    pub changed_token_elements: usize,
    pub suppressed_keys: usize,
}

#[derive(Debug, Clone, Copy)]
struct BuiltInElementDefinition {
    name: &'static str,
    allowed_tags: &'static [&'static str],
    required_attribute: &'static str,
}

const DEFINITIONS: [BuiltInElementDefinition; 4] = [
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
    BuiltInElementDefinition {
        name: "state-token",
        allowed_tags: &["div", "span", "section"],
        required_attribute: BIND_ATTRIBUTE,
    },
    BuiltInElementDefinition {
        name: "clock-text",
        allowed_tags: &["time"],
        required_attribute: FORMAT_ATTRIBUTE,
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
    pub(crate) target: Option<ElementInstanceId>,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltInElementIndex {
    elements: BTreeMap<String, IndexedElement>,
    text_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    token_bindings: BTreeMap<StateBindingKey, Vec<String>>,
    actions: Vec<String>,
    applied_values: BTreeMap<(StateBindingKey, StateValueKind), String>,
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
        let mut text_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut token_bindings: BTreeMap<StateBindingKey, Vec<String>> = BTreeMap::new();
        let mut actions = Vec::new();
        let mut unresolved_action_targets = BTreeMap::new();
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
            if kind == BuiltInElementKind::ClockText
                && (element.has_attr(LocalName::from(DATETIME_ATTRIBUTE))
                    || element.has_attr(LocalName::from(STATE_ATTRIBUTE)))
            {
                return Err(invalid_declaration(
                    &context,
                    "`datetime` and `data-htm-state` are runtime-owned",
                ));
            }
            let (binding, binding_kind, action, clock) = match kind {
                BuiltInElementKind::StateText => {
                    validate_state_text_children(document, slot, &context)?;
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    if !binding.supports(StateValueKind::Text) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "state binding `{required}` does not support text presentation"
                            ),
                        ));
                    }
                    (Some(binding), Some(StateValueKind::Text), None, None)
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
                    let clock_action = matches!(
                        action,
                        ShellAction::ClockEnable
                            | ShellAction::ClockDisable
                            | ShellAction::ClockToggle
                    );
                    match (
                        clock_action,
                        element.attr(LocalName::from(TARGET_ATTRIBUTE)),
                    ) {
                        (true, Some(target)) if !target.is_empty() => {
                            unresolved_action_targets.insert(html_id.clone(), target.to_owned());
                        }
                        (true, _) => {
                            return Err(invalid_declaration(
                                &context,
                                "clock action requires nonempty `data-htm-target`",
                            ));
                        }
                        (false, Some(_)) => {
                            return Err(invalid_declaration(
                                &context,
                                "`data-htm-target` is forbidden for overlay actions",
                            ));
                        }
                        (false, None) => {}
                    }
                    (None, None, Some(action), None)
                }
                BuiltInElementKind::StateToken => {
                    let binding = required.parse::<StateBindingKey>().map_err(|()| {
                        invalid_declaration(
                            &context,
                            format!("unsupported state binding `{required}`"),
                        )
                    })?;
                    if !binding.supports(StateValueKind::Token) {
                        return Err(invalid_declaration(
                            &context,
                            format!(
                                "state binding `{required}` does not support token presentation"
                            ),
                        ));
                    }
                    (Some(binding), Some(StateValueKind::Token), None, None)
                }
                BuiltInElementKind::ClockText => {
                    validate_state_text_children(document, slot, &context)?;
                    let format = ClockFormat::compile(required).map_err(|error| {
                        invalid_declaration(
                            &context,
                            format!("invalid `{FORMAT_ATTRIBUTE}`: {error}"),
                        )
                    })?;
                    let time_zone =
                        ClockTimeZone::parse(element.attr(LocalName::from(TIME_ZONE_ATTRIBUTE)))
                            .map_err(|error| {
                                invalid_declaration(
                                    &context,
                                    format!("invalid `{TIME_ZONE_ATTRIBUTE}`: {error}"),
                                )
                            })?;
                    let enabled = match element.attr(LocalName::from(ENABLED_ATTRIBUTE)) {
                        None | Some("true") => true,
                        Some("false") => false,
                        Some(value) => {
                            return Err(invalid_declaration(
                                &context,
                                format!(
                                    "`{ENABLED_ATTRIBUTE}` must be `true` or `false`, not `{value}`"
                                ),
                            ));
                        }
                    };
                    (
                        None,
                        None,
                        None,
                        Some(ClockDeclaration {
                            id: ElementInstanceId {
                                document_generation,
                                html_id: html_id.clone(),
                            },
                            format,
                            time_zone,
                            enabled,
                        }),
                    )
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
                binding_kind,
                action,
                action_target: None,
                clock,
                disabled: element.has_attr(local_name!("disabled")),
            };
            let indexed = IndexedElement {
                declaration,
                node: identities.identity_for_slot(document, slot)?,
                depth: node_depth(document, slot),
                order,
            };
            if let Some(binding) = binding {
                match binding_kind {
                    Some(StateValueKind::Text) => {
                        text_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    Some(StateValueKind::Token) => {
                        token_bindings
                            .entry(binding)
                            .or_default()
                            .push(html_id.clone());
                    }
                    None => {
                        return Err(invalid_declaration(
                            &context,
                            "state binding has no presentation kind",
                        ));
                    }
                }
            }
            if action.is_some() {
                actions.push(html_id.clone());
            }
            elements.insert(html_id, indexed);
        }

        let clock_declarations = elements
            .values()
            .filter(|element| element.declaration.clock.is_some())
            .count();
        if clock_declarations > MAX_CLOCK_DECLARATIONS_PER_DOCUMENT {
            return Err(RuntimeError::LimitExceeded(format!(
                "{source} contains {clock_declarations} clock declarations; the per-document limit is {MAX_CLOCK_DECLARATIONS_PER_DOCUMENT}"
            )));
        }
        for (action_id, target_id) in unresolved_action_targets {
            let target = elements.get(&target_id).ok_or_else(|| {
                invalid_declaration(
                    &declaration_context(source, Some(&action_id)),
                    format!("clock target `#{target_id}` does not exist"),
                )
            })?;
            if target.declaration.kind != BuiltInElementKind::ClockText {
                return Err(invalid_declaration(
                    &declaration_context(source, Some(&action_id)),
                    format!("clock target `#{target_id}` is not `clock-text`"),
                ));
            }
            let target_identity = target.declaration.id.clone();
            elements
                .get_mut(&action_id)
                .expect("action was indexed above")
                .declaration
                .action_target = Some(target_identity);
        }

        actions.sort_by_key(|id| {
            let element = &elements[id];
            (
                std::cmp::Reverse(element.depth),
                std::cmp::Reverse(element.order),
            )
        });
        for ids in text_bindings.values_mut() {
            ids.sort();
        }
        for ids in token_bindings.values_mut() {
            ids.sort();
        }
        let text_binding_count = text_bindings.values().map(Vec::len).sum();
        let token_binding_count = token_bindings.values().map(Vec::len).sum();
        let summary = BuiltInElementSummary {
            registered_elements: elements.len(),
            bindings: text_binding_count + token_binding_count,
            text_bindings: text_binding_count,
            token_bindings: token_binding_count,
            actions: actions.len(),
            clock_declarations,
            discovery_scans: 1,
        };
        Ok(Self {
            elements,
            text_bindings,
            token_bindings,
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

    pub(crate) fn clock_declarations(&self) -> Vec<ClockDeclaration> {
        self.elements
            .values()
            .filter_map(|element| element.declaration.clock.clone())
            .collect()
    }

    pub(crate) fn clock_declaration(
        &self,
        identity: &ElementInstanceId,
    ) -> Option<&ClockDeclaration> {
        self.elements
            .get(&identity.html_id)
            .filter(|element| element.declaration.id == *identity)
            .and_then(|element| element.declaration.clock.as_ref())
    }

    pub(crate) fn set_clock_enabled(
        &mut self,
        identity: &ElementInstanceId,
        enabled: bool,
    ) -> Result<bool, RuntimeError> {
        let element = self.elements.get_mut(&identity.html_id).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "clock target `#{}` disappeared",
                identity.html_id
            ))
        })?;
        if element.declaration.id != *identity {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "clock target `#{}` belongs to a stale document generation",
                identity.html_id
            )));
        }
        let clock = element.declaration.clock.as_mut().ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "target `#{}` is not `clock-text`",
                identity.html_id
            ))
        })?;
        if clock.enabled == enabled {
            return Ok(false);
        }
        clock.enabled = enabled;
        Ok(true)
    }

    pub(crate) fn element(&self, html_id: &str) -> Option<&ElementDeclaration> {
        self.elements.get(html_id).map(|entry| &entry.declaration)
    }

    pub(crate) fn binding_targets(&self, key: StateBindingKey, kind: StateValueKind) -> &[String] {
        match kind {
            StateValueKind::Text => &self.text_bindings,
            StateValueKind::Token => &self.token_bindings,
        }
        .get(&key)
        .map(Vec::as_slice)
        .unwrap_or(&[])
    }

    pub(crate) fn binding_is_unchanged(
        &self,
        key: StateBindingKey,
        kind: StateValueKind,
        value: &str,
    ) -> bool {
        self.applied_values
            .get(&(key, kind))
            .is_some_and(|old| old == value)
    }

    pub(crate) fn record_binding(
        &mut self,
        key: StateBindingKey,
        kind: StateValueKind,
        value: String,
    ) {
        self.applied_values.insert((key, kind), value);
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
        if let Some(target) = &entry.declaration.action_target {
            if target.document_generation != entry.declaration.id.document_generation {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` belongs to a stale document generation",
                    target.html_id
                )));
            }
            let target_entry = self.elements.get(&target.html_id).ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` disappeared",
                    target.html_id
                ))
            })?;
            if target_entry.declaration.kind != BuiltInElementKind::ClockText
                || target_entry.declaration.id != *target
            {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "clock target `#{}` is stale or has the wrong kind",
                    target.html_id
                )));
            }
            identities.resolve(document, target_entry.node)?;
        }
        Ok(Some(ActionTarget {
            id: entry.declaration.id.clone(),
            action,
            node: entry.node,
            target: entry.declaration.action_target.clone(),
        }))
    }
}

pub fn built_in_registry_names() -> &'static [&'static str] {
    &["state-text", "action-button", "state-token", "clock-text"]
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
        BuiltInElementKind::StateToken => &DEFINITIONS[2],
        BuiltInElementKind::ClockText => &DEFINITIONS[3],
    }
}

fn allowed_behavior_attributes(kind: BuiltInElementKind) -> &'static [&'static str] {
    match kind {
        BuiltInElementKind::StateText => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE],
        BuiltInElementKind::ActionButton => {
            &[ELEMENT_ATTRIBUTE, ACTION_ATTRIBUTE, TARGET_ATTRIBUTE]
        }
        BuiltInElementKind::StateToken => &[ELEMENT_ATTRIBUTE, BIND_ATTRIBUTE],
        BuiltInElementKind::ClockText => &[
            ELEMENT_ATTRIBUTE,
            FORMAT_ATTRIBUTE,
            TIME_ZONE_ATTRIBUTE,
            ENABLED_ATTRIBUTE,
        ],
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
        assert_eq!(
            built_in_registry_names(),
            &["state-text", "action-button", "state-token", "clock-text"]
        );
        assert!(validate_definitions(&DEFINITIONS).is_ok());
        let duplicate = [DEFINITIONS[0], DEFINITIONS[0]];
        assert!(validate_definitions(&duplicate).is_err());
        assert_eq!(
            StateBindingKey::ALL.map(StateBindingKey::as_str),
            [
                "clock.time",
                "battery.percentage",
                "battery.status",
                "battery.warning",
                "output.label",
                "output.scale",
                "surface.template_id",
                "surface.scale_profile",
                "overlay.status",
                "overlay.activation_count",
                "shell.last_action",
            ]
        );
        assert_eq!(
            ShellAction::ALL.map(ShellAction::as_str),
            [
                "overlay.toggle",
                "overlay.close",
                "overlay.activate",
                "clock.enable",
                "clock.disable",
                "clock.toggle",
            ]
        );
        for key in StateBindingKey::ALL {
            assert_eq!(key.as_str().parse::<StateBindingKey>(), Ok(key));
        }
        assert_eq!(
            StateBindingKey::ClockTime.scope(),
            StateBindingScope::Process
        );
        for key in [
            StateBindingKey::BatteryPercentage,
            StateBindingKey::BatteryStatus,
            StateBindingKey::BatteryWarning,
        ] {
            assert_eq!(key.scope(), StateBindingScope::Process);
        }
        assert_eq!(
            StateBindingKey::OutputLabel.scope(),
            StateBindingScope::Output
        );
        assert_eq!(
            StateBindingKey::SurfaceTemplateId.scope(),
            StateBindingScope::Surface
        );
        assert_eq!(
            StateBindingKey::SurfaceScaleProfile.scope(),
            StateBindingScope::Surface
        );
        assert!(StateBindingKey::OverlayStatus.supports(StateValueKind::Text));
        assert!(StateBindingKey::OverlayStatus.supports(StateValueKind::Token));
        assert!(StateBindingKey::BatteryPercentage.supports(StateValueKind::Text));
        assert!(!StateBindingKey::BatteryPercentage.supports(StateValueKind::Token));
        assert!(StateBindingKey::BatteryStatus.supports(StateValueKind::Text));
        assert!(StateBindingKey::BatteryStatus.supports(StateValueKind::Token));
        assert!(!StateBindingKey::BatteryWarning.supports(StateValueKind::Text));
        assert!(StateBindingKey::BatteryWarning.supports(StateValueKind::Token));
        assert!(StateBindingKey::SurfaceScaleProfile.supports(StateValueKind::Token));
        assert!(!StateBindingKey::SurfaceScaleProfile.supports(StateValueKind::Text));
        assert!(!StateBindingKey::ClockTime.supports(StateValueKind::Token));
        assert_eq!(
            StateToken::ALL.map(StateToken::as_str),
            [
                "open",
                "closed",
                "scale-1",
                "fractional",
                "unavailable",
                "absent",
                "unknown",
                "charging",
                "discharging",
                "empty",
                "full",
                "pending-charge",
                "pending-discharge",
                "none",
                "low",
                "critical",
                "action",
                "enabled",
                "disabled",
            ]
        );
        assert!(StateToken::Open.valid_for(StateBindingKey::OverlayStatus));
        assert!(!StateToken::Open.valid_for(StateBindingKey::SurfaceScaleProfile));
        for token in [
            StateToken::Unavailable,
            StateToken::Absent,
            StateToken::Unknown,
            StateToken::Charging,
            StateToken::Discharging,
            StateToken::Empty,
            StateToken::Full,
            StateToken::PendingCharge,
            StateToken::PendingDischarge,
        ] {
            assert!(token.valid_for(StateBindingKey::BatteryStatus));
        }
        for token in [
            StateToken::Unknown,
            StateToken::None,
            StateToken::Discharging,
            StateToken::Low,
            StateToken::Critical,
            StateToken::Action,
        ] {
            assert!(token.valid_for(StateBindingKey::BatteryWarning));
        }
        assert!(!StateToken::Low.valid_for(StateBindingKey::BatteryStatus));
        assert!(!StateToken::Charging.valid_for(StateBindingKey::BatteryWarning));
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
                text_bindings: 1,
                token_bindings: 0,
                actions: 1,
                clock_declarations: 0,
                discovery_scans: 1,
            }
        );
        assert_eq!(
            index.element("status").unwrap().binding,
            Some(StateBindingKey::OverlayStatus)
        );
        assert_eq!(
            index.element("status").unwrap().binding_kind,
            Some(StateValueKind::Text)
        );
        assert_eq!(
            index.element("toggle").unwrap().action,
            Some(ShellAction::OverlayToggle)
        );
    }

    #[test]
    fn process_clock_bindings_share_the_existing_state_text_kind() {
        let index = discover(
            r#"<span id="clock-a" data-htm-element="state-text" data-htm-bind="clock.time"></span>
               <output id="clock-b" data-htm-element="state-text" data-htm-bind="clock.time"></output>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index
                .binding_targets(StateBindingKey::ClockTime, StateValueKind::Text)
                .len(),
            2
        );
        assert_eq!(
            index.element("clock-a").unwrap().kind,
            BuiltInElementKind::StateText
        );
        assert_eq!(built_in_registry_names().len(), 4);
    }

    #[test]
    fn clock_text_declarations_are_typed_once_and_targets_are_generation_safe() {
        let index = discover(
            r#"<time id="local" class="clock" aria-label="Local time"
                    data-htm-element="clock-text" data-htm-format="%H:%M"></time>
               <time id="tokyo" data-htm-element="clock-text"
                    data-htm-format="%I:%M:%S %p" data-htm-time-zone="Asia/Tokyo"
                    data-htm-enabled="false"></time>
               <button id="enable" data-htm-element="action-button"
                    data-htm-action="clock.enable" data-htm-target="tokyo">Enable</button>
               <button id="disable" data-htm-element="action-button"
                    data-htm-action="clock.disable" data-htm-target="tokyo">Disable</button>
               <button id="toggle" data-htm-element="action-button"
                    data-htm-action="clock.toggle" data-htm-target="tokyo">Toggle</button>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        let declarations = index.clock_declarations();
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].format.source(), "%H:%M");
        assert_eq!(declarations[0].time_zone, ClockTimeZone::Local);
        assert!(declarations[0].enabled);
        assert_eq!(declarations[1].time_zone.declaration_value(), "Asia/Tokyo");
        assert!(!declarations[1].enabled);
        assert_eq!(index.summary().clock_declarations, 2);
        assert_eq!(index.summary().discovery_scans, 1);
        for id in ["enable", "disable", "toggle"] {
            let action = index.element(id).unwrap();
            assert_eq!(
                action.action_target.as_ref().unwrap().document_generation,
                ExperimentalDocumentIdentity { serial: 9 }
            );
            assert_eq!(action.action_target.as_ref().unwrap().html_id, "tokyo");
        }
    }

    #[test]
    fn invalid_clock_declarations_and_targets_are_rejected_atomically() {
        for body in [
            r#"<span id="clock" data-htm-element="clock-text" data-htm-format="%H"></span>"#,
            r#"<time data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format=""></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%s"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-enabled="TRUE"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-enabled=""></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-time-zone="../UTC"></time>"#,
            r#"<time id="clock" datetime="2026-01-01T00:00:00Z" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-state="enabled" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-bind="clock.time"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-action="clock.toggle"></time>"#,
            r#"<time id="clock" data-htm-element="clock-text" data-htm-format="%H" data-htm-target="other"></time>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle"></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target=""></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target="missing"></button>"#,
            r#"<span id="target" data-htm-element="state-text" data-htm-bind="clock.time"></span><button id="toggle" data-htm-element="action-button" data-htm-action="clock.toggle" data-htm-target="target"></button>"#,
            r#"<button id="toggle" data-htm-element="action-button" data-htm-action="overlay.toggle" data-htm-target="clock"></button><time id="clock" data-htm-element="clock-text" data-htm-format="%H"></time>"#,
        ] {
            assert!(discover(body, BuiltInSurfaceKind::Panel).is_err(), "{body}");
        }
        let excessive: String = (0..=MAX_CLOCK_DECLARATIONS_PER_DOCUMENT)
            .map(|index| {
                format!(
                    r#"<time id="clock-{index}" data-htm-element="clock-text" data-htm-format="%H"></time>"#
                )
            })
            .collect();
        assert!(discover(&excessive, BuiltInSurfaceKind::Panel).is_err());
    }

    #[test]
    fn state_tokens_are_typed_indexed_and_limited_to_visual_wrappers() {
        let index = discover(
            r#"<span id="status" class="indicator" data-extra="kept"
                    data-htm-element="state-token" data-htm-bind="overlay.status"></span>
               <section id="scale" data-htm-element="state-token"
                    data-htm-bind="surface.scale_profile"></section>"#,
            BuiltInSurfaceKind::Panel,
        )
        .unwrap();
        assert_eq!(
            index.binding_targets(StateBindingKey::OverlayStatus, StateValueKind::Token),
            &["status"]
        );
        assert_eq!(
            index.element("status").unwrap().binding_kind,
            Some(StateValueKind::Token)
        );
        assert_eq!(index.summary().text_bindings, 0);
        assert_eq!(index.summary().token_bindings, 2);
        for tag in ["div", "span", "section"] {
            assert!(
                discover(
                    &format!(
                        r#"<{tag} id="token" data-htm-element="state-token" data-htm-bind="overlay.status"></{tag}>"#
                    ),
                    BuiltInSurfaceKind::Panel,
                )
                .is_ok()
            );
        }
        for tag in ["button", "img", "svg", "input"] {
            assert!(
                discover(
                    &format!(
                        r#"<{tag} id="token" data-htm-element="state-token" data-htm-bind="overlay.status"></{tag}>"#
                    ),
                    BuiltInSurfaceKind::Panel,
                )
                .is_err()
            );
        }
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
            r#"<span id="x" data-htm-element="state-token"></span>"#,
            r#"<span id="x" data-htm-element="state-token" data-htm-bind="clock.time"></span>"#,
            r#"<span id="x" data-htm-element="state-text" data-htm-bind="surface.scale_profile"></span>"#,
            r#"<span id="x" data-htm-element="state-token" data-htm-bind="overlay.status" data-htm-state="open"></span>"#,
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
