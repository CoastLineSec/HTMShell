use crate::package::{
    PackageAlias, PackageErrorKind, PackageId, PackageLoadError, PackageSchemaSource,
    PackageSnapshotGeneration, ResolvedPackage,
};
use crate::{NumericValue, StateToken, StateValueFormat};
use blitz_dom::node::NodeData;
use blitz_dom::{Attribute, DocumentConfig, LocalName, QualName, ns};
use blitz_html::HtmlDocument;
use cssparser::{Parser, ParserInput, Token};
use selectors::matching::QuirksMode;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use style_traits::ParsingMode;
use stylo::parser::{Parse, ParserContext};
use stylo::stylesheets::{CssRuleType, Origin, UrlExtraData};
use stylo::values::specified;
use url::Url;

pub const MAX_COMPONENT_NAME_BYTES: usize = 64;
pub const MAX_COMPONENT_EXPORTS_PER_PACKAGE: usize = 256;
pub const MAX_COMPONENT_EXPORTS_PER_GRAPH: usize = 4_096;
pub const MAX_COMPONENT_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_COMPONENT_SOURCE_NODES: usize = 10_000;
pub const MAX_COMPONENT_INSTANCES_PER_DOCUMENT: usize = 4_096;
pub const MAX_COMPONENT_REFERENCES_PER_DOCUMENT: usize = 256;
pub const MAX_COMPONENT_NESTING_DEPTH: usize = 32;
pub const MAX_COMPONENT_EXPANDED_NODES: usize = 50_000;
pub const MAX_COMPONENT_INPUTS: usize = 64;
pub const MAX_COMPONENT_INPUT_NAME_BYTES: usize = 64;
pub const MAX_COMPONENT_INPUT_STRING_BYTES: usize = 4_096;
pub const MAX_COMPONENT_INPUT_LITERAL_BYTES: usize = 16 * 1_024;
pub const MAX_COMPONENT_INPUT_ATTRIBUTES: usize = 64;

const COMPONENT_ATTRIBUTE: &str = "data-htm-component";
const BUILTIN_ATTRIBUTE: &str = "data-htm-element";
const USE_ELEMENT: &str = "htm-use";
const TEMPLATE_ELEMENT: &str = "template";
const BIND_ATTRIBUTE: &str = "data-htm-bind";
const FORMAT_ATTRIBUTE: &str = "data-htm-format";
const STATE_ATTRIBUTE: &str = "data-htm-state";

const RESERVED_COMPONENT_INPUT_NAMES: &[&str] = &[
    "component",
    "slot",
    "id",
    "class",
    "style",
    "input",
    "state",
    "action",
    "service",
    "resource",
    "repeat",
    "surface",
    "host",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ComponentInputName(String);

impl ComponentInputName {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        if value.is_empty() || value.len() > MAX_COMPONENT_INPUT_NAME_BYTES || !value.is_ascii() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentInputName,
                format!(
                    "component input name must contain 1..={MAX_COMPONENT_INPUT_NAME_BYTES} ASCII bytes"
                ),
            ));
        }
        let bytes = value.as_bytes();
        if !bytes[0].is_ascii_lowercase()
            || bytes.last() == Some(&b'-')
            || value.contains("--")
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentInputName,
                format!(
                    "component input name `{value}` must start with a lowercase letter and contain only lowercase letters, digits, and single interior hyphens"
                ),
            ));
        }
        if RESERVED_COMPONENT_INPUT_NAMES.contains(&value) {
            return Err(PackageLoadError::new(
                PackageErrorKind::ReservedComponentInputName,
                format!("component input name `{value}` is reserved"),
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentInputName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentInputType {
    String,
    Number,
    Boolean,
    Token,
    Color,
    Length,
}

impl ComponentInputType {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        match value {
            "string" => Ok(Self::String),
            "number" => Ok(Self::Number),
            "boolean" => Ok(Self::Boolean),
            "token" => Ok(Self::Token),
            "color" => Ok(Self::Color),
            "length" => Ok(Self::Length),
            "state-reference" => Err(PackageLoadError::new(
                PackageErrorKind::ComponentStateReferenceInputNotSupported,
                "state-reference component inputs are not supported",
            )),
            "action-reference" => Err(PackageLoadError::new(
                PackageErrorKind::ComponentActionReferenceInputNotSupported,
                "action-reference component inputs are not supported",
            )),
            "resource-reference" => Err(PackageLoadError::new(
                PackageErrorKind::ComponentResourceReferenceInputNotSupported,
                "resource-reference component inputs are not supported",
            )),
            _ => Err(PackageLoadError::new(
                PackageErrorKind::UnsupportedComponentInputType,
                format!("unsupported component input type `{value}`"),
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Token => "token",
            Self::Color => "color",
            Self::Length => "length",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentNumber(u64);

impl ComponentNumber {
    fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > crate::MAX_RANGE_NUMBER_BYTES {
            return Err(format!(
                "number must contain 1..={} bytes",
                crate::MAX_RANGE_NUMBER_BYTES
            ));
        }
        let mut parsed = value
            .parse::<f64>()
            .map_err(|_| "number must be a complete decimal literal".to_owned())?;
        if !parsed.is_finite() {
            return Err("number must be finite".to_owned());
        }
        if parsed == 0.0 {
            parsed = 0.0;
        }
        Ok(Self(parsed.to_bits()))
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }

    fn canonical(self) -> String {
        NumericValue::finite_decimal(self.get())
            .format(StateValueFormat::Raw)
            .expect("component numbers are finite")
            .display
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentColor([u8; 4]);

impl ComponentColor {
    pub fn rgba(self) -> [u8; 4] {
        self.0
    }

    fn canonical(self) -> String {
        let [red, green, blue, alpha] = self.0;
        if alpha == u8::MAX {
            format!("#{red:02x}{green:02x}{blue:02x}")
        } else {
            format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentLength(u32);

impl ComponentLength {
    pub fn logical_px(self) -> f32 {
        f32::from_bits(self.0)
    }

    fn canonical(self) -> String {
        if self.logical_px() == 0.0 {
            "0px".to_owned()
        } else {
            NumericValue::finite_decimal(self.logical_px() as f64)
                .format(StateValueFormat::Raw)
                .expect("component lengths are finite")
                .display
                + "px"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentInputValue {
    String(String),
    Number(ComponentNumber),
    Boolean(bool),
    Token(StateToken),
    Color(ComponentColor),
    Length(ComponentLength),
}

impl ComponentInputValue {
    pub const fn input_type(&self) -> ComponentInputType {
        match self {
            Self::String(_) => ComponentInputType::String,
            Self::Number(_) => ComponentInputType::Number,
            Self::Boolean(_) => ComponentInputType::Boolean,
            Self::Token(_) => ComponentInputType::Token,
            Self::Color(_) => ComponentInputType::Color,
            Self::Length(_) => ComponentInputType::Length,
        }
    }

    pub fn canonical_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Number(value) => value.canonical(),
            Self::Boolean(value) => value.to_string(),
            Self::Token(value) => value.as_str().to_owned(),
            Self::Color(value) => value.canonical(),
            Self::Length(value) => value.canonical(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInputDeclaration {
    name: ComponentInputName,
    input_type: ComponentInputType,
    required: bool,
    default: Option<ComponentInputValue>,
}

impl ComponentInputDeclaration {
    pub(crate) fn new(
        name: ComponentInputName,
        input_type: ComponentInputType,
        required: bool,
        default: Option<ComponentInputValue>,
    ) -> Self {
        Self {
            name,
            input_type,
            required,
            default,
        }
    }

    pub fn name(&self) -> &ComponentInputName {
        &self.name
    }

    pub fn input_type(&self) -> ComponentInputType {
        self.input_type
    }

    pub fn required(&self) -> bool {
        self.required
    }

    pub fn default(&self) -> Option<&ComponentInputValue> {
        self.default.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentInputProvenance {
    Supplied,
    Defaulted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentInput {
    declaration: ComponentInputDeclaration,
    value: ComponentInputValue,
    provenance: ComponentInputProvenance,
}

impl ResolvedComponentInput {
    pub fn declaration(&self) -> &ComponentInputDeclaration {
        &self.declaration
    }

    pub fn value(&self) -> &ComponentInputValue {
        &self.value
    }

    pub fn provenance(&self) -> ComponentInputProvenance {
        self.provenance
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentInputVersion(Arc<str>);

impl ComponentInputVersion {
    pub fn deterministic_string(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComponentInputs {
    values: Arc<[ResolvedComponentInput]>,
    version: ComponentInputVersion,
}

impl ResolvedComponentInputs {
    fn new(values: Vec<ResolvedComponentInput>) -> Self {
        let version = component_input_version(&values);
        Self {
            values: values.into(),
            version,
        }
    }

    fn for_instance(&self) -> Self {
        Self::new(self.values.to_vec())
    }

    pub fn values(&self) -> &[ResolvedComponentInput] {
        &self.values
    }

    pub fn version(&self) -> &ComponentInputVersion {
        &self.version
    }

    pub fn get(&self, name: &ComponentInputName) -> Option<&ComponentInputValue> {
        self.values
            .iter()
            .find(|value| value.declaration.name() == name)
            .map(ResolvedComponentInput::value)
    }

    pub fn is_structurally_compatible_with(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .zip(other.values.iter())
                .all(|(left, right)| {
                    left.declaration.name() == right.declaration.name()
                        && left.declaration.input_type() == right.declaration.input_type()
                })
    }
}

pub(crate) fn parse_component_input_default(
    input_type: ComponentInputType,
    value: &serde_json::Value,
) -> Result<ComponentInputValue, PackageLoadError> {
    let literal = match (input_type, value) {
        (ComponentInputType::Boolean, serde_json::Value::Bool(value)) => {
            return Ok(ComponentInputValue::Boolean(*value));
        }
        (ComponentInputType::Number, serde_json::Value::Number(value)) => value.to_string(),
        (
            ComponentInputType::String
            | ComponentInputType::Token
            | ComponentInputType::Color
            | ComponentInputType::Length,
            serde_json::Value::String(value),
        ) => value.clone(),
        _ => {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentInputDefault,
                format!(
                    "component input default must be a JSON value matching `{}`",
                    input_type.as_str()
                ),
            ));
        }
    };
    parse_component_input_literal(input_type, &literal).map_err(|error| {
        PackageLoadError::new(
            PackageErrorKind::InvalidComponentInputDefault,
            error.to_string(),
        )
    })
}

fn parse_component_input_literal(
    input_type: ComponentInputType,
    value: &str,
) -> Result<ComponentInputValue, PackageLoadError> {
    if value.contains('\0') {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentInputLiteral,
            "component input literal must not contain NUL",
        ));
    }
    match input_type {
        ComponentInputType::String => {
            if value.len() > MAX_COMPONENT_INPUT_STRING_BYTES {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentInputStringLimit,
                    format!(
                        "component string input exceeds {MAX_COMPONENT_INPUT_STRING_BYTES} UTF-8 bytes"
                    ),
                ));
            }
            Ok(ComponentInputValue::String(value.to_owned()))
        }
        ComponentInputType::Number => ComponentNumber::parse(value)
            .map(ComponentInputValue::Number)
            .map_err(|message| {
                PackageLoadError::new(PackageErrorKind::InvalidComponentInputLiteral, message)
            }),
        ComponentInputType::Boolean => match value {
            "true" => Ok(ComponentInputValue::Boolean(true)),
            "false" => Ok(ComponentInputValue::Boolean(false)),
            _ => Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentInputLiteral,
                "boolean component inputs accept exactly `true` or `false`",
            )),
        },
        ComponentInputType::Token => value
            .parse::<StateToken>()
            .map(ComponentInputValue::Token)
            .map_err(|()| {
                PackageLoadError::new(
                    PackageErrorKind::InvalidComponentInputLiteral,
                    format!("`{value}` is not a supported state token"),
                )
            }),
        ComponentInputType::Color => parse_component_color(value)
            .map(ComponentInputValue::Color)
            .map_err(|message| {
                PackageLoadError::new(PackageErrorKind::InvalidComponentInputLiteral, message)
            }),
        ComponentInputType::Length => parse_component_length(value)
            .map(ComponentInputValue::Length)
            .map_err(|message| {
                PackageLoadError::new(PackageErrorKind::InvalidComponentInputLiteral, message)
            }),
    }
}

fn parse_component_color(value: &str) -> Result<ComponentColor, String> {
    let url_data: UrlExtraData = Url::parse("htm-local://package/input")
        .expect("static component input URL is valid")
        .into();
    let context = ParserContext::new(
        Origin::Author,
        &url_data,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );
    let mut parser_input = ParserInput::new(value);
    let mut parser = Parser::new(&mut parser_input);
    let parsed = parser
        .parse_entirely(|parser| specified::Color::parse(&context, parser))
        .map_err(|_| "component color is not a supported context-free CSS color".to_owned())?;
    let specified::Color::Absolute(color) = parsed else {
        return Err(
            "component color must resolve without currentColor or style context".to_owned(),
        );
    };
    let color = color.color.into_srgb_legacy();
    let [red, green, blue, alpha] = [
        color.components.0,
        color.components.1,
        color.components.2,
        color.alpha,
    ]
    .map(cssparser::color::clamp_unit_f32);
    Ok(ComponentColor([red, green, blue, alpha]))
}

fn parse_component_length(value: &str) -> Result<ComponentLength, String> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let parsed = parser
        .parse_entirely(|parser| {
            let location = parser.current_source_location();
            match parser.next()? {
                Token::Number { value, .. } if *value == 0.0 => Ok(0.0),
                Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("px") => {
                    Ok(*value)
                }
                _ => Err(location.new_custom_error::<(), ()>(())),
            }
        })
        .map_err(|_| {
            "component length accepts only finite px values or unitless zero".to_owned()
        })?;
    if !parsed.is_finite() {
        return Err("component length must be finite".to_owned());
    }
    let normalized = if parsed == 0.0 { 0.0 } else { parsed };
    Ok(ComponentLength(normalized.to_bits()))
}

fn component_input_version(values: &[ResolvedComponentInput]) -> ComponentInputVersion {
    let mut serialized = String::from("component-inputs-v1;");
    for value in values {
        let name = value.declaration.name().as_str();
        let canonical = value.value.canonical_string();
        serialized.push_str(&name.len().to_string());
        serialized.push(':');
        serialized.push_str(name);
        serialized.push(':');
        serialized.push_str(value.declaration.input_type().as_str());
        serialized.push(':');
        serialized.push_str(&canonical.len().to_string());
        serialized.push(':');
        serialized.push_str(&canonical);
        serialized.push(';');
    }
    ComponentInputVersion(serialized.into())
}

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
    inputs: Arc<[ComponentInputDeclaration]>,
}

impl ComponentExport {
    pub(crate) fn new(
        name: ComponentName,
        source: String,
        inputs: Vec<ComponentInputDeclaration>,
    ) -> Self {
        Self {
            name,
            source,
            inputs: inputs.into(),
        }
    }

    pub fn name(&self) -> &ComponentName {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn inputs(&self) -> &[ComponentInputDeclaration] {
        &self.inputs
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentInputConsumerKind {
    StateText,
    StateToken,
    StateValue,
}

impl ComponentInputConsumerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateText => "state-text",
            Self::StateToken => "state-token",
            Self::StateValue => "state-value",
        }
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
        inputs: ResolvedComponentInputs,
        source_ordinal: u32,
    },
    InputConsumer {
        name: QualName,
        attributes: Vec<Attribute>,
        children: Arc<[ComponentTemplateNode]>,
        consumer_kind: ComponentInputConsumerKind,
        input: ComponentInputName,
        value_format: StateValueFormat,
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
    inputs: Arc<[ComponentInputDeclaration]>,
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

    pub fn inputs(&self) -> &[ComponentInputDeclaration] {
        &self.inputs
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
    pub inputs: Arc<[ComponentInputDeclaration]>,
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
        supplied_inputs: Vec<(ComponentInputName, String)>,
        source_ordinal: u32,
    },
    InputConsumer {
        name: QualName,
        attributes: Vec<Attribute>,
        children: Vec<UnresolvedTemplateNode>,
        consumer_kind: ComponentInputConsumerKind,
        input: ComponentInputName,
        value_format: StateValueFormat,
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
            input_consumers: Vec::new(),
        };
        let children = instantiate_nodes(&mut document, &self.nodes, None, None, &[], &mut state)?;
        document.mutate().append_children(0, &children);
        Ok(InstantiatedDocument {
            document,
            instances: state.instances,
            descendants: state.descendants,
            input_consumers: state.input_consumers,
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
    inputs: ResolvedComponentInputs,
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

    pub fn inputs(&self) -> &ResolvedComponentInputs {
        &self.inputs
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInputConsumerRecord {
    instance_id: ComponentInstanceId,
    node_slot: usize,
    template_source_ordinal: u32,
    kind: ComponentInputConsumerKind,
    input: ComponentInputName,
}

impl ComponentInputConsumerRecord {
    pub fn instance_id(&self) -> &ComponentInstanceId {
        &self.instance_id
    }

    pub fn node_slot(&self) -> usize {
        self.node_slot
    }

    pub fn template_source_ordinal(&self) -> u32 {
        self.template_source_ordinal
    }

    pub fn kind(&self) -> ComponentInputConsumerKind {
        self.kind
    }

    pub fn input(&self) -> &ComponentInputName {
        &self.input
    }
}

pub(crate) struct InstantiatedDocument {
    pub document: HtmlDocument,
    pub instances: Vec<ComponentInstanceRecord>,
    pub descendants: Vec<ComponentDescendantProvenance>,
    pub input_consumers: Vec<ComponentInputConsumerRecord>,
}

struct InstantiationState<'a> {
    catalog: &'a ComponentCatalog,
    generation: PackageSnapshotGeneration,
    document_serial: u64,
    instances: Vec<ComponentInstanceRecord>,
    descendants: Vec<ComponentDescendantProvenance>,
    input_consumers: Vec<ComponentInputConsumerRecord>,
}

pub(crate) fn parse_component_source(
    html: &str,
    owner: &PackageId,
    logical_source: &str,
    expected: &BTreeMap<ComponentName, Arc<[ComponentInputDeclaration]>>,
) -> Result<BTreeMap<ComponentName, UnresolvedComponentDefinition>, PackageLoadError> {
    reject_duplicate_control_attributes(html, logical_source)?;
    let document = HtmlDocument::from_html(html, parser_config());
    validate_document_depth(&document, owner, logical_source)?;
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
                if !expected.contains_key(&name) {
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
                let context = ComponentNormalizationContext {
                    owner,
                    logical_source,
                    definition_name: &name,
                    inputs: expected
                        .get(&name)
                        .expect("validated component export inputs exist"),
                };
                let children = node
                    .children
                    .iter()
                    .map(|child| {
                        normalize_component_node(
                            &document,
                            *child,
                            context,
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
                        key: ComponentDefinitionKey::new(owner.clone(), name.clone()),
                        logical_source: logical_source.to_owned(),
                        nodes: children,
                        source_node_count,
                        inputs: Arc::clone(
                            expected
                                .get(&name)
                                .expect("validated component export inputs exist"),
                        ),
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
    for name in expected.keys() {
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
    let input_declarations: BTreeMap<_, _> = unresolved
        .iter()
        .map(|definition| (definition.key.clone(), Arc::clone(&definition.inputs)))
        .collect();
    let mut definitions = Vec::with_capacity(unresolved.len());
    let mut indices = BTreeMap::new();
    for unresolved in unresolved {
        let mut dependencies = Vec::new();
        let mut dependency_set = BTreeSet::new();
        let mut references = Vec::new();
        let mut resolution = ComponentResolutionContext {
            packages: &package_by_id,
            available: &available,
            input_declarations: &input_declarations,
            dependencies: &mut dependencies,
            dependency_set: &mut dependency_set,
            references: &mut references,
        };
        let nodes = resolve_nodes(
            unresolved.nodes,
            &unresolved.key.package_id,
            &mut resolution,
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
            inputs: unresolved.inputs,
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
    validate_document_depth(&document, owner.id(), logical_path)?;
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

#[derive(Clone, Copy)]
struct ComponentNormalizationContext<'a> {
    owner: &'a PackageId,
    logical_source: &'a str,
    definition_name: &'a ComponentName,
    inputs: &'a [ComponentInputDeclaration],
}

fn normalize_component_node(
    document: &HtmlDocument,
    slot: usize,
    context: ComponentNormalizationContext<'_>,
    ordinal: &mut u32,
    count: &mut usize,
) -> Result<UnresolvedTemplateNode, PackageLoadError> {
    let ComponentNormalizationContext {
        owner,
        logical_source,
        definition_name,
        inputs,
    } = context;
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
                let supplied_inputs =
                    validate_use_element(element, &node.children, document, owner, logical_source)?;
                let value = element_attr(element, "component")
                    .expect("validated htm-use has component attribute");
                let reference = ComponentReference::parse(value)
                    .map_err(|error| error.in_package(owner.to_string()).at(logical_source))?;
                return Ok(UnresolvedTemplateNode::Use {
                    reference,
                    supplied_inputs,
                    source_ordinal,
                });
            }
            let children = node
                .children
                .iter()
                .map(|child| normalize_component_node(document, *child, context, ordinal, count))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some((consumer_kind, input, value_format)) = validate_component_input_consumer(
                element,
                &children,
                inputs,
                owner,
                logical_source,
            )? {
                return Ok(UnresolvedTemplateNode::InputConsumer {
                    name: element.name.clone(),
                    attributes: element.attrs().to_vec(),
                    children: children
                        .into_iter()
                        .filter(|child| matches!(child, UnresolvedTemplateNode::Comment { .. }))
                        .collect(),
                    consumer_kind,
                    input,
                    value_format,
                    source_ordinal,
                });
            }
            validate_static_component_element(element, owner, logical_source)?;
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
                let supplied_inputs = validate_use_element(
                    element,
                    &node.children,
                    document,
                    owner.id(),
                    logical_path,
                )?;
                let reference = ComponentReference::parse(
                    element_attr(element, "component")
                        .expect("validated htm-use has component attribute"),
                )
                .map_err(|error| error.in_package(owner.id().to_string()).at(logical_path))?;
                let target = resolve_reference_from_package(owner, &reference, catalog)?;
                let definition = catalog
                    .definition(&target)
                    .expect("resolved definition exists");
                let inputs = resolve_component_inputs(
                    definition.inputs(),
                    supplied_inputs,
                    owner.id(),
                    &target.name,
                    logical_path,
                )?;
                return Ok(ComponentTemplateNode::Host {
                    reference,
                    target,
                    inputs,
                    source_ordinal,
                });
            }
            if element_attr(element, BIND_ATTRIBUTE)
                .is_some_and(|value| value.starts_with("input."))
            {
                return Err(component_error(
                    PackageErrorKind::ComponentInputNamespaceOutsideComponent,
                    owner.id(),
                    logical_path,
                    "`input.*` is available only inside a component instance",
                ));
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

fn validate_component_input_consumer(
    element: &blitz_dom::ElementData,
    children: &[UnresolvedTemplateNode],
    inputs: &[ComponentInputDeclaration],
    owner: &PackageId,
    logical_source: &str,
) -> Result<
    Option<(
        ComponentInputConsumerKind,
        ComponentInputName,
        StateValueFormat,
    )>,
    PackageLoadError,
> {
    let Some(kind_name) = element_attr(element, BUILTIN_ATTRIBUTE) else {
        return Ok(None);
    };
    let (kind, allowed_tags): (ComponentInputConsumerKind, &[&str]) = match kind_name {
        "state-text" => (
            ComponentInputConsumerKind::StateText,
            &["span", "p", "output"],
        ),
        "state-token" => (
            ComponentInputConsumerKind::StateToken,
            &["div", "span", "section"],
        ),
        "state-value" => (ComponentInputConsumerKind::StateValue, &["data"]),
        _ => return Ok(None),
    };
    let Some(binding) =
        element_attr(element, BIND_ATTRIBUTE).filter(|binding| binding.starts_with("input."))
    else {
        return Ok(None);
    };
    let tag = element.name.local.as_ref();
    if !allowed_tags.contains(&tag) {
        return Err(component_error(
            PackageErrorKind::ComponentInputConsumerTypeMismatch,
            owner,
            logical_source,
            format!("`{kind_name}` cannot use component element `<{tag}>`"),
        ));
    }
    if children.iter().any(|node| {
        matches!(
            node,
            UnresolvedTemplateNode::Element { .. }
                | UnresolvedTemplateNode::Use { .. }
                | UnresolvedTemplateNode::InputConsumer { .. }
        )
    }) {
        return Err(component_error(
            PackageErrorKind::ComponentInputConsumerTypeMismatch,
            owner,
            logical_source,
            format!("`{kind_name}` content must not contain child elements"),
        ));
    }
    for attribute in element.attrs() {
        let name = attribute.name.local.as_ref();
        let allowed = match kind {
            ComponentInputConsumerKind::StateValue => {
                matches!(name, BUILTIN_ATTRIBUTE | BIND_ATTRIBUTE | FORMAT_ATTRIBUTE)
            }
            ComponentInputConsumerKind::StateText | ComponentInputConsumerKind::StateToken => {
                matches!(name, BUILTIN_ATTRIBUTE | BIND_ATTRIBUTE)
            }
        };
        if name.starts_with("data-htm-") && !allowed {
            return Err(component_error(
                PackageErrorKind::ComponentStateActionNotSupported,
                owner,
                logical_source,
                format!("unsupported component input consumer attribute `{name}`"),
            ));
        }
        if matches!(
            name,
            "id" | "for"
                | "slot"
                | "value"
                | "aria-labelledby"
                | "aria-describedby"
                | "aria-controls"
                | "aria-owns"
                | "aria-activedescendant"
                | "list"
                | "form"
                | "headers"
        ) || (name == "href" && attribute.value.starts_with('#'))
        {
            return Err(component_error(
                PackageErrorKind::ComponentFeatureNotSupported,
                owner,
                logical_source,
                format!("component-local attribute `{name}` is not supported"),
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
            let lowercase = attribute.value.to_ascii_lowercase();
            if lowercase.contains("url")
                || lowercase.contains("@import")
                || attribute.value.contains('\\')
            {
                return Err(component_error(
                    PackageErrorKind::ComponentResourceNotSupported,
                    owner,
                    logical_source,
                    "component inline style must not load external resources",
                ));
            }
        }
    }
    let input_name = binding
        .strip_prefix("input.")
        .expect("component input binding prefix was checked");
    let input = ComponentInputName::parse(input_name)
        .map_err(|error| error.in_package(owner.to_string()).at(logical_source))?;
    let declaration = inputs
        .iter()
        .find(|declaration| declaration.name() == &input)
        .ok_or_else(|| {
            component_error(
                PackageErrorKind::ComponentInputNamespaceUnknown,
                owner,
                logical_source,
                format!("component input `{input}` is not declared"),
            )
        })?;
    let compatible = match kind {
        ComponentInputConsumerKind::StateText => true,
        ComponentInputConsumerKind::StateToken => matches!(
            declaration.input_type(),
            ComponentInputType::Token | ComponentInputType::Boolean
        ),
        ComponentInputConsumerKind::StateValue => {
            declaration.input_type() == ComponentInputType::Number
        }
    };
    if !compatible {
        return Err(component_error(
            PackageErrorKind::ComponentInputConsumerTypeMismatch,
            owner,
            logical_source,
            format!(
                "`{kind_name}` cannot consume component input `{input}` of type `{}`",
                declaration.input_type().as_str()
            ),
        ));
    }
    let value_format = match element_attr(element, FORMAT_ATTRIBUTE) {
        None | Some("raw") => StateValueFormat::Raw,
        Some(value) => {
            return Err(component_error(
                PackageErrorKind::ComponentInputConsumerTypeMismatch,
                owner,
                logical_source,
                format!("component state-value supports only raw format, not `{value}`"),
            ));
        }
    };
    Ok(Some((kind, input, value_format)))
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
                "component input placeholder attributes are not supported",
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
) -> Result<Vec<(ComponentInputName, String)>, PackageLoadError> {
    if element_attr(element, "component").is_none() {
        return Err(component_error(
            PackageErrorKind::ComponentInvocationAttributes,
            owner,
            logical_source,
            "`htm-use` requires one `component` attribute",
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
    let supplied_count = element.attrs().len().saturating_sub(1);
    if supplied_count > MAX_COMPONENT_INPUT_ATTRIBUTES {
        return Err(component_error(
            PackageErrorKind::ComponentInputCountLimit,
            owner,
            logical_source,
            format!(
                "`htm-use` supplies {supplied_count} inputs; limit is {MAX_COMPONENT_INPUT_ATTRIBUTES}"
            ),
        ));
    }
    let mut supplied = Vec::with_capacity(supplied_count);
    let mut supplied_names = BTreeSet::new();
    let mut literal_bytes = 0usize;
    for attribute in element.attrs() {
        let attribute_name = attribute.name.local.as_ref();
        if attribute_name == "component" {
            continue;
        }
        let Some(suffix) = attribute_name.strip_prefix("input-") else {
            return Err(component_error(
                PackageErrorKind::ComponentInvocationAttributes,
                owner,
                logical_source,
                format!("unsupported `htm-use` attribute `{attribute_name}`"),
            ));
        };
        let input = ComponentInputName::parse(suffix)
            .map_err(|error| error.in_package(owner.to_string()).at(logical_source))?;
        if !supplied_names.insert(input.clone()) {
            return Err(component_error(
                PackageErrorKind::ComponentInputDuplicate,
                owner,
                logical_source,
                format!("`htm-use` repeats input `{input}`"),
            ));
        }
        literal_bytes = literal_bytes
            .checked_add(attribute.value.len())
            .ok_or_else(|| {
                component_error(
                    PackageErrorKind::ComponentInputLiteralByteLimit,
                    owner,
                    logical_source,
                    "component input literal byte count overflowed",
                )
            })?;
        if literal_bytes > MAX_COMPONENT_INPUT_LITERAL_BYTES {
            return Err(component_error(
                PackageErrorKind::ComponentInputLiteralByteLimit,
                owner,
                logical_source,
                format!(
                    "`htm-use` literal inputs exceed {MAX_COMPONENT_INPUT_LITERAL_BYTES} UTF-8 bytes"
                ),
            ));
        }
        supplied.push((input, attribute.value.to_string()));
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
    Ok(supplied)
}

struct ComponentResolutionContext<'a> {
    packages: &'a BTreeMap<PackageId, Arc<ResolvedPackage>>,
    available: &'a BTreeSet<ComponentDefinitionKey>,
    input_declarations: &'a BTreeMap<ComponentDefinitionKey, Arc<[ComponentInputDeclaration]>>,
    dependencies: &'a mut Vec<ComponentDefinitionKey>,
    dependency_set: &'a mut BTreeSet<ComponentDefinitionKey>,
    references: &'a mut Vec<(ComponentReference, ComponentDefinitionKey)>,
}

fn resolve_nodes(
    nodes: Vec<UnresolvedTemplateNode>,
    owner: &PackageId,
    context: &mut ComponentResolutionContext<'_>,
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
                children: resolve_nodes(children, owner, context)?.into(),
                source_ordinal,
            }),
            UnresolvedTemplateNode::Use {
                reference,
                supplied_inputs,
                source_ordinal,
            } => {
                let package = context.packages.get(owner).ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component owner package `{owner}` is absent"),
                    )
                })?;
                let target = resolve_reference_key(package, &reference)?;
                if !context.available.contains(&target) {
                    return Err(PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component reference `{reference}` resolves to missing `{target}`"),
                    )
                    .in_package(owner.to_string()));
                }
                let declarations = context.input_declarations.get(&target).ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentExportUnknown,
                        format!("component input declarations for `{target}` are absent"),
                    )
                })?;
                let inputs = resolve_component_inputs(
                    declarations,
                    supplied_inputs,
                    owner,
                    &target.name,
                    "component template",
                )?;
                if context.dependency_set.insert(target.clone()) {
                    context.dependencies.push(target.clone());
                }
                context.references.push((reference.clone(), target.clone()));
                Ok(ComponentTemplateNode::Host {
                    reference,
                    target,
                    inputs,
                    source_ordinal,
                })
            }
            UnresolvedTemplateNode::InputConsumer {
                name,
                attributes,
                children,
                consumer_kind,
                input,
                value_format,
                source_ordinal,
            } => Ok(ComponentTemplateNode::InputConsumer {
                name,
                attributes,
                children: resolve_nodes(children, owner, context)?.into(),
                consumer_kind,
                input,
                value_format,
                source_ordinal,
            }),
        })
        .collect()
}

fn resolve_component_inputs(
    declarations: &[ComponentInputDeclaration],
    supplied: Vec<(ComponentInputName, String)>,
    owner: &PackageId,
    component: &ComponentName,
    logical_source: &str,
) -> Result<ResolvedComponentInputs, PackageLoadError> {
    let supplied: BTreeMap<_, _> = supplied.into_iter().collect();
    for name in supplied.keys() {
        if !declarations
            .iter()
            .any(|declaration| declaration.name() == name)
        {
            return Err(component_error(
                PackageErrorKind::ComponentInputUnknown,
                owner,
                logical_source,
                format!("component `{component}` does not declare input `{name}`"),
            ));
        }
    }
    let mut values = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let (value, provenance) = match supplied.get(declaration.name()) {
            Some(literal) => (
                parse_component_input_literal(declaration.input_type(), literal).map_err(
                    |error| {
                        component_error(
                            error.kind(),
                            owner,
                            logical_source,
                            format!(
                                "component `{component}` input `{}`: {error}",
                                declaration.name()
                            ),
                        )
                    },
                )?,
                ComponentInputProvenance::Supplied,
            ),
            None => match declaration.default() {
                Some(value) => (value.clone(), ComponentInputProvenance::Defaulted),
                None => {
                    return Err(component_error(
                        PackageErrorKind::ComponentInputMissingRequired,
                        owner,
                        logical_source,
                        format!(
                            "component `{component}` is missing required input `{}`",
                            declaration.name()
                        ),
                    ));
                }
            },
        };
        values.push(ResolvedComponentInput {
            declaration: declaration.clone(),
            value,
            provenance,
        });
    }
    Ok(ResolvedComponentInputs::new(values))
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
        fn add_expanded(state: &mut Expansion<'_>, count: usize) -> Result<(), PackageLoadError> {
            state.expanded = state.expanded.checked_add(count).ok_or_else(|| {
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
            Ok(())
        }

        for node in nodes {
            add_expanded(state, 1)?;
            match node {
                ComponentTemplateNode::Element { children, .. } => {
                    visit(children, state, depth, path)?;
                }
                ComponentTemplateNode::InputConsumer { children, .. } => {
                    // Materialization always appends one canonical value text node.
                    add_expanded(state, 1)?;
                    visit(children, state, depth, path)?;
                }
                ComponentTemplateNode::Host {
                    reference,
                    target,
                    source_ordinal,
                    ..
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
    current_inputs: Option<&ResolvedComponentInputs>,
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
                    current_inputs,
                    invocation_path,
                    state,
                )?;
                document.mutate().append_children(slot, &child_slots);
                created.push(slot);
            }
            ComponentTemplateNode::InputConsumer {
                name,
                attributes,
                children,
                consumer_kind,
                input,
                value_format,
                source_ordinal,
            } => {
                let instance_id = current_instance.ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentInputNamespaceOutsideComponent,
                        "component input consumer has no component instance",
                    )
                })?;
                let inputs = current_inputs.ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentInputNamespaceOutsideComponent,
                        "component input consumer has no input map",
                    )
                })?;
                let value = inputs.get(input).ok_or_else(|| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentInputNamespaceUnknown,
                        format!("component input `{input}` disappeared during instantiation"),
                    )
                })?;
                let mut attributes = attributes.clone();
                let (display, runtime_attribute) =
                    materialize_component_input(*consumer_kind, value, *value_format)?;
                if let Some((name, value)) = runtime_attribute {
                    set_template_attribute(&mut attributes, name, &value);
                }
                let slot = document.mutate().create_element(name.clone(), attributes);
                record_descendant(current_instance, *source_ordinal, slot, state);
                let mut child_slots = instantiate_nodes(
                    document,
                    children,
                    current_instance,
                    current_inputs,
                    invocation_path,
                    state,
                )?;
                let text = document.mutate().create_text_node(&display);
                record_descendant(current_instance, *source_ordinal, text, state);
                child_slots.push(text);
                document.mutate().append_children(slot, &child_slots);
                state.input_consumers.push(ComponentInputConsumerRecord {
                    instance_id: instance_id.clone(),
                    node_slot: slot,
                    template_source_ordinal: *source_ordinal,
                    kind: *consumer_kind,
                    input: input.clone(),
                });
                created.push(slot);
            }
            ComponentTemplateNode::Host {
                reference,
                target,
                inputs,
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
                let instance_inputs = inputs.for_instance();
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
                    inputs: instance_inputs.clone(),
                });
                let child_slots = instantiate_nodes(
                    document,
                    &definition.nodes,
                    Some(&instance_id),
                    Some(&instance_inputs),
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

type MaterializedComponentInput = (String, Option<(&'static str, String)>);

fn materialize_component_input(
    kind: ComponentInputConsumerKind,
    value: &ComponentInputValue,
    value_format: StateValueFormat,
) -> Result<MaterializedComponentInput, PackageLoadError> {
    match kind {
        ComponentInputConsumerKind::StateText => Ok((value.canonical_string(), None)),
        ComponentInputConsumerKind::StateToken => {
            let token = match value {
                ComponentInputValue::Token(value) => value.as_str(),
                ComponentInputValue::Boolean(true) => "true",
                ComponentInputValue::Boolean(false) => "false",
                _ => {
                    return Err(PackageLoadError::new(
                        PackageErrorKind::ComponentInputConsumerTypeMismatch,
                        "state-token received a non-token component input",
                    ));
                }
            };
            Ok((String::new(), Some((STATE_ATTRIBUTE, token.to_owned()))))
        }
        ComponentInputConsumerKind::StateValue => {
            if value_format != StateValueFormat::Raw {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentInputConsumerTypeMismatch,
                    "component state-value supports only raw numeric formatting",
                ));
            }
            let ComponentInputValue::Number(number) = value else {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentInputConsumerTypeMismatch,
                    "state-value received a non-number component input",
                ));
            };
            let formatted = NumericValue::finite_decimal(number.get())
                .format(value_format)
                .map_err(|error| {
                    PackageLoadError::new(
                        PackageErrorKind::ComponentInputConsumerTypeMismatch,
                        format!("component number formatting failed: {error}"),
                    )
                })?;
            Ok((
                formatted.display,
                formatted.value.map(|value| ("value", value)),
            ))
        }
    }
}

fn set_template_attribute(attributes: &mut Vec<Attribute>, name: &str, value: &str) {
    if let Some(attribute) = attributes
        .iter_mut()
        .find(|attribute| attribute.name.local.as_ref() == name)
    {
        attribute.value = value.into();
        return;
    }
    attributes.push(Attribute {
        name: QualName {
            prefix: None,
            ns: ns!(),
            local: LocalName::from(name),
        },
        value: value.into(),
    });
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

fn validate_document_depth(
    document: &HtmlDocument,
    owner: &PackageId,
    logical_source: &str,
) -> Result<(), PackageLoadError> {
    let mut stack = vec![(0usize, 0usize)];
    while let Some((slot, depth)) = stack.pop() {
        if depth > crate::adapter::MAX_DOM_DEPTH {
            return Err(component_error(
                PackageErrorKind::DocumentDepthLimit,
                owner,
                logical_source,
                format!(
                    "document nesting exceeds {} levels",
                    crate::adapter::MAX_DOM_DEPTH
                ),
            ));
        }
        if let Some(node) = document.get_node(slot) {
            stack.extend(node.children.iter().rev().map(|child| (*child, depth + 1)));
        }
    }
    Ok(())
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
    fn tag_end(source: &str, start: usize) -> Option<usize> {
        let mut quote = None;
        for (relative, byte) in source.as_bytes()[start..].iter().copied().enumerate() {
            match (quote, byte) {
                (Some(active), byte) if byte == active => quote = None,
                (Some(_), _) => {}
                (None, b'\'' | b'"') => quote = Some(byte),
                (None, b'>') => return Some(start + relative),
                (None, _) => {}
            }
        }
        None
    }

    if source.contains('\0') {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentInputLiteral,
            "component documents must not contain NUL",
        )
        .at(logical_source));
    }

    fn attribute_names(fragment: &str, tag: &str) -> Vec<String> {
        let bytes = fragment.as_bytes();
        let mut index = 1 + tag.len();
        let mut names = Vec::new();
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
            if name_start < index {
                names.push(fragment[name_start..index].to_ascii_lowercase());
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
        names
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
            let Some(end) = tag_end(source, start) else {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentSourceParse,
                    format!("unterminated `<{tag}>` start tag"),
                )
                .at(logical_source));
            };
            let fragment = &source[start..=end];
            let attribute = if tag == TEMPLATE_ELEMENT {
                COMPONENT_ATTRIBUTE
            } else {
                "component"
            };
            let attribute_names = attribute_names(fragment, tag);
            let count = attribute_names
                .iter()
                .filter(|name| name.as_str() == attribute)
                .count();
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
            if tag == USE_ELEMENT {
                let mut input_names = BTreeSet::new();
                let mut input_count = 0usize;
                for name in attribute_names
                    .iter()
                    .filter(|name| name.starts_with("input-"))
                {
                    input_count = input_count.saturating_add(1);
                    if !input_names.insert(name) {
                        return Err(PackageLoadError::new(
                            PackageErrorKind::ComponentInputDuplicate,
                            format!("`<htm-use>` repeats the `{name}` attribute"),
                        )
                        .at(logical_source));
                    }
                }
                if input_count > MAX_COMPONENT_INPUT_ATTRIBUTES {
                    return Err(PackageLoadError::new(
                        PackageErrorKind::ComponentInputCountLimit,
                        format!(
                            "`<htm-use>` supplies {input_count} inputs; limit is {MAX_COMPONENT_INPUT_ATTRIBUTES}"
                        ),
                    )
                    .at(logical_source));
                }
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
