use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::component::{
    ComponentCatalog, ComponentExport, ComponentInputDeclaration, ComponentInputName,
    ComponentInputType, ComponentName, ComponentSlotDeclaration, ComponentSlotName,
    ComponentValidationTotals, ExpectedComponentDefinition, MAX_COMPONENT_EXPORTS_PER_PACKAGE,
    MAX_COMPONENT_INPUTS, MAX_COMPONENT_SLOTS, MAX_COMPONENT_SOURCE_BYTES, PreparedDocument,
    build_component_catalog, parse_component_input_default, parse_component_source,
    prepare_root_document,
};
use crate::component_style::{
    ComponentStyleCatalog, ComponentStyleValidationTotals, ComponentStylesheetAssociation,
    ComponentStylesheetPath, ComponentStylesheetSemanticVersion, ComponentStylesheetSource,
    ComponentStylesheetSourceKey, MAX_COMPONENT_STYLESHEET_BYTES,
    MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE, MAX_COMPONENT_STYLESHEET_PATH_BYTES,
    MAX_COMPONENT_STYLESHEETS,
};
use crate::stylesheet::{ComponentCssErrorKind, prepare_component_author_stylesheet};

pub const PACKAGE_MANIFEST_FILE: &str = "shell.json";
pub const MAX_PACKAGE_ID_BYTES: usize = 255;
pub const MAX_PACKAGE_ALIAS_BYTES: usize = 64;
pub const MAX_PACKAGE_VERSION_BYTES: usize = 255;
pub const MAX_PACKAGE_PATH_BYTES: usize = 512;
pub const MAX_PACKAGE_MANIFEST_BYTES: u64 = 256 * 1024;
pub const MAX_PACKAGES_PER_GRAPH: usize = 64;
pub const MAX_DIRECT_DEPENDENCIES: usize = 32;
pub const MAX_DEPENDENCY_DEPTH: usize = 16;
pub const MAX_CANDIDATE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PACKAGE_HTML_BYTES: u64 = 2 * 1024 * 1024;

const SCHEMA_V1: u32 = 1;
const SCHEMA_V2: u32 = 2;
const MAX_SURFACE_TEMPLATES: usize = 16;
const MAX_V1_ID_BYTES: usize = 64;
const MAX_DOCUMENT_PATH_BYTES: usize = 512;
const MAX_PANEL_THICKNESS: u32 = 512;
const LEGACY_HEADLESS_ID: &str = "local.headless-root";
const RESERVED_ALIASES: [&str; 9] = [
    "self", "root", "input", "state", "action", "service", "surface", "slot", "htm",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackageErrorKind {
    MissingRoot,
    InvalidRootType,
    ManifestMissing,
    ManifestSymlink,
    ManifestSpecialFile,
    ManifestTooLarge,
    ManifestIo,
    MalformedJson,
    UnsupportedSchema,
    UnknownField,
    InvalidPackageKind,
    InvalidPackageId,
    ReservedPackageId,
    InvalidVersion,
    InvalidDependencyAlias,
    ReservedDependencyAlias,
    InvalidDependencyPath,
    DependencyEscape,
    DependencySymlink,
    DependencyMissing,
    DependencyIdMismatch,
    ImportedShellPackage,
    LibraryTopologyViolation,
    DuplicateAlias,
    DuplicatePackageId,
    PackageLocationConflict,
    PackageVersionConflict,
    DependencyCycle,
    DependencyDepthLimit,
    DirectDependencyLimit,
    PackageCountLimit,
    TotalReadLimit,
    RootTopologyFailure,
    SnapshotGenerationOverflow,
    EntryDocument,
    InvalidComponentExport,
    DuplicateComponentExport,
    InvalidComponentName,
    ReservedComponentName,
    ComponentSourceMissing,
    ComponentSourceInvalidType,
    ComponentSourceSymlink,
    ComponentSourceTooLarge,
    ComponentSourceParse,
    ComponentTemplateMissing,
    ComponentTemplateDuplicate,
    ComponentTemplateUnexported,
    ComponentSourceRenderedContent,
    InvalidComponentReference,
    ComponentAliasUnknown,
    ComponentExportUnknown,
    ComponentDependencyCycle,
    ComponentSourceNodeLimit,
    ComponentGraphExportLimit,
    ComponentNestingLimit,
    ComponentInstanceLimit,
    ComponentReferencedDefinitionLimit,
    ComponentExpandedNodeLimit,
    ComponentInvocationAttributes,
    ComponentInvocationChildren,
    ComponentFeatureNotSupported,
    ComponentResourceNotSupported,
    ComponentStateActionNotSupported,
    ComponentRepeatNotSupported,
    InvalidComponentInputDeclaration,
    DuplicateComponentInputDeclaration,
    InvalidComponentInputName,
    ReservedComponentInputName,
    UnsupportedComponentInputType,
    ComponentInputRequiredWithDefault,
    ComponentInputOptionalWithoutDefault,
    InvalidComponentInputDefault,
    ComponentInputMissingRequired,
    ComponentInputUnknown,
    ComponentInputDuplicate,
    InvalidComponentInputLiteral,
    ComponentInputStringLimit,
    ComponentInputLiteralByteLimit,
    ComponentInputCountLimit,
    ComponentInputNamespaceUnknown,
    ComponentInputConsumerTypeMismatch,
    ComponentInputNamespaceOutsideComponent,
    ComponentInputBindingNotSupported,
    ComponentStateReferenceInputNotSupported,
    ComponentActionReferenceInputNotSupported,
    ComponentResourceReferenceInputNotSupported,
    InvalidComponentSlotDeclaration,
    InvalidComponentSlotName,
    UnsupportedNamedComponentSlot,
    DuplicateDefaultComponentSlot,
    DuplicateComponentSlotDeclaration,
    ComponentSlotDeclarationLimit,
    ComponentSlotDefinitionMissing,
    ComponentSlotDefinitionDuplicate,
    ComponentSlotDefinitionUndeclared,
    ComponentSlotAttributesUnsupported,
    ComponentSlotNestedFallback,
    ComponentSlotOutsideDefinition,
    ComponentRequiredSlotFallback,
    ComponentRequiredSlotContentMissing,
    ComponentInvocationContentWithoutSlot,
    ComponentNamedSlotAttributeUnsupported,
    ComponentSlotAssignmentUnknown,
    ComponentSlotAttributePlacement,
    ComponentProjectedRepeatNotSupported,
    ComponentSlotAssignmentDuplicate,
    ComponentSlotProjectionReentry,
    ComponentSlotProjectionLimit,
    ComponentSlotCallerScopeInvalid,
    ComponentSlotProjectionUnresolved,
    InvalidComponentStylesheetDeclaration,
    ComponentStylesheetDeclarationLimit,
    ComponentStylesheetPackageFileLimit,
    DuplicateComponentStylesheet,
    InvalidComponentStylesheetPath,
    ComponentStylesheetMissing,
    ComponentStylesheetSymlink,
    ComponentStylesheetSpecialFile,
    ComponentStylesheetTooLarge,
    ComponentStylesheetReadFailure,
    ComponentStylesheetParseFailure,
    ComponentStylesheetForbiddenImport,
    ComponentStylesheetForbiddenUrlResource,
    ComponentStylesheetForbiddenFontResource,
    ComponentStylesheetForbiddenHostSelector,
    ComponentStylesheetForbiddenSlottedSelector,
    ComponentStylesheetForbiddenShadowSelector,
    ComponentStylesheetAssociationInvalid,
    ComponentStyleScopeActivationInvalid,
    ComponentStyleOwnerAssignmentInvalid,
    ComponentSelectorScopeValidationFailure,
    DocumentDepthLimit,
    PermissionDenied,
    SpecialFile,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageLoadError {
    kind: PackageErrorKind,
    message: String,
    package_id: Option<String>,
    logical_path: Option<String>,
}

impl PackageLoadError {
    pub(crate) fn new(kind: PackageErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: bounded_message(message.into()),
            package_id: None,
            logical_path: None,
        }
    }

    pub(crate) fn at(mut self, logical_path: impl Into<String>) -> Self {
        self.logical_path = Some(bounded_path(logical_path.into()));
        self
    }

    pub(crate) fn in_package(mut self, package_id: impl Into<String>) -> Self {
        self.package_id = Some(bounded_path(package_id.into()));
        self
    }

    pub fn kind(&self) -> PackageErrorKind {
        self.kind
    }

    pub fn package_id(&self) -> Option<&str> {
        self.package_id.as_deref()
    }

    pub fn logical_path(&self) -> Option<&str> {
        self.logical_path.as_deref()
    }
}

impl fmt::Display for PackageLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "package load error ({:?})", self.kind)?;
        if let Some(package_id) = &self.package_id {
            write!(f, " in `{package_id}`")?;
        }
        if let Some(path) = &self.logical_path {
            write!(f, " at `{path}`")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for PackageLoadError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId(String);

impl PackageId {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        validate_package_id(value)?;
        Ok(Self(value.to_owned()))
    }

    fn compatibility(value: String) -> Result<Self, PackageLoadError> {
        let suffix = value.strip_prefix("local.").ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::InvalidPackageId,
                "compatibility package ID must use the reserved `local.` prefix",
            )
        })?;
        if value.len() > MAX_PACKAGE_ID_BYTES {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidPackageId,
                format!("compatibility package ID exceeds {MAX_PACKAGE_ID_BYTES} bytes"),
            ));
        }
        validate_v1_id("compatibility package ID", suffix)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageAlias(String);

impl PackageAlias {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        if value.is_empty() || value.len() > MAX_PACKAGE_ALIAS_BYTES || !value.is_ascii() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidDependencyAlias,
                format!("dependency alias must contain 1..={MAX_PACKAGE_ALIAS_BYTES} ASCII bytes"),
            ));
        }
        let bytes = value.as_bytes();
        if !bytes[0].is_ascii_lowercase()
            || bytes.last() == Some(&b'-')
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidDependencyAlias,
                format!(
                    "dependency alias `{value}` must start with a lowercase letter and use lowercase letters, digits, and interior hyphens"
                ),
            ));
        }
        if RESERVED_ALIASES.contains(&value) {
            return Err(PackageLoadError::new(
                PackageErrorKind::ReservedDependencyAlias,
                format!("dependency alias `{value}` is reserved"),
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageVersion(Version);

impl PackageVersion {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        if value.is_empty() || value.len() > MAX_PACKAGE_VERSION_BYTES || !value.is_ascii() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidVersion,
                format!("package version must contain 1..={MAX_PACKAGE_VERSION_BYTES} ASCII bytes"),
            ));
        }
        Version::parse(value).map(Self).map_err(|error| {
            PackageLoadError::new(
                PackageErrorKind::InvalidVersion,
                format!("invalid SemVer package version `{value}`: {error}"),
            )
        })
    }

    pub fn as_semver(&self) -> &Version {
        &self.0
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    Shell,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageSchemaSource {
    SchemaV1,
    SchemaV2,
    LegacyHeadless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PackageSnapshotGeneration(u64);

impl PackageSnapshotGeneration {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManifestMeasurements {
    pub parse_us: u64,
    pub validation_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputScope {
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Panel,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTemplate {
    pub edge: PanelEdge,
    pub thickness: u32,
    pub reserve_space: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEdge {
    Top,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayTemplate {
    pub initially_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfacePreset {
    Panel(PanelTemplate),
    Overlay(OverlayTemplate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTemplate {
    id: String,
    document: PathBuf,
    canonical_document: PathBuf,
    html: Arc<str>,
    prepared_document: Option<Arc<PreparedDocument>>,
    outputs: OutputScope,
    preset: SurfacePreset,
    namespace: String,
}

impl SurfaceTemplate {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn document(&self) -> &Path {
        &self.document
    }

    pub fn canonical_document(&self) -> &Path {
        &self.canonical_document
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn prepared_document(&self) -> Option<&Arc<PreparedDocument>> {
        self.prepared_document.as_ref()
    }

    pub fn outputs(&self) -> OutputScope {
        self.outputs
    }

    pub fn kind(&self) -> SurfaceKind {
        match self.preset {
            SurfacePreset::Panel(_) => SurfaceKind::Panel,
            SurfacePreset::Overlay(_) => SurfaceKind::Overlay,
        }
    }

    pub fn panel(&self) -> Option<&PanelTemplate> {
        match &self.preset {
            SurfacePreset::Panel(panel) => Some(panel),
            SurfacePreset::Overlay(_) => None,
        }
    }

    pub fn overlay(&self) -> Option<&OverlayTemplate> {
        match &self.preset {
            SurfacePreset::Overlay(overlay) => Some(overlay),
            SurfacePreset::Panel(_) => None,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellManifest {
    pub version: u32,
    pub id: String,
    pub surfaces: Vec<SurfaceTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDependency {
    alias: PackageAlias,
    expected_id: PackageId,
    logical_path: String,
}

impl PackageDependency {
    pub fn alias(&self) -> &PackageAlias {
        &self.alias
    }

    pub fn expected_id(&self) -> &PackageId {
        &self.expected_id
    }

    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackageDependency {
    alias: PackageAlias,
    target: PackageId,
}

impl ResolvedPackageDependency {
    pub fn alias(&self) -> &PackageAlias {
        &self.alias
    }

    pub fn target(&self) -> &PackageId {
        &self.target
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    id: PackageId,
    kind: PackageKind,
    version: Option<PackageVersion>,
    schema: PackageSchemaSource,
    compatibility_normalized: bool,
    logical_location: String,
    canonical_root: PathBuf,
    manifest_source: Option<PathBuf>,
    dependencies: Vec<ResolvedPackageDependency>,
    components: Vec<ComponentExport>,
}

impl ResolvedPackage {
    pub fn id(&self) -> &PackageId {
        &self.id
    }

    pub fn kind(&self) -> PackageKind {
        self.kind
    }

    pub fn version(&self) -> Option<&PackageVersion> {
        self.version.as_ref()
    }

    pub fn schema_source(&self) -> PackageSchemaSource {
        self.schema
    }

    pub fn compatibility_normalized(&self) -> bool {
        self.compatibility_normalized
    }

    pub fn logical_location(&self) -> &str {
        &self.logical_location
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn manifest_source(&self) -> Option<&Path> {
        self.manifest_source.as_deref()
    }

    pub fn dependencies(&self) -> &[ResolvedPackageDependency] {
        &self.dependencies
    }

    pub fn components(&self) -> &[ComponentExport] {
        &self.components
    }
}

#[derive(Debug, Clone)]
pub struct PackageEntryDocument {
    logical_path: PathBuf,
    canonical_path: PathBuf,
    html: Arc<str>,
    prepared_document: Option<Arc<PreparedDocument>>,
}

impl PackageEntryDocument {
    pub fn logical_path(&self) -> &Path {
        &self.logical_path
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn prepared_document(&self) -> Option<&Arc<PreparedDocument>> {
        self.prepared_document.as_ref()
    }
}

#[derive(Debug)]
pub struct PackageSnapshot {
    generation: PackageSnapshotGeneration,
    composition_root: PathBuf,
    packages: Arc<[Arc<ResolvedPackage>]>,
    root_index: usize,
    root_manifest: Option<ShellManifest>,
    headless_entry: Option<PackageEntryDocument>,
    components: Arc<ComponentCatalog>,
    component_styles: Arc<ComponentStyleCatalog>,
    bytes_read: u64,
    measurements: ManifestMeasurements,
}

impl PackageSnapshot {
    pub fn generation(&self) -> PackageSnapshotGeneration {
        self.generation
    }

    pub fn composition_root(&self) -> &Path {
        &self.composition_root
    }

    pub fn packages(&self) -> &[Arc<ResolvedPackage>] {
        &self.packages
    }

    pub fn root_package(&self) -> &ResolvedPackage {
        &self.packages[self.root_index]
    }

    pub fn root_manifest(&self) -> Option<&ShellManifest> {
        self.root_manifest.as_ref()
    }

    pub fn headless_entry(&self) -> Option<&PackageEntryDocument> {
        self.headless_entry.as_ref()
    }

    pub fn components(&self) -> &ComponentCatalog {
        &self.components
    }

    pub fn component_styles(&self) -> &ComponentStyleCatalog {
        &self.component_styles
    }

    pub fn component_definition_id(
        &self,
        key: &crate::ComponentDefinitionKey,
    ) -> Option<crate::ComponentDefinitionId> {
        self.components
            .definition(key)
            .map(|_| crate::ComponentDefinitionId {
                generation: self.generation,
                key: key.clone(),
            })
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn measurements(&self) -> ManifestMeasurements {
        self.measurements
    }

    pub fn node_identity(&self, id: &PackageId) -> Option<PackageNodeIdentity> {
        self.packages
            .iter()
            .any(|package| package.id() == id)
            .then(|| PackageNodeIdentity {
                generation: self.generation,
                package_id: id.clone(),
            })
    }

    pub fn contains_node_identity(&self, identity: &PackageNodeIdentity) -> bool {
        identity.generation == self.generation
            && self
                .packages
                .iter()
                .any(|package| package.id() == &identity.package_id)
    }

    pub fn deterministic_json(&self) -> Result<String, serde_json::Error> {
        let diagnostic = PackageGraphDiagnostic::from_snapshot(self);
        serde_json::to_string_pretty(&diagnostic).map(|mut json| {
            json.push('\n');
            json
        })
    }

    pub(crate) fn instantiate_document(
        &self,
        prepared: &PreparedDocument,
        document_serial: u64,
        config: blitz_dom::DocumentConfig,
    ) -> Result<crate::component::InstantiatedDocument, PackageLoadError> {
        let mut instantiated =
            prepared.instantiate(&self.components, self.generation, document_serial, config)?;
        instantiated.style_activation = self
            .component_styles
            .activation_mode(
                prepared.referenced_definition_keys(),
                self.generation,
                prepared.ownership_aware_styles(),
            )
            .map_err(|error| {
                PackageLoadError::new(
                    PackageErrorKind::ComponentStyleScopeActivationInvalid,
                    error.to_string(),
                )
                .at(prepared.logical_path())
            })?;
        Ok(instantiated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageNodeIdentity {
    generation: PackageSnapshotGeneration,
    package_id: PackageId,
}

impl PackageNodeIdentity {
    pub fn generation(&self) -> PackageSnapshotGeneration {
        self.generation
    }

    pub fn package_id(&self) -> &PackageId {
        &self.package_id
    }
}

#[derive(Debug)]
pub struct PackageSnapshotCandidate {
    composition_root: PathBuf,
    packages: Vec<Arc<ResolvedPackage>>,
    root_index: usize,
    root_manifest: Option<ShellManifest>,
    headless_entry: Option<PackageEntryDocument>,
    components: ComponentCatalog,
    component_styles: ComponentStyleCatalog,
    bytes_read: u64,
    measurements: ManifestMeasurements,
}

impl PackageSnapshotCandidate {
    pub fn package_count(&self) -> usize {
        self.packages.len()
    }

    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }
}

#[derive(Debug)]
pub struct PackageSnapshotLoader {
    current: Option<Arc<PackageSnapshot>>,
    next_generation: u64,
    file_system: Arc<dyn ReadOnlyPackageFileSystem>,
}

impl Default for PackageSnapshotLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageSnapshotLoader {
    pub fn new() -> Self {
        Self {
            current: None,
            next_generation: 1,
            file_system: Arc::new(LocalPackageFileSystem),
        }
    }

    #[cfg(test)]
    fn with_file_system(file_system: Arc<dyn ReadOnlyPackageFileSystem>) -> Self {
        Self {
            current: None,
            next_generation: 1,
            file_system,
        }
    }

    pub fn current(&self) -> Option<&Arc<PackageSnapshot>> {
        self.current.as_ref()
    }

    pub fn build_manifest_candidate(
        &self,
        manifest_path: impl AsRef<Path>,
    ) -> Result<PackageSnapshotCandidate, PackageLoadError> {
        build_manifest_candidate(self.file_system.as_ref(), manifest_path.as_ref(), false)
    }

    pub fn build_headless_candidate(
        &self,
        package_root: impl AsRef<Path>,
    ) -> Result<PackageSnapshotCandidate, PackageLoadError> {
        build_headless_candidate(self.file_system.as_ref(), package_root.as_ref())
    }

    pub fn publish(
        &mut self,
        candidate: PackageSnapshotCandidate,
    ) -> Result<Arc<PackageSnapshot>, PackageLoadError> {
        let next = self.next_generation.checked_add(1).ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::SnapshotGenerationOverflow,
                "package snapshot generation is exhausted",
            )
        })?;
        let snapshot = Arc::new(PackageSnapshot {
            generation: PackageSnapshotGeneration(self.next_generation),
            composition_root: candidate.composition_root,
            packages: candidate.packages.into(),
            root_index: candidate.root_index,
            root_manifest: candidate.root_manifest,
            headless_entry: candidate.headless_entry,
            components: Arc::new(candidate.components),
            component_styles: Arc::new(candidate.component_styles),
            bytes_read: candidate.bytes_read,
            measurements: candidate.measurements,
        });
        validate_snapshot_style_scopes(&snapshot)?;
        self.next_generation = next;
        self.current = Some(Arc::clone(&snapshot));
        Ok(snapshot)
    }

    pub fn load_manifest(
        &mut self,
        manifest_path: impl AsRef<Path>,
    ) -> Result<Arc<PackageSnapshot>, PackageLoadError> {
        let candidate = self.build_manifest_candidate(manifest_path)?;
        self.publish(candidate)
    }

    pub fn load_headless(
        &mut self,
        package_root: impl AsRef<Path>,
    ) -> Result<Arc<PackageSnapshot>, PackageLoadError> {
        let candidate = self.build_headless_candidate(package_root)?;
        self.publish(candidate)
    }
}

fn validate_snapshot_style_scopes(snapshot: &PackageSnapshot) -> Result<(), PackageLoadError> {
    let mut prepared = Vec::new();
    if let Some(manifest) = snapshot.root_manifest() {
        prepared.extend(
            manifest
                .surfaces
                .iter()
                .filter_map(SurfaceTemplate::prepared_document),
        );
    }
    if let Some(entry) = snapshot.headless_entry()
        && let Some(document) = entry.prepared_document()
    {
        prepared.push(document);
    }
    for (index, prepared) in prepared.into_iter().enumerate() {
        if !prepared.ownership_aware_styles() {
            continue;
        }
        let serial = u64::try_from(index + 1).expect("surface template limit fits u64");
        let mut instantiated = snapshot.instantiate_document(
            prepared,
            serial,
            blitz_dom::DocumentConfig {
                base_url: Some("htm-package://candidate/root.html".to_owned()),
                style_threading: blitz_dom::StyleThreading::Sequential,
                ..Default::default()
            },
        )?;
        crate::style_owner::activate_style_ownership(
            &mut instantiated.document,
            &instantiated.style_ownership,
            &instantiated.style_activation,
        )
        .map_err(|error| {
            PackageLoadError::new(
                PackageErrorKind::ComponentSelectorScopeValidationFailure,
                error.to_string(),
            )
            .at(prepared.logical_path())
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ValidatedManifest {
    source: PathBuf,
    package_root: PathBuf,
    manifest: ShellManifest,
    snapshot: Arc<PackageSnapshot>,
    parse_count: u32,
    measurements: ManifestMeasurements,
}

impl ValidatedManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PackageLoadError> {
        let mut loader = PackageSnapshotLoader::new();
        let snapshot = loader.load_manifest(path.as_ref())?;
        let root = snapshot.root_package();
        let source = root.manifest_source.clone().ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::ManifestMissing,
                "published manifest snapshot has no root manifest source",
            )
        })?;
        let manifest = snapshot.root_manifest.clone().ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::RootTopologyFailure,
                "published manifest snapshot has no shell topology",
            )
        })?;
        Ok(Self {
            source,
            package_root: root.canonical_root.clone(),
            manifest,
            measurements: snapshot.measurements,
            snapshot,
            parse_count: 1,
        })
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn manifest(&self) -> &ShellManifest {
        &self.manifest
    }

    pub fn snapshot(&self) -> &Arc<PackageSnapshot> {
        &self.snapshot
    }

    pub fn parse_count(&self) -> u32 {
        self.parse_count
    }

    pub fn measurements(&self) -> ManifestMeasurements {
        self.measurements
    }

    pub fn deterministic_package_graph_json(&self) -> Result<String, serde_json::Error> {
        self.snapshot.deterministic_json()
    }

    pub fn surface(&self, id: &str) -> Option<&SurfaceTemplate> {
        self.manifest
            .surfaces
            .iter()
            .find(|surface| surface.id == id)
    }
}

#[derive(Debug, Serialize)]
struct PackageGraphDiagnostic<'a> {
    snapshot_generation: u64,
    root_package_id: &'a str,
    package_count: usize,
    dependency_first_packages: Vec<PackageDiagnostic<'a>>,
    component_definition_count: usize,
    dependency_first_components: Vec<ComponentDefinitionDiagnostic>,
    component_stylesheet_sources: Vec<ComponentStylesheetSourceDiagnostic>,
    prepared_root_documents: Vec<PreparedDocumentDiagnostic<'a>>,
}

#[derive(Debug, Serialize)]
struct PackageDiagnostic<'a> {
    id: &'a str,
    kind: PackageKind,
    version: Option<String>,
    logical_location: &'a str,
    source_schema: PackageSchemaSource,
    compatibility_normalized: bool,
    dependencies: Vec<DependencyDiagnostic<'a>>,
    component_exports: Vec<ComponentExportDiagnostic<'a>>,
}

#[derive(Debug, Serialize)]
struct DependencyDiagnostic<'a> {
    alias: &'a str,
    target: &'a str,
}

#[derive(Debug, Serialize)]
struct ComponentExportDiagnostic<'a> {
    name: &'a str,
    source: &'a str,
    inputs: Vec<ComponentInputDeclarationDiagnostic>,
    slots: Vec<ComponentSlotDeclarationDiagnostic>,
    styles: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct ComponentInputDeclarationDiagnostic {
    name: String,
    input_type: &'static str,
    required: bool,
    default: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComponentDefinitionDiagnostic {
    identity: String,
    source: String,
    source_nodes: usize,
    inputs: Vec<ComponentInputDeclarationDiagnostic>,
    slots: Vec<ComponentSlotDefinitionDiagnostic>,
    resolved_references: Vec<ComponentReferenceDiagnostic>,
    stylesheets: Vec<ComponentStylesheetAssociationDiagnostic>,
}

#[derive(Debug, Serialize)]
struct ComponentStylesheetSourceDiagnostic {
    identity: String,
    package_id: String,
    path: String,
    semantic_version: String,
    bytes: u64,
    parsed_rules: usize,
    selectors: usize,
}

#[derive(Debug, Serialize)]
struct ComponentStylesheetAssociationDiagnostic {
    identity: String,
    source_identity: String,
    path: String,
    ordinal: u16,
}

#[derive(Debug, Serialize)]
struct ComponentSlotDeclarationDiagnostic {
    name: String,
    required: bool,
}

#[derive(Debug, Serialize)]
struct ComponentSlotDefinitionDiagnostic {
    name: String,
    required: bool,
    source_ordinal: u32,
    fallback_nodes: usize,
    fallback_version: String,
}

#[derive(Debug, Serialize)]
struct ComponentReferenceDiagnostic {
    reference: String,
    target: String,
}

#[derive(Debug, Serialize)]
struct PreparedDocumentDiagnostic<'a> {
    logical_path: &'a str,
    component_instances: usize,
    referenced_definitions: usize,
    expanded_nodes: usize,
    maximum_nesting_depth: usize,
    instance_paths: &'a [String],
    inputs: Vec<ComponentInstanceInputDiagnostic>,
    consumers: Vec<ComponentInputConsumerDiagnostic>,
    projections: Vec<ComponentSlotProjectionDiagnostic>,
    projected_nodes: Vec<ProjectedNodeDiagnostic>,
    fallback_nodes: Vec<FallbackNodeDiagnostic>,
    matching_mode: &'static str,
    root_style_owner: String,
    style_scope_definitions: Vec<String>,
    style_scope_instances: Vec<String>,
    style_owned_nodes: Vec<StyleOwnedNodeDiagnostic>,
}

#[derive(Debug, Serialize)]
struct StyleOwnedNodeDiagnostic {
    dom_slot: usize,
    dom_slot_generation: u64,
    owner: String,
    category: &'static str,
}

#[derive(Debug, Serialize)]
struct ComponentSlotProjectionDiagnostic {
    identity: String,
    slot_definition: String,
    outcome: &'static str,
    caller: String,
    assigned_nodes: usize,
    fallback_nodes: usize,
    semantic_version: String,
}

#[derive(Debug, Serialize)]
struct ProjectedNodeDiagnostic {
    projection: String,
    caller: String,
    caller_source_ordinal: u32,
    projected_node_ordinal: u32,
    dom_slot: usize,
    dom_slot_generation: u64,
}

#[derive(Debug, Serialize)]
struct FallbackNodeDiagnostic {
    projection: String,
    instance: String,
    fallback_source_ordinal: u32,
    dom_slot: usize,
    dom_slot_generation: u64,
}

#[derive(Debug, Serialize)]
struct ComponentInstanceInputDiagnostic {
    instance_path: String,
    semantic_version: String,
    values: Vec<ComponentInputValueDiagnostic>,
}

#[derive(Debug, Serialize)]
struct ComponentInputValueDiagnostic {
    name: String,
    input_type: &'static str,
    value: String,
    provenance: crate::ComponentInputProvenance,
}

#[derive(Debug, Serialize)]
struct ComponentInputConsumerDiagnostic {
    instance_path: String,
    input: String,
    kind: &'static str,
    source_ordinal: u32,
}

impl<'a> PackageGraphDiagnostic<'a> {
    fn from_snapshot(snapshot: &'a PackageSnapshot) -> Self {
        let mut prepared_root_documents = Vec::new();
        if let Some(manifest) = snapshot.root_manifest() {
            prepared_root_documents.extend(manifest.surfaces.iter().filter_map(|surface| {
                surface.prepared_document().map(|prepared| {
                    let stats = prepared.stats();
                    let diagnostics = prepared_component_diagnostics(snapshot, prepared);
                    PreparedDocumentDiagnostic {
                        logical_path: prepared.logical_path(),
                        component_instances: stats.component_instances,
                        referenced_definitions: stats.referenced_definitions,
                        expanded_nodes: stats.expanded_nodes,
                        maximum_nesting_depth: stats.maximum_nesting_depth,
                        instance_paths: prepared.logical_instance_paths(),
                        inputs: diagnostics.inputs,
                        consumers: diagnostics.consumers,
                        projections: diagnostics.projections,
                        projected_nodes: diagnostics.projected_nodes,
                        fallback_nodes: diagnostics.fallback_nodes,
                        matching_mode: diagnostics.matching_mode,
                        root_style_owner: diagnostics.root_style_owner,
                        style_scope_definitions: diagnostics.style_scope_definitions,
                        style_scope_instances: diagnostics.style_scope_instances,
                        style_owned_nodes: diagnostics.style_owned_nodes,
                    }
                })
            }));
        }
        if let Some(prepared) = snapshot
            .headless_entry()
            .and_then(PackageEntryDocument::prepared_document)
        {
            let stats = prepared.stats();
            if !prepared_root_documents
                .iter()
                .any(|entry| entry.logical_path == prepared.logical_path())
            {
                let diagnostics = prepared_component_diagnostics(snapshot, prepared);
                prepared_root_documents.push(PreparedDocumentDiagnostic {
                    logical_path: prepared.logical_path(),
                    component_instances: stats.component_instances,
                    referenced_definitions: stats.referenced_definitions,
                    expanded_nodes: stats.expanded_nodes,
                    maximum_nesting_depth: stats.maximum_nesting_depth,
                    instance_paths: prepared.logical_instance_paths(),
                    inputs: diagnostics.inputs,
                    consumers: diagnostics.consumers,
                    projections: diagnostics.projections,
                    projected_nodes: diagnostics.projected_nodes,
                    fallback_nodes: diagnostics.fallback_nodes,
                    matching_mode: diagnostics.matching_mode,
                    root_style_owner: diagnostics.root_style_owner,
                    style_scope_definitions: diagnostics.style_scope_definitions,
                    style_scope_instances: diagnostics.style_scope_instances,
                    style_owned_nodes: diagnostics.style_owned_nodes,
                });
            }
        }
        Self {
            snapshot_generation: snapshot.generation.get(),
            root_package_id: snapshot.root_package().id.as_str(),
            package_count: snapshot.packages.len(),
            dependency_first_packages: snapshot
                .packages
                .iter()
                .map(|package| PackageDiagnostic {
                    id: package.id.as_str(),
                    kind: package.kind,
                    version: package.version.as_ref().map(ToString::to_string),
                    logical_location: &package.logical_location,
                    source_schema: package.schema,
                    compatibility_normalized: package.compatibility_normalized,
                    dependencies: package
                        .dependencies
                        .iter()
                        .map(|dependency| DependencyDiagnostic {
                            alias: dependency.alias.as_str(),
                            target: dependency.target.as_str(),
                        })
                        .collect(),
                    component_exports: package
                        .components
                        .iter()
                        .map(|export| ComponentExportDiagnostic {
                            name: export.name().as_str(),
                            source: export.source(),
                            inputs: input_declaration_diagnostics(export.inputs()),
                            slots: export
                                .slots()
                                .iter()
                                .map(|slot| ComponentSlotDeclarationDiagnostic {
                                    name: slot.name().as_str().to_owned(),
                                    required: slot.required(),
                                })
                                .collect(),
                            styles: export
                                .styles()
                                .iter()
                                .map(ComponentStylesheetPath::as_str)
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            component_definition_count: snapshot.components.definitions().len(),
            dependency_first_components: snapshot
                .components
                .dependency_first_order()
                .iter()
                .filter_map(|key| {
                    let definition = snapshot.components.definition(key)?;
                    let identity = snapshot.component_definition_id(key)?;
                    Some(ComponentDefinitionDiagnostic {
                        identity: identity.deterministic_string(),
                        source: definition.logical_source().to_owned(),
                        source_nodes: definition.source_node_count(),
                        inputs: input_declaration_diagnostics(definition.inputs()),
                        slots: definition
                            .slots()
                            .iter()
                            .map(|slot| ComponentSlotDefinitionDiagnostic {
                                name: slot.declaration().name().as_str().to_owned(),
                                required: slot.declaration().required(),
                                source_ordinal: slot.source_ordinal(),
                                fallback_nodes: slot.fallback_node_count(),
                                fallback_version: slot.fallback_version().to_owned(),
                            })
                            .collect(),
                        resolved_references: definition
                            .resolved_references()
                            .iter()
                            .map(|(reference, target)| ComponentReferenceDiagnostic {
                                reference: reference.deterministic_string(),
                                target: target.deterministic_string(),
                            })
                            .collect(),
                        stylesheets: snapshot
                            .component_styles()
                            .associations_for(key)
                            .iter()
                            .map(|association| ComponentStylesheetAssociationDiagnostic {
                                identity: association.deterministic_id(snapshot.generation()),
                                source_identity: association
                                    .source()
                                    .deterministic_id(snapshot.generation()),
                                path: association.source().path().to_string(),
                                ordinal: association.ordinal(),
                            })
                            .collect(),
                    })
                })
                .collect(),
            component_stylesheet_sources: snapshot
                .component_styles()
                .sources()
                .iter()
                .map(|source| ComponentStylesheetSourceDiagnostic {
                    identity: source.deterministic_id(snapshot.generation()),
                    package_id: source.package_id().to_string(),
                    path: source.path().to_string(),
                    semantic_version: source.semantic_version().deterministic_string().to_owned(),
                    bytes: source.bytes(),
                    parsed_rules: source.parsed_rule_count(),
                    selectors: source.selector_count(),
                })
                .collect(),
            prepared_root_documents,
        }
    }
}

fn input_declaration_diagnostics(
    declarations: &[ComponentInputDeclaration],
) -> Vec<ComponentInputDeclarationDiagnostic> {
    declarations
        .iter()
        .map(|declaration| ComponentInputDeclarationDiagnostic {
            name: declaration.name().to_string(),
            input_type: declaration.input_type().as_str(),
            required: declaration.required(),
            default: declaration
                .default()
                .map(crate::ComponentInputValue::canonical_string),
        })
        .collect()
}

struct PreparedComponentDiagnostics {
    inputs: Vec<ComponentInstanceInputDiagnostic>,
    consumers: Vec<ComponentInputConsumerDiagnostic>,
    projections: Vec<ComponentSlotProjectionDiagnostic>,
    projected_nodes: Vec<ProjectedNodeDiagnostic>,
    fallback_nodes: Vec<FallbackNodeDiagnostic>,
    matching_mode: &'static str,
    root_style_owner: String,
    style_scope_definitions: Vec<String>,
    style_scope_instances: Vec<String>,
    style_owned_nodes: Vec<StyleOwnedNodeDiagnostic>,
}

fn prepared_component_diagnostics(
    snapshot: &PackageSnapshot,
    prepared: &PreparedDocument,
) -> PreparedComponentDiagnostics {
    let Ok(instantiated) = snapshot.instantiate_document(
        prepared,
        0,
        blitz_dom::DocumentConfig {
            base_url: Some("htm-local://diagnostic/root.html".to_owned()),
            ..Default::default()
        },
    ) else {
        return PreparedComponentDiagnostics {
            inputs: Vec::new(),
            consumers: Vec::new(),
            projections: Vec::new(),
            projected_nodes: Vec::new(),
            fallback_nodes: Vec::new(),
            matching_mode: "invalid",
            root_style_owner: "invalid".to_owned(),
            style_scope_definitions: Vec::new(),
            style_scope_instances: Vec::new(),
            style_owned_nodes: Vec::new(),
        };
    };
    let matching_mode = instantiated.style_activation.as_str();
    let root_style_owner = instantiated
        .style_ownership
        .root_owner()
        .deterministic_string();
    let style_owned_nodes = instantiated
        .style_ownership
        .nodes()
        .map(|node| StyleOwnedNodeDiagnostic {
            dom_slot: node.dom_slot(),
            dom_slot_generation: node.dom_slot_generation(),
            owner: node.owner().deterministic_string(),
            category: node.kind().as_str(),
        })
        .collect::<Vec<_>>();
    let style_scope_instances = instantiated
        .style_ownership
        .nodes()
        .filter_map(|node| match node.owner() {
            crate::style_owner::StyleOwnerId::RootDocument { .. } => None,
            owner => Some(format!(
                "selector-scope-instance:{}",
                owner.deterministic_string()
            )),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let style_scope_definitions = instantiated
        .style_ownership
        .nodes()
        .filter_map(|node| match node.owner().definition() {
            crate::style_owner::StylesheetOwnerId::RootDocument => None,
            crate::style_owner::StylesheetOwnerId::ComponentDefinition(definition) => {
                Some(format!(
                    "selector-scope-definition:{}",
                    definition.deterministic_string()
                ))
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let instance_paths: BTreeMap<_, _> = instantiated
        .instances
        .iter()
        .map(|instance| (instance.id().clone(), instance.logical_path().to_owned()))
        .collect();
    let inputs = instantiated
        .instances
        .iter()
        .map(|instance| ComponentInstanceInputDiagnostic {
            instance_path: instance.logical_path().to_owned(),
            semantic_version: instance
                .inputs()
                .version()
                .deterministic_string()
                .to_owned(),
            values: instance
                .inputs()
                .values()
                .iter()
                .map(|input| ComponentInputValueDiagnostic {
                    name: input.declaration().name().to_string(),
                    input_type: input.declaration().input_type().as_str(),
                    value: input.value().canonical_string(),
                    provenance: input.provenance(),
                })
                .collect(),
        })
        .collect();
    let consumers = instantiated
        .input_consumers
        .iter()
        .map(|consumer| ComponentInputConsumerDiagnostic {
            instance_path: instance_paths
                .get(consumer.instance_id())
                .cloned()
                .unwrap_or_else(|| "unknown-component-instance".to_owned()),
            input: consumer.input().to_string(),
            kind: consumer.kind().as_str(),
            source_ordinal: consumer.template_source_ordinal(),
        })
        .collect();
    let projections = instantiated
        .slot_projections
        .iter()
        .map(|projection| ComponentSlotProjectionDiagnostic {
            identity: projection.id().deterministic_string(),
            slot_definition: projection.id().slot_definition().deterministic_string(),
            outcome: projection.outcome().as_str(),
            caller: projection.source().deterministic_string(),
            assigned_nodes: projection.assigned_node_count(),
            fallback_nodes: projection.fallback_node_count(),
            semantic_version: projection.version().deterministic_string().to_owned(),
        })
        .collect();
    let projected_nodes = instantiated
        .projected_nodes
        .iter()
        .map(|node| ProjectedNodeDiagnostic {
            projection: node.projection_id().deterministic_string(),
            caller: node.caller().deterministic_string(),
            caller_source_ordinal: node.caller_source_ordinal(),
            projected_node_ordinal: node.projected_node_ordinal(),
            dom_slot: node.dom_slot(),
            dom_slot_generation: node.dom_slot_generation(),
        })
        .collect();
    let fallback_nodes = instantiated
        .fallback_nodes
        .iter()
        .map(|node| FallbackNodeDiagnostic {
            projection: node.projection_id().deterministic_string(),
            instance: node.instance_id().deterministic_string(),
            fallback_source_ordinal: node.fallback_source_ordinal(),
            dom_slot: node.dom_slot(),
            dom_slot_generation: node.dom_slot_generation(),
        })
        .collect();
    PreparedComponentDiagnostics {
        inputs,
        consumers,
        projections,
        projected_nodes,
        fallback_nodes,
        matching_mode,
        root_style_owner,
        style_scope_definitions,
        style_scope_instances,
        style_owned_nodes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageFileKind {
    File,
    Directory,
    Symlink,
    Special,
}

#[derive(Debug, Clone, Copy)]
struct PackageFileMetadata {
    kind: PackageFileKind,
    len: u64,
}

trait ReadOnlyPackageFileSystem: fmt::Debug + Send + Sync {
    fn metadata(&self, path: &Path) -> io::Result<PackageFileMetadata>;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn read_bounded(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>>;
}

#[derive(Debug)]
struct LocalPackageFileSystem;

impl ReadOnlyPackageFileSystem for LocalPackageFileSystem {
    fn metadata(&self, path: &Path) -> io::Result<PackageFileMetadata> {
        let metadata = std::fs::symlink_metadata(path)?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            PackageFileKind::Symlink
        } else if file_type.is_file() {
            PackageFileKind::File
        } else if file_type.is_dir() {
            PackageFileKind::Directory
        } else {
            PackageFileKind::Special
        };
        Ok(PackageFileMetadata {
            kind,
            len: metadata.len(),
        })
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    fn read_bounded(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

#[derive(Debug)]
struct ReadBudget {
    bytes: u64,
}

impl ReadBudget {
    fn new() -> Self {
        Self { bytes: 0 }
    }

    fn account(&mut self, bytes: usize) -> Result<(), PackageLoadError> {
        let bytes = u64::try_from(bytes).map_err(|_| {
            PackageLoadError::new(
                PackageErrorKind::TotalReadLimit,
                "candidate byte count does not fit in u64",
            )
        })?;
        let total = self.bytes.checked_add(bytes).ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::TotalReadLimit,
                "candidate byte count overflowed",
            )
        })?;
        if total > MAX_CANDIDATE_BYTES {
            return Err(PackageLoadError::new(
                PackageErrorKind::TotalReadLimit,
                format!("candidate read would exceed {MAX_CANDIDATE_BYTES} bytes"),
            ));
        }
        self.bytes = total;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RawSchema {
    version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawManifestV1 {
    version: u32,
    id: String,
    surfaces: Vec<RawSurfaceTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawManifestV2 {
    version: u32,
    package: RawPackageMetadata,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
    #[serde(default)]
    components: Vec<RawComponentExport>,
    surfaces: Option<Vec<RawSurfaceTemplate>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPackageMetadata {
    id: String,
    kind: String,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawDependency {
    alias: String,
    id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawComponentExport {
    name: String,
    source: String,
    #[serde(default)]
    inputs: Vec<RawComponentInput>,
    #[serde(default)]
    slots: Vec<RawComponentSlot>,
    #[serde(default)]
    styles: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawComponentInput {
    name: String,
    #[serde(rename = "type")]
    input_type: String,
    required: Option<bool>,
    default: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawComponentSlot {
    name: String,
    required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum RawSurfaceTemplate {
    Panel(RawPanelTemplate),
    Overlay(RawOverlayTemplate),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPanelTemplate {
    id: String,
    document: String,
    outputs: RawOutputScope,
    edge: RawPanelEdge,
    thickness: u32,
    reserve_space: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawOverlayTemplate {
    id: String,
    document: String,
    outputs: RawOutputScope,
    initially_open: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawOutputScope {
    All,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawPanelEdge {
    Top,
}

#[derive(Debug)]
struct ParsedPackage {
    id: PackageId,
    kind: PackageKind,
    version: Option<PackageVersion>,
    schema: PackageSchemaSource,
    compatibility_normalized: bool,
    dependencies: Vec<PackageDependency>,
    components: Vec<ComponentExport>,
    topology: Option<ShellManifest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Resolved,
}

#[derive(Debug)]
struct SeenLocation {
    state: VisitState,
    id: PackageId,
}

struct GraphBuilder<'a> {
    file_system: &'a dyn ReadOnlyPackageFileSystem,
    composition_root: PathBuf,
    budget: ReadBudget,
    locations: BTreeMap<PathBuf, SeenLocation>,
    ids: BTreeMap<PackageId, (PathBuf, Option<PackageVersion>)>,
    ordered: Vec<Arc<ResolvedPackage>>,
    stack: Vec<PackageId>,
    root_topology: Option<ShellManifest>,
    root_parse_us: u64,
    root_validation_us: u64,
}

impl<'a> GraphBuilder<'a> {
    fn new(file_system: &'a dyn ReadOnlyPackageFileSystem, composition_root: PathBuf) -> Self {
        Self {
            file_system,
            composition_root,
            budget: ReadBudget::new(),
            locations: BTreeMap::new(),
            ids: BTreeMap::new(),
            ordered: Vec::new(),
            stack: Vec::new(),
            root_topology: None,
            root_parse_us: 0,
            root_validation_us: 0,
        }
    }

    fn resolve(
        &mut self,
        package_root: PathBuf,
        manifest_path: PathBuf,
        expected_id: Option<&PackageId>,
        root: bool,
        depth: usize,
    ) -> Result<PackageId, PackageLoadError> {
        if depth > MAX_DEPENDENCY_DEPTH {
            return Err(PackageLoadError::new(
                PackageErrorKind::DependencyDepthLimit,
                format!("dependency depth exceeds {MAX_DEPENDENCY_DEPTH}"),
            ));
        }
        if let Some(seen) = self.locations.get(&package_root) {
            if let Some(expected_id) = expected_id
                && &seen.id != expected_id
            {
                return Err(PackageLoadError::new(
                    PackageErrorKind::PackageLocationConflict,
                    format!(
                        "one package location claims `{}` and `{expected_id}`",
                        seen.id
                    ),
                )
                .at(logical_location(&self.composition_root, &package_root)));
            }
            if seen.state == VisitState::Visiting {
                let mut cycle: Vec<_> = self.stack.iter().map(ToString::to_string).collect();
                cycle.push(seen.id.to_string());
                return Err(PackageLoadError::new(
                    PackageErrorKind::DependencyCycle,
                    format!("dependency cycle: {}", cycle.join(" -> ")),
                ));
            }
            return Ok(seen.id.clone());
        }
        if self.locations.len() >= MAX_PACKAGES_PER_GRAPH {
            return Err(PackageLoadError::new(
                PackageErrorKind::PackageCountLimit,
                format!("package graph exceeds {MAX_PACKAGES_PER_GRAPH} packages"),
            ));
        }

        let parse_started = Instant::now();
        let bytes = read_manifest(
            self.file_system,
            &manifest_path,
            &mut self.budget,
            logical_location(&self.composition_root, &manifest_path),
        )?;
        let schema: RawSchema =
            serde_json::from_slice(&bytes).map_err(|error| json_error(error, &manifest_path))?;
        let parse_us = elapsed_us(parse_started);
        let validation_started = Instant::now();
        let parsed = match schema.version {
            SCHEMA_V1 => {
                let raw: RawManifestV1 = serde_json::from_slice(&bytes)
                    .map_err(|error| json_error(error, &manifest_path))?;
                self.normalize_v1(raw, &package_root)?
            }
            SCHEMA_V2 => {
                let raw: RawManifestV2 = serde_json::from_slice(&bytes)
                    .map_err(|error| json_error(error, &manifest_path))?;
                self.normalize_v2(raw, &package_root)?
            }
            version => {
                return Err(PackageLoadError::new(
                    PackageErrorKind::UnsupportedSchema,
                    format!(
                        "unsupported schema version {version}; expected {SCHEMA_V1} or {SCHEMA_V2}"
                    ),
                ));
            }
        };
        let validation_us = elapsed_us(validation_started);
        if root {
            self.root_parse_us = parse_us;
            self.root_validation_us = validation_us;
        }
        if let Some(expected_id) = expected_id
            && &parsed.id != expected_id
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::DependencyIdMismatch,
                format!(
                    "dependency expected `{expected_id}` but manifest declares `{}`",
                    parsed.id
                ),
            )
            .at(logical_location(&self.composition_root, &package_root)));
        }
        if root {
            if parsed.kind != PackageKind::Shell {
                return Err(PackageLoadError::new(
                    PackageErrorKind::InvalidPackageKind,
                    "the package graph root must be a shell package",
                )
                .in_package(parsed.id.to_string()));
            }
            self.root_topology = parsed.topology.clone();
        } else if parsed.kind != PackageKind::Library {
            return Err(PackageLoadError::new(
                PackageErrorKind::ImportedShellPackage,
                "a dependency must be a library package",
            )
            .in_package(parsed.id.to_string()));
        }

        if let Some((prior_location, prior_version)) = self.ids.get(&parsed.id) {
            let kind = if prior_version != &parsed.version {
                PackageErrorKind::PackageVersionConflict
            } else {
                PackageErrorKind::DuplicatePackageId
            };
            return Err(PackageLoadError::new(
                kind,
                format!(
                    "package ID `{}` is claimed by `{}` and `{}`",
                    parsed.id,
                    logical_location(&self.composition_root, prior_location),
                    logical_location(&self.composition_root, &package_root)
                ),
            ));
        }

        self.ids.insert(
            parsed.id.clone(),
            (package_root.clone(), parsed.version.clone()),
        );
        self.locations.insert(
            package_root.clone(),
            SeenLocation {
                state: VisitState::Visiting,
                id: parsed.id.clone(),
            },
        );
        self.stack.push(parsed.id.clone());

        let mut resolved_dependencies = Vec::with_capacity(parsed.dependencies.len());
        for dependency in &parsed.dependencies {
            if dependency.expected_id == parsed.id {
                return Err(PackageLoadError::new(
                    PackageErrorKind::DependencyCycle,
                    format!("package `{}` cannot depend on itself", parsed.id),
                ));
            }
            let dependency_root = resolve_dependency_root(
                self.file_system,
                &self.composition_root,
                &package_root,
                dependency,
            )?;
            let target = self.resolve(
                dependency_root.clone(),
                dependency_root.join(PACKAGE_MANIFEST_FILE),
                Some(&dependency.expected_id),
                false,
                depth + 1,
            )?;
            resolved_dependencies.push(ResolvedPackageDependency {
                alias: dependency.alias.clone(),
                target,
            });
        }

        self.stack.pop();
        let seen = self
            .locations
            .get_mut(&package_root)
            .expect("location inserted before dependency traversal");
        seen.state = VisitState::Resolved;
        let resolved = Arc::new(ResolvedPackage {
            id: parsed.id.clone(),
            kind: parsed.kind,
            version: parsed.version,
            schema: parsed.schema,
            compatibility_normalized: parsed.compatibility_normalized,
            logical_location: logical_location(&self.composition_root, &package_root),
            canonical_root: package_root,
            manifest_source: Some(manifest_path),
            dependencies: resolved_dependencies,
            components: parsed.components,
        });
        self.ordered.push(resolved);
        Ok(parsed.id)
    }

    fn normalize_v1(
        &mut self,
        raw: RawManifestV1,
        package_root: &Path,
    ) -> Result<ParsedPackage, PackageLoadError> {
        if raw.version != SCHEMA_V1 {
            return Err(PackageLoadError::new(
                PackageErrorKind::UnsupportedSchema,
                "schema-v1 parser received another schema",
            ));
        }
        validate_v1_id("manifest id", &raw.id)?;
        let package_id = PackageId::compatibility(format!("local.{}", raw.id))?;
        let topology = self.validate_topology(raw.version, &raw.id, raw.surfaces, package_root)?;
        Ok(ParsedPackage {
            id: package_id,
            kind: PackageKind::Shell,
            version: None,
            schema: PackageSchemaSource::SchemaV1,
            compatibility_normalized: true,
            dependencies: Vec::new(),
            components: Vec::new(),
            topology: Some(topology),
        })
    }

    fn normalize_v2(
        &mut self,
        raw: RawManifestV2,
        package_root: &Path,
    ) -> Result<ParsedPackage, PackageLoadError> {
        if raw.version != SCHEMA_V2 {
            return Err(PackageLoadError::new(
                PackageErrorKind::UnsupportedSchema,
                "schema-v2 parser received another schema",
            ));
        }
        let id = PackageId::parse(&raw.package.id)?;
        let kind = match raw.package.kind.as_str() {
            "shell" => PackageKind::Shell,
            "library" => PackageKind::Library,
            value => {
                return Err(PackageLoadError::new(
                    PackageErrorKind::InvalidPackageKind,
                    format!("package kind `{value}` must be `shell` or `library`"),
                )
                .in_package(id.to_string()));
            }
        };
        let version = raw
            .package
            .version
            .as_deref()
            .map(PackageVersion::parse)
            .transpose()?;
        let dependencies = validate_dependencies(raw.dependencies, &id)?;
        let components = validate_component_exports(raw.components, &id)?;
        let topology = match (kind, raw.surfaces) {
            (PackageKind::Shell, Some(surfaces)) => {
                Some(self.validate_topology(raw.version, id.as_str(), surfaces, package_root)?)
            }
            (PackageKind::Shell, None) => {
                return Err(PackageLoadError::new(
                    PackageErrorKind::RootTopologyFailure,
                    "schema-v2 shell package must declare surfaces",
                )
                .in_package(id.to_string()));
            }
            (PackageKind::Library, Some(_)) => {
                return Err(PackageLoadError::new(
                    PackageErrorKind::LibraryTopologyViolation,
                    "library package must not declare surfaces",
                )
                .in_package(id.to_string()));
            }
            (PackageKind::Library, None) => None,
        };
        Ok(ParsedPackage {
            id,
            kind,
            version,
            schema: PackageSchemaSource::SchemaV2,
            compatibility_normalized: false,
            dependencies,
            components,
            topology,
        })
    }

    fn validate_topology(
        &mut self,
        schema: u32,
        shell_id: &str,
        raw_surfaces: Vec<RawSurfaceTemplate>,
        package_root: &Path,
    ) -> Result<ShellManifest, PackageLoadError> {
        if raw_surfaces.is_empty() {
            return Err(PackageLoadError::new(
                PackageErrorKind::RootTopologyFailure,
                "surfaces must not be empty",
            ));
        }
        if raw_surfaces.len() > MAX_SURFACE_TEMPLATES {
            return Err(PackageLoadError::new(
                PackageErrorKind::RootTopologyFailure,
                format!(
                    "manifest has {} surfaces; limit is {MAX_SURFACE_TEMPLATES}",
                    raw_surfaces.len()
                ),
            ));
        }
        let mut ids = BTreeSet::new();
        let mut panel_count = 0usize;
        let mut overlay_count = 0usize;
        let mut surfaces = Vec::with_capacity(raw_surfaces.len());
        for raw_surface in raw_surfaces {
            let (id, document, outputs, preset) = match raw_surface {
                RawSurfaceTemplate::Panel(panel) => {
                    panel_count += 1;
                    if panel.thickness == 0 || panel.thickness > MAX_PANEL_THICKNESS {
                        return Err(PackageLoadError::new(
                            PackageErrorKind::RootTopologyFailure,
                            format!(
                                "surface `{}` thickness {} is outside 1..={MAX_PANEL_THICKNESS}",
                                panel.id, panel.thickness
                            ),
                        ));
                    }
                    (
                        panel.id,
                        panel.document,
                        output_scope(panel.outputs),
                        SurfacePreset::Panel(PanelTemplate {
                            edge: match panel.edge {
                                RawPanelEdge::Top => PanelEdge::Top,
                            },
                            thickness: panel.thickness,
                            reserve_space: panel.reserve_space,
                        }),
                    )
                }
                RawSurfaceTemplate::Overlay(overlay) => {
                    overlay_count += 1;
                    (
                        overlay.id,
                        overlay.document,
                        output_scope(overlay.outputs),
                        SurfacePreset::Overlay(OverlayTemplate {
                            initially_open: overlay.initially_open,
                        }),
                    )
                }
            };
            validate_v1_id("surface id", &id)?;
            if !ids.insert(id.clone()) {
                return Err(PackageLoadError::new(
                    PackageErrorKind::RootTopologyFailure,
                    format!("duplicate surface id `{id}`"),
                ));
            }
            let relative = validate_document_path(&id, &document)?;
            let requested = package_root.join(&relative);
            let canonical_document =
                self.file_system.canonicalize(&requested).map_err(|error| {
                    io_package_error(
                        PackageErrorKind::EntryDocument,
                        "resolve surface document",
                        &relative,
                        error,
                    )
                })?;
            if !canonical_document.starts_with(package_root) {
                return Err(PackageLoadError::new(
                    PackageErrorKind::DependencyEscape,
                    format!("surface `{id}` document resolves outside the package"),
                )
                .at(path_to_logical(&relative)));
            }
            let html = read_text_file(
                self.file_system,
                &canonical_document,
                MAX_PACKAGE_HTML_BYTES,
                &mut self.budget,
                PackageErrorKind::EntryDocument,
                &path_to_logical(&relative),
            )?;
            let namespace = format!("htmshell-{shell_id}-{id}");
            surfaces.push(SurfaceTemplate {
                id,
                document: relative,
                canonical_document,
                html: Arc::from(html),
                prepared_document: None,
                outputs,
                preset,
                namespace,
            });
        }
        if panel_count != 1 || overlay_count != 1 {
            return Err(PackageLoadError::new(
                PackageErrorKind::RootTopologyFailure,
                format!(
                    "schema version {schema} requires exactly one panel and one overlay; found {panel_count} panel(s) and {overlay_count} overlay(s)"
                ),
            ));
        }
        surfaces.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(ShellManifest {
            version: schema,
            id: shell_id.to_owned(),
            surfaces,
        })
    }

    fn load_component_catalog(&mut self) -> Result<ComponentCatalog, PackageLoadError> {
        let packages = self.ordered.clone();
        let export_count = packages
            .iter()
            .try_fold(0usize, |total, package| {
                total.checked_add(package.components.len())
            })
            .ok_or_else(|| {
                PackageLoadError::new(
                    PackageErrorKind::ComponentGraphExportLimit,
                    "component graph export count overflowed",
                )
            })?;
        if export_count > crate::MAX_COMPONENT_EXPORTS_PER_GRAPH {
            return Err(PackageLoadError::new(
                PackageErrorKind::ComponentGraphExportLimit,
                format!(
                    "component graph contains {export_count} exports; limit is {}",
                    crate::MAX_COMPONENT_EXPORTS_PER_GRAPH
                ),
            ));
        }

        let mut unresolved = Vec::with_capacity(export_count);
        let mut totals = ComponentValidationTotals {
            export_count,
            ..Default::default()
        };
        for package in &packages {
            let mut source_order = Vec::new();
            let mut expected_by_source: BTreeMap<
                String,
                BTreeMap<ComponentName, ExpectedComponentDefinition>,
            > = BTreeMap::new();
            for export in package.components() {
                if !expected_by_source.contains_key(export.source()) {
                    source_order.push(export.source().to_owned());
                }
                expected_by_source
                    .entry(export.source().to_owned())
                    .or_default()
                    .insert(
                        export.name().clone(),
                        ExpectedComponentDefinition {
                            inputs: export.inputs().to_vec().into(),
                            slots: export.slots().to_vec().into(),
                        },
                    );
            }
            let mut parsed_by_source = BTreeMap::new();
            for source in source_order {
                let expected = expected_by_source
                    .get(&source)
                    .expect("component source order is derived from source map");
                let html = self.read_component_source(package, &source)?;
                totals.source_document_count =
                    totals.source_document_count.checked_add(1).ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentGraphExportLimit,
                            "component source document count overflowed",
                        )
                    })?;
                totals.source_read_count = totals.source_read_count.saturating_add(1);
                totals.source_parse_count = totals.source_parse_count.saturating_add(1);
                let parsed = parse_component_source(&html, package.id(), &source, expected)?;
                parsed_by_source.insert(source, parsed);
            }
            for export in package.components() {
                let definition = parsed_by_source
                    .get_mut(export.source())
                    .and_then(|source| source.remove(export.name()))
                    .ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentTemplateMissing,
                            format!(
                                "component export `{}` has no matching parsed template",
                                export.name()
                            ),
                        )
                        .in_package(package.id().to_string())
                        .at(export.source())
                    })?;
                totals.source_node_count = totals
                    .source_node_count
                    .checked_add(definition.source_node_count)
                    .ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentSourceNodeLimit,
                            "component source node total overflowed",
                        )
                    })?;
                unresolved.push(definition);
            }
        }
        build_component_catalog(&packages, unresolved, totals)
    }

    fn load_component_style_catalog(
        &mut self,
        components: &ComponentCatalog,
    ) -> Result<ComponentStyleCatalog, PackageLoadError> {
        let packages = self.ordered.clone();
        let mut sources = Vec::new();
        let mut associations = Vec::new();
        let mut totals = ComponentStyleValidationTotals::default();
        for package in &packages {
            let unique_paths = package
                .components()
                .iter()
                .flat_map(ComponentExport::styles)
                .cloned()
                .collect::<BTreeSet<_>>();
            if unique_paths.len() > MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentStylesheetPackageFileLimit,
                    format!(
                        "package `{}` declares {} unique component stylesheet files; limit is {MAX_COMPONENT_STYLESHEET_FILES_PER_PACKAGE}",
                        package.id(),
                        unique_paths.len()
                    ),
                )
                .in_package(package.id().to_string()));
            }

            let mut loaded =
                BTreeMap::<ComponentStylesheetPath, Arc<ComponentStylesheetSource>>::new();
            for path in unique_paths {
                let css = self.read_component_stylesheet(package, &path)?;
                let declaring_component = package
                    .components()
                    .iter()
                    .find(|export| export.styles().contains(&path))
                    .map(ComponentExport::name);
                let parsed =
                    prepare_component_author_stylesheet(&css, path.as_str()).map_err(|error| {
                        component_stylesheet_css_error(package, declaring_component, &path, error)
                    })?;
                let source = Arc::new(ComponentStylesheetSource::new(
                    ComponentStylesheetSourceKey::new(package.id().clone(), path.clone()),
                    ComponentStylesheetSemanticVersion::new(parsed.semantic_version()),
                    parsed,
                    css.len() as u64,
                ));
                totals.source_count = totals.source_count.saturating_add(1);
                totals.source_read_count = totals.source_read_count.saturating_add(1);
                totals.source_parse_count = totals.source_parse_count.saturating_add(1);
                totals.bytes_read = totals.bytes_read.saturating_add(css.len() as u64);
                loaded.insert(path, Arc::clone(&source));
                sources.push(source);
            }

            for export in package.components() {
                let definition =
                    crate::ComponentDefinitionKey::new(package.id().clone(), export.name().clone());
                if components.definition(&definition).is_none() {
                    return Err(PackageLoadError::new(
                        PackageErrorKind::ComponentStylesheetAssociationInvalid,
                        format!(
                            "component stylesheet association targets missing definition `{definition}`"
                        ),
                    )
                    .in_package(package.id().to_string()));
                }
                for (ordinal, path) in export.styles().iter().enumerate() {
                    let source = loaded.get(path).ok_or_else(|| {
                        PackageLoadError::new(
                            PackageErrorKind::ComponentStylesheetAssociationInvalid,
                            format!(
                                "component `{}` stylesheet `{path}` was not loaded",
                                export.name()
                            ),
                        )
                        .in_package(package.id().to_string())
                        .at(path.as_str())
                    })?;
                    associations.push(ComponentStylesheetAssociation::new(
                        definition.clone(),
                        Arc::clone(source),
                        u16::try_from(ordinal).expect("component stylesheet limit fits u16"),
                    ));
                    totals.association_count = totals.association_count.saturating_add(1);
                }
            }
        }
        Ok(ComponentStyleCatalog::new(sources, associations, totals))
    }

    fn read_component_stylesheet(
        &mut self,
        package: &ResolvedPackage,
        logical_path: &ComponentStylesheetPath,
    ) -> Result<String, PackageLoadError> {
        let logical = logical_path.as_str();
        let mut requested = package.canonical_root().to_path_buf();
        let components = logical.split('/').collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            requested.push(component);
            let metadata = self.file_system.metadata(&requested).map_err(|error| {
                let kind = if error.kind() == io::ErrorKind::NotFound {
                    PackageErrorKind::ComponentStylesheetMissing
                } else {
                    PackageErrorKind::ComponentStylesheetReadFailure
                };
                io_package_error(
                    kind,
                    "inspect component stylesheet path",
                    Path::new(logical),
                    error,
                )
                .in_package(package.id().to_string())
            })?;
            if metadata.kind == PackageFileKind::Symlink {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentStylesheetSymlink,
                    "component stylesheet path contains a symbolic link",
                )
                .in_package(package.id().to_string())
                .at(logical));
            }
            let final_component = index + 1 == components.len();
            let expected_kind = if final_component {
                PackageFileKind::File
            } else {
                PackageFileKind::Directory
            };
            if metadata.kind != expected_kind {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentStylesheetSpecialFile,
                    if final_component {
                        "component stylesheet is not a regular file"
                    } else {
                        "component stylesheet path component is not a directory"
                    },
                )
                .in_package(package.id().to_string())
                .at(logical));
            }
            if final_component && metadata.len > MAX_COMPONENT_STYLESHEET_BYTES {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentStylesheetTooLarge,
                    format!(
                        "component stylesheet is {} bytes; limit is {MAX_COMPONENT_STYLESHEET_BYTES}",
                        metadata.len
                    ),
                )
                .in_package(package.id().to_string())
                .at(logical));
            }
        }
        let canonical = self.file_system.canonicalize(&requested).map_err(|error| {
            io_package_error(
                PackageErrorKind::ComponentStylesheetMissing,
                "resolve component stylesheet",
                Path::new(logical),
                error,
            )
            .in_package(package.id().to_string())
        })?;
        if !canonical.starts_with(package.canonical_root())
            || !canonical.starts_with(&self.composition_root)
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentStylesheetPath,
                "component stylesheet resolves outside its owning package",
            )
            .in_package(package.id().to_string())
            .at(logical));
        }
        let bytes = self
            .file_system
            .read_bounded(&canonical, MAX_COMPONENT_STYLESHEET_BYTES)
            .map_err(|error| {
                io_package_error(
                    PackageErrorKind::ComponentStylesheetReadFailure,
                    "read component stylesheet",
                    Path::new(logical),
                    error,
                )
                .in_package(package.id().to_string())
            })?;
        if bytes.len() as u64 > MAX_COMPONENT_STYLESHEET_BYTES {
            return Err(PackageLoadError::new(
                PackageErrorKind::ComponentStylesheetTooLarge,
                format!("component stylesheet exceeds {MAX_COMPONENT_STYLESHEET_BYTES} bytes"),
            )
            .in_package(package.id().to_string())
            .at(logical));
        }
        self.budget.account(bytes.len())?;
        String::from_utf8(bytes).map_err(|_| {
            PackageLoadError::new(
                PackageErrorKind::ComponentStylesheetReadFailure,
                "component stylesheet is not UTF-8",
            )
            .in_package(package.id().to_string())
            .at(logical)
        })
    }

    fn read_component_source(
        &mut self,
        package: &ResolvedPackage,
        logical_source: &str,
    ) -> Result<String, PackageLoadError> {
        let mut requested = package.canonical_root().to_path_buf();
        let components: Vec<_> = logical_source.split('/').collect();
        for (index, component) in components.iter().enumerate() {
            requested.push(component);
            let metadata = self.file_system.metadata(&requested).map_err(|error| {
                io_package_error(
                    PackageErrorKind::ComponentSourceMissing,
                    "inspect component source path",
                    Path::new(logical_source),
                    error,
                )
                .in_package(package.id().to_string())
            })?;
            if metadata.kind == PackageFileKind::Symlink {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentSourceSymlink,
                    "component source path contains a symbolic link",
                )
                .in_package(package.id().to_string())
                .at(logical_source));
            }
            let final_component = index + 1 == components.len();
            let expected_kind = if final_component {
                PackageFileKind::File
            } else {
                PackageFileKind::Directory
            };
            if metadata.kind != expected_kind {
                return Err(PackageLoadError::new(
                    PackageErrorKind::ComponentSourceInvalidType,
                    if final_component {
                        "component source is not a regular file"
                    } else {
                        "component source path component is not a directory"
                    },
                )
                .in_package(package.id().to_string())
                .at(logical_source));
            }
        }
        let canonical = self.file_system.canonicalize(&requested).map_err(|error| {
            io_package_error(
                PackageErrorKind::ComponentSourceMissing,
                "resolve component source",
                Path::new(logical_source),
                error,
            )
            .in_package(package.id().to_string())
        })?;
        if !canonical.starts_with(package.canonical_root())
            || !canonical.starts_with(&self.composition_root)
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::DependencyEscape,
                "component source resolves outside its owning package",
            )
            .in_package(package.id().to_string())
            .at(logical_source));
        }
        read_text_file(
            self.file_system,
            &canonical,
            MAX_COMPONENT_SOURCE_BYTES,
            &mut self.budget,
            PackageErrorKind::ComponentSourceTooLarge,
            logical_source,
        )
        .map_err(|error| error.in_package(package.id().to_string()))
    }

    fn finish(
        mut self,
        mut headless_entry: Option<PackageEntryDocument>,
    ) -> Result<PackageSnapshotCandidate, PackageLoadError> {
        let root_index = self.ordered.len().checked_sub(1).ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::MissingRoot,
                "package graph contains no root",
            )
        })?;
        let components = self.load_component_catalog()?;
        let component_styles = self.load_component_style_catalog(&components)?;
        let root_package = Arc::clone(&self.ordered[root_index]);
        if let Some(manifest) = self.root_topology.as_mut() {
            for surface in &mut manifest.surfaces {
                let mut prepared = prepare_root_document(
                    surface.html(),
                    &path_to_logical(surface.document()),
                    &root_package,
                    &components,
                )?;
                prepared.select_style_matching_mode(
                    component_styles.has_reachable_styles(prepared.referenced_definition_keys()),
                );
                surface.prepared_document = Some(Arc::new(prepared));
            }
        }
        if let Some(entry) = headless_entry.as_mut() {
            let mut prepared = prepare_root_document(
                entry.html(),
                &path_to_logical(entry.logical_path()),
                &root_package,
                &components,
            )?;
            prepared.select_style_matching_mode(
                component_styles.has_reachable_styles(prepared.referenced_definition_keys()),
            );
            entry.prepared_document = Some(Arc::new(prepared));
        }
        Ok(PackageSnapshotCandidate {
            composition_root: self.composition_root,
            packages: self.ordered,
            root_index,
            root_manifest: self.root_topology,
            headless_entry,
            components,
            component_styles,
            bytes_read: self.budget.bytes,
            measurements: ManifestMeasurements {
                parse_us: self.root_parse_us,
                validation_us: self.root_validation_us,
            },
        })
    }
}

fn build_manifest_candidate(
    file_system: &dyn ReadOnlyPackageFileSystem,
    manifest_path: &Path,
    headless: bool,
) -> Result<PackageSnapshotCandidate, PackageLoadError> {
    ensure_existing_path_has_no_symlink(file_system, manifest_path, true)?;
    let manifest_metadata = file_system.metadata(manifest_path).map_err(|error| {
        io_package_error(
            PackageErrorKind::ManifestMissing,
            "inspect manifest",
            Path::new(PACKAGE_MANIFEST_FILE),
            error,
        )
    })?;
    match manifest_metadata.kind {
        PackageFileKind::File => {}
        PackageFileKind::Symlink => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSymlink,
                "manifest must not be a symbolic link",
            )
            .at(PACKAGE_MANIFEST_FILE));
        }
        _ => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSpecialFile,
                "manifest is not a regular file",
            )
            .at(PACKAGE_MANIFEST_FILE));
        }
    }
    let requested_root = manifest_path.parent().ok_or_else(|| {
        PackageLoadError::new(
            PackageErrorKind::MissingRoot,
            "manifest has no package directory",
        )
    })?;
    ensure_existing_path_has_no_symlink(file_system, requested_root, false)?;
    let composition_root = file_system.canonicalize(requested_root).map_err(|error| {
        io_package_error(
            PackageErrorKind::MissingRoot,
            "resolve composition root",
            Path::new("."),
            error,
        )
    })?;
    let root_metadata = file_system.metadata(&composition_root).map_err(|error| {
        io_package_error(
            PackageErrorKind::MissingRoot,
            "inspect composition root",
            Path::new("."),
            error,
        )
    })?;
    if root_metadata.kind != PackageFileKind::Directory {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidRootType,
            "composition root is not a directory",
        ));
    }
    let canonical_manifest = file_system.canonicalize(manifest_path).map_err(|error| {
        io_package_error(
            PackageErrorKind::ManifestMissing,
            "resolve manifest",
            Path::new(PACKAGE_MANIFEST_FILE),
            error,
        )
    })?;
    if canonical_manifest.parent() != Some(composition_root.as_path()) {
        return Err(PackageLoadError::new(
            PackageErrorKind::DependencyEscape,
            "root manifest resolves outside the composition root",
        ));
    }
    let mut builder = GraphBuilder::new(file_system, composition_root.clone());
    builder.resolve(composition_root.clone(), canonical_manifest, None, true, 0)?;
    let headless_entry = if headless {
        Some(load_headless_entry(
            file_system,
            &composition_root,
            &mut builder.budget,
        )?)
    } else {
        None
    };
    builder.finish(headless_entry)
}

fn build_headless_candidate(
    file_system: &dyn ReadOnlyPackageFileSystem,
    package_root: &Path,
) -> Result<PackageSnapshotCandidate, PackageLoadError> {
    ensure_existing_path_has_no_symlink(file_system, package_root, false)?;
    let root = file_system.canonicalize(package_root).map_err(|error| {
        io_package_error(
            PackageErrorKind::MissingRoot,
            "resolve headless package root",
            Path::new("."),
            error,
        )
    })?;
    let metadata = file_system.metadata(&root).map_err(|error| {
        io_package_error(
            PackageErrorKind::MissingRoot,
            "inspect headless package root",
            Path::new("."),
            error,
        )
    })?;
    if metadata.kind != PackageFileKind::Directory {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidRootType,
            "headless package root is not a directory",
        ));
    }
    let manifest_path = root.join(PACKAGE_MANIFEST_FILE);
    match file_system.metadata(&manifest_path) {
        Ok(_) => build_manifest_candidate(file_system, &manifest_path, true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut budget = ReadBudget::new();
            let entry = load_headless_entry(file_system, &root, &mut budget)?;
            let id = PackageId::compatibility(LEGACY_HEADLESS_ID.to_owned())?;
            let package = Arc::new(ResolvedPackage {
                id,
                kind: PackageKind::Shell,
                version: None,
                schema: PackageSchemaSource::LegacyHeadless,
                compatibility_normalized: true,
                logical_location: ".".into(),
                canonical_root: root.clone(),
                manifest_source: None,
                dependencies: Vec::new(),
                components: Vec::new(),
            });
            let components = ComponentCatalog::empty();
            let prepared = Arc::new(prepare_root_document(
                entry.html(),
                "index.html",
                &package,
                &components,
            )?);
            let mut entry = entry;
            entry.prepared_document = Some(prepared);
            Ok(PackageSnapshotCandidate {
                composition_root: root,
                packages: vec![package],
                root_index: 0,
                root_manifest: None,
                headless_entry: Some(entry),
                components,
                component_styles: ComponentStyleCatalog::empty(),
                bytes_read: budget.bytes,
                measurements: ManifestMeasurements::default(),
            })
        }
        Err(error) => Err(io_package_error(
            PackageErrorKind::ManifestIo,
            "inspect optional headless manifest",
            Path::new(PACKAGE_MANIFEST_FILE),
            error,
        )),
    }
}

fn load_headless_entry(
    file_system: &dyn ReadOnlyPackageFileSystem,
    root: &Path,
    budget: &mut ReadBudget,
) -> Result<PackageEntryDocument, PackageLoadError> {
    let logical = PathBuf::from("index.html");
    let requested = root.join(&logical);
    let canonical = file_system.canonicalize(&requested).map_err(|error| {
        io_package_error(
            PackageErrorKind::EntryDocument,
            "resolve headless index.html",
            &logical,
            error,
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(PackageLoadError::new(
            PackageErrorKind::DependencyEscape,
            "headless index.html resolves outside the package root",
        )
        .at("index.html"));
    }
    let html = read_text_file(
        file_system,
        &canonical,
        MAX_PACKAGE_HTML_BYTES,
        budget,
        PackageErrorKind::EntryDocument,
        "index.html",
    )?;
    Ok(PackageEntryDocument {
        logical_path: logical,
        canonical_path: canonical,
        html: Arc::from(html),
        prepared_document: None,
    })
}

fn validate_dependencies(
    raw: Vec<RawDependency>,
    owner: &PackageId,
) -> Result<Vec<PackageDependency>, PackageLoadError> {
    if raw.len() > MAX_DIRECT_DEPENDENCIES {
        return Err(PackageLoadError::new(
            PackageErrorKind::DirectDependencyLimit,
            format!(
                "package `{owner}` has {} dependencies; limit is {MAX_DIRECT_DEPENDENCIES}",
                raw.len()
            ),
        ));
    }
    let mut aliases = BTreeSet::new();
    let mut declarations = BTreeSet::new();
    let mut dependencies = Vec::with_capacity(raw.len());
    for dependency in raw {
        let alias = PackageAlias::parse(&dependency.alias)?;
        let expected_id = PackageId::parse(&dependency.id)?;
        let logical_path = validate_dependency_path(&dependency.path)?;
        if !aliases.insert(alias.clone()) {
            return Err(PackageLoadError::new(
                PackageErrorKind::DuplicateAlias,
                format!("duplicate dependency alias `{alias}`"),
            )
            .in_package(owner.to_string()));
        }
        if !declarations.insert((alias.clone(), expected_id.clone(), logical_path.clone())) {
            return Err(PackageLoadError::new(
                PackageErrorKind::DuplicateAlias,
                format!("duplicate dependency declaration `{alias}`"),
            )
            .in_package(owner.to_string()));
        }
        dependencies.push(PackageDependency {
            alias,
            expected_id,
            logical_path,
        });
    }
    Ok(dependencies)
}

fn validate_component_exports(
    raw: Vec<RawComponentExport>,
    owner: &PackageId,
) -> Result<Vec<ComponentExport>, PackageLoadError> {
    if raw.len() > MAX_COMPONENT_EXPORTS_PER_PACKAGE {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentExport,
            format!(
                "package `{owner}` has {} component exports; limit is {MAX_COMPONENT_EXPORTS_PER_PACKAGE}",
                raw.len()
            ),
        ));
    }
    let mut names = BTreeSet::new();
    let mut exports = Vec::with_capacity(raw.len());
    for raw_export in raw {
        let name = ComponentName::parse(&raw_export.name)
            .map_err(|error| error.in_package(owner.to_string()))?;
        let source = validate_component_source_path(&raw_export.source)
            .map_err(|error| error.in_package(owner.to_string()))?;
        let inputs = validate_component_inputs(raw_export.inputs, owner, &name)?;
        let slots = validate_component_slots(raw_export.slots, owner, &name)?;
        let styles = validate_component_stylesheets(raw_export.styles, owner, &name)?;
        if !names.insert(name.clone()) {
            return Err(PackageLoadError::new(
                PackageErrorKind::DuplicateComponentExport,
                format!("duplicate component export `{name}`"),
            )
            .in_package(owner.to_string()));
        }
        exports.push(ComponentExport::new(name, source, inputs, slots, styles));
    }
    Ok(exports)
}

fn validate_component_stylesheets(
    raw: Vec<serde_json::Value>,
    owner: &PackageId,
    component: &ComponentName,
) -> Result<Vec<ComponentStylesheetPath>, PackageLoadError> {
    if raw.len() > MAX_COMPONENT_STYLESHEETS {
        return Err(PackageLoadError::new(
            PackageErrorKind::ComponentStylesheetDeclarationLimit,
            format!(
                "component `{component}` declares {} stylesheets; limit is {MAX_COMPONENT_STYLESHEETS}",
                raw.len()
            ),
        )
        .in_package(owner.to_string()));
    }
    let mut paths = BTreeSet::new();
    let mut styles = Vec::with_capacity(raw.len());
    for value in raw {
        let value = value.as_str().ok_or_else(|| {
            PackageLoadError::new(
                PackageErrorKind::InvalidComponentStylesheetDeclaration,
                format!("component `{component}` stylesheet declarations must be strings"),
            )
            .in_package(owner.to_string())
        })?;
        let path = validate_component_stylesheet_path(value)
            .map_err(|error| error.in_package(owner.to_string()))?;
        if !paths.insert(path.clone()) {
            return Err(PackageLoadError::new(
                PackageErrorKind::DuplicateComponentStylesheet,
                format!("component `{component}` repeats stylesheet `{path}`"),
            )
            .in_package(owner.to_string())
            .at(path));
        }
        styles.push(ComponentStylesheetPath::new(path));
    }
    Ok(styles)
}

fn validate_component_stylesheet_path(value: &str) -> Result<String, PackageLoadError> {
    if value.is_empty()
        || value.len() > MAX_COMPONENT_STYLESHEET_PATH_BYTES
        || value.contains('\0')
        || value.contains('\\')
        || value.contains("://")
        || value.starts_with("//")
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentStylesheetPath,
            format!(
                "component stylesheet path must contain 1..={MAX_COMPONENT_STYLESHEET_PATH_BYTES} local UTF-8 bytes"
            ),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || value
            .split('/')
            .any(|component| component.is_empty() || component == ".")
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentStylesheetPath,
            "component stylesheet must be a normalized package-relative path",
        ));
    }
    Ok(value.to_owned())
}

fn validate_component_slots(
    raw: Vec<RawComponentSlot>,
    owner: &PackageId,
    component: &ComponentName,
) -> Result<Vec<ComponentSlotDeclaration>, PackageLoadError> {
    if raw.len() > MAX_COMPONENT_SLOTS {
        return Err(PackageLoadError::new(
            PackageErrorKind::ComponentSlotDeclarationLimit,
            format!(
                "component `{component}` declares {} slots; limit is {}",
                raw.len(),
                MAX_COMPONENT_SLOTS
            ),
        )
        .in_package(owner.to_string()));
    }
    let mut names = BTreeSet::new();
    let mut declarations = Vec::with_capacity(raw.len());
    for slot in raw {
        let name = ComponentSlotName::parse(&slot.name)
            .map_err(|error| error.in_package(owner.to_string()))?;
        if !names.insert(name.clone()) {
            return Err(PackageLoadError::new(
                if name.is_default() {
                    PackageErrorKind::DuplicateDefaultComponentSlot
                } else {
                    PackageErrorKind::DuplicateComponentSlotDeclaration
                },
                format!("component `{component}` repeats slot `{name}`"),
            )
            .in_package(owner.to_string()));
        }
        declarations.push(ComponentSlotDeclaration::new(name, slot.required));
    }
    Ok(declarations)
}

fn validate_component_inputs(
    raw: Vec<RawComponentInput>,
    owner: &PackageId,
    component: &ComponentName,
) -> Result<Vec<ComponentInputDeclaration>, PackageLoadError> {
    if raw.len() > MAX_COMPONENT_INPUTS {
        return Err(PackageLoadError::new(
            PackageErrorKind::ComponentInputCountLimit,
            format!(
                "component `{component}` declares {} inputs; limit is {MAX_COMPONENT_INPUTS}",
                raw.len()
            ),
        )
        .in_package(owner.to_string()));
    }
    let mut names = BTreeSet::new();
    let mut declarations = Vec::with_capacity(raw.len());
    for input in raw {
        let name = ComponentInputName::parse(&input.name).map_err(|error| {
            error
                .in_package(owner.to_string())
                .at(format!("component {component} input {}", input.name))
        })?;
        if !names.insert(name.clone()) {
            return Err(PackageLoadError::new(
                PackageErrorKind::DuplicateComponentInputDeclaration,
                format!("component `{component}` repeats input `{name}`"),
            )
            .in_package(owner.to_string()));
        }
        let input_type = ComponentInputType::parse(&input.input_type).map_err(|error| {
            error
                .in_package(owner.to_string())
                .at(format!("component {component} input {name}"))
        })?;
        let required = input.required.unwrap_or(false);
        if required && input.default.is_some() {
            return Err(PackageLoadError::new(
                PackageErrorKind::ComponentInputRequiredWithDefault,
                format!("required component input `{name}` cannot declare a default"),
            )
            .in_package(owner.to_string()));
        }
        if !required && input.default.is_none() {
            return Err(PackageLoadError::new(
                PackageErrorKind::ComponentInputOptionalWithoutDefault,
                format!("optional component input `{name}` requires a default"),
            )
            .in_package(owner.to_string()));
        }
        let default = input
            .default
            .as_ref()
            .map(|value| parse_component_input_default(input_type, value))
            .transpose()
            .map_err(|error| {
                error
                    .in_package(owner.to_string())
                    .at(format!("component {component} input {name}"))
            })?;
        declarations.push(ComponentInputDeclaration::new(
            name, input_type, required, default,
        ));
    }
    Ok(declarations)
}

fn validate_component_source_path(value: &str) -> Result<String, PackageLoadError> {
    if value.is_empty()
        || value.len() > MAX_PACKAGE_PATH_BYTES
        || value.contains('\0')
        || value.contains('\\')
        || value.contains("://")
        || value.starts_with("//")
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentExport,
            format!("component source must contain 1..={MAX_PACKAGE_PATH_BYTES} local UTF-8 bytes"),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || value
            .split('/')
            .any(|component| component.is_empty() || component == ".")
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidComponentExport,
            "component source must be a normalized package-relative path",
        ));
    }
    Ok(value.to_owned())
}

fn validate_package_id(value: &str) -> Result<(), PackageLoadError> {
    if value.is_empty() || value.len() > MAX_PACKAGE_ID_BYTES || !value.is_ascii() {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidPackageId,
            format!("package ID must contain 1..={MAX_PACKAGE_ID_BYTES} ASCII bytes"),
        ));
    }
    let segments: Vec<_> = value.split('.').collect();
    if segments.len() < 2 {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidPackageId,
            "package ID must contain at least two dot-separated segments",
        ));
    }
    for segment in segments {
        if segment.is_empty() || segment.len() > 63 {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidPackageId,
                "package ID segments must contain 1..=63 bytes",
            ));
        }
        let bytes = segment.as_bytes();
        if !bytes[0].is_ascii_lowercase()
            || bytes.last() == Some(&b'-')
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidPackageId,
                format!(
                    "package ID segment `{segment}` must start with a lowercase letter and use lowercase letters, digits, and interior hyphens"
                ),
            ));
        }
    }
    if value.starts_with("local.") {
        return Err(PackageLoadError::new(
            PackageErrorKind::ReservedPackageId,
            "the `local.` package ID prefix is reserved",
        ));
    }
    Ok(())
}

fn validate_v1_id(field: &str, id: &str) -> Result<(), PackageLoadError> {
    if id.is_empty() {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!("{field} must not be empty"),
        ));
    }
    if id.len() > MAX_V1_ID_BYTES {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!("{field} `{id}` exceeds {MAX_V1_ID_BYTES} bytes"),
        ));
    }
    let valid = id.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || (byte == b'-' && index > 0 && index + 1 < id.len())
    });
    if !valid {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!(
                "{field} `{id}` must use lowercase ASCII letters, digits, and interior hyphens"
            ),
        ));
    }
    Ok(())
}

fn validate_dependency_path(value: &str) -> Result<String, PackageLoadError> {
    if value.is_empty()
        || value.len() > MAX_PACKAGE_PATH_BYTES
        || value.contains('\0')
        || value.contains('\\')
        || value.contains("://")
        || value.starts_with("//")
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidDependencyPath,
            format!("dependency path must contain 1..={MAX_PACKAGE_PATH_BYTES} local ASCII bytes"),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidDependencyPath,
            "dependency path must be relative",
        ));
    }
    let mut normalized = Vec::new();
    for raw in value.split('/') {
        if raw.is_empty() {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidDependencyPath,
                "dependency path contains an empty component",
            ));
        }
        if raw == "." {
            continue;
        }
        if raw == ".." {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidDependencyPath,
                "dependency path must not contain parent traversal",
            ));
        }
        normalized.push(raw);
    }
    if normalized.is_empty() {
        return Err(PackageLoadError::new(
            PackageErrorKind::InvalidDependencyPath,
            "dependency path resolves to the declaring package",
        ));
    }
    Ok(normalized.join("/"))
}

fn validate_document_path(surface_id: &str, value: &str) -> Result<PathBuf, PackageLoadError> {
    if value.is_empty() || value.len() > MAX_DOCUMENT_PATH_BYTES {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!(
                "surface `{surface_id}` document path must contain 1..={MAX_DOCUMENT_PATH_BYTES} bytes"
            ),
        ));
    }
    if value.contains("://") || value.starts_with("//") {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!("surface `{surface_id}` document must be a local relative path"),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PackageLoadError::new(
            PackageErrorKind::RootTopologyFailure,
            format!("surface `{surface_id}` document must remain inside the manifest package"),
        ));
    }
    Ok(path.to_path_buf())
}

fn resolve_dependency_root(
    file_system: &dyn ReadOnlyPackageFileSystem,
    composition_root: &Path,
    declaring_root: &Path,
    dependency: &PackageDependency,
) -> Result<PathBuf, PackageLoadError> {
    let mut candidate = declaring_root.to_path_buf();
    for component in dependency.logical_path.split('/') {
        candidate.push(component);
        let metadata = file_system.metadata(&candidate).map_err(|error| {
            let kind = if error.kind() == io::ErrorKind::NotFound {
                PackageErrorKind::DependencyMissing
            } else {
                PackageErrorKind::Io
            };
            io_package_error(
                kind,
                "inspect dependency path",
                Path::new(&dependency.logical_path),
                error,
            )
        })?;
        if metadata.kind == PackageFileKind::Symlink {
            return Err(PackageLoadError::new(
                PackageErrorKind::DependencySymlink,
                "dependency path contains a symbolic link",
            )
            .at(&dependency.logical_path));
        }
        if metadata.kind != PackageFileKind::Directory {
            return Err(PackageLoadError::new(
                PackageErrorKind::SpecialFile,
                "dependency path component is not a directory",
            )
            .at(&dependency.logical_path));
        }
    }
    let canonical = file_system.canonicalize(&candidate).map_err(|error| {
        io_package_error(
            PackageErrorKind::DependencyMissing,
            "resolve dependency directory",
            Path::new(&dependency.logical_path),
            error,
        )
    })?;
    if !canonical.starts_with(composition_root) {
        return Err(PackageLoadError::new(
            PackageErrorKind::DependencyEscape,
            "dependency resolves outside the composition root",
        )
        .at(&dependency.logical_path));
    }
    let manifest = canonical.join(PACKAGE_MANIFEST_FILE);
    let metadata = file_system.metadata(&manifest).map_err(|error| {
        io_package_error(
            PackageErrorKind::DependencyMissing,
            "inspect dependency manifest",
            Path::new(&format!(
                "{}/{}",
                dependency.logical_path, PACKAGE_MANIFEST_FILE
            )),
            error,
        )
    })?;
    match metadata.kind {
        PackageFileKind::File => {}
        PackageFileKind::Symlink => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSymlink,
                "dependency manifest must not be a symbolic link",
            )
            .at(format!(
                "{}/{}",
                dependency.logical_path, PACKAGE_MANIFEST_FILE
            )));
        }
        _ => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSpecialFile,
                "dependency manifest is not a regular file",
            )
            .at(format!(
                "{}/{}",
                dependency.logical_path, PACKAGE_MANIFEST_FILE
            )));
        }
    }
    Ok(canonical)
}

fn read_manifest(
    file_system: &dyn ReadOnlyPackageFileSystem,
    path: &Path,
    budget: &mut ReadBudget,
    logical: String,
) -> Result<Vec<u8>, PackageLoadError> {
    let metadata = file_system.metadata(path).map_err(|error| {
        io_package_error(
            PackageErrorKind::ManifestMissing,
            "inspect package manifest",
            Path::new(&logical),
            error,
        )
    })?;
    match metadata.kind {
        PackageFileKind::File => {}
        PackageFileKind::Symlink => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSymlink,
                "manifest must not be a symbolic link",
            )
            .at(logical));
        }
        _ => {
            return Err(PackageLoadError::new(
                PackageErrorKind::ManifestSpecialFile,
                "manifest is not a regular file",
            )
            .at(logical));
        }
    }
    if metadata.len > MAX_PACKAGE_MANIFEST_BYTES {
        return Err(PackageLoadError::new(
            PackageErrorKind::ManifestTooLarge,
            format!(
                "manifest is {} bytes; limit is {MAX_PACKAGE_MANIFEST_BYTES}",
                metadata.len
            ),
        )
        .at(logical));
    }
    let bytes = file_system
        .read_bounded(path, MAX_PACKAGE_MANIFEST_BYTES)
        .map_err(|error| {
            io_package_error(
                PackageErrorKind::ManifestIo,
                "read package manifest",
                Path::new(&logical),
                error,
            )
        })?;
    if bytes.len() as u64 > MAX_PACKAGE_MANIFEST_BYTES {
        return Err(PackageLoadError::new(
            PackageErrorKind::ManifestTooLarge,
            format!("manifest exceeds {MAX_PACKAGE_MANIFEST_BYTES} bytes"),
        )
        .at(logical));
    }
    budget.account(bytes.len())?;
    Ok(bytes)
}

fn read_text_file(
    file_system: &dyn ReadOnlyPackageFileSystem,
    path: &Path,
    max_bytes: u64,
    budget: &mut ReadBudget,
    kind: PackageErrorKind,
    logical: &str,
) -> Result<String, PackageLoadError> {
    let metadata = file_system
        .metadata(path)
        .map_err(|error| io_package_error(kind, "inspect text file", Path::new(logical), error))?;
    if metadata.kind != PackageFileKind::File {
        return Err(PackageLoadError::new(
            PackageErrorKind::SpecialFile,
            "expected a regular text file",
        )
        .at(logical));
    }
    if metadata.len > max_bytes {
        return Err(PackageLoadError::new(
            kind,
            format!("text file is {} bytes; limit is {max_bytes}", metadata.len),
        )
        .at(logical));
    }
    let bytes = file_system
        .read_bounded(path, max_bytes)
        .map_err(|error| io_package_error(kind, "read text file", Path::new(logical), error))?;
    if bytes.len() as u64 > max_bytes {
        return Err(
            PackageLoadError::new(kind, format!("text file exceeds {max_bytes} bytes")).at(logical),
        );
    }
    budget.account(bytes.len())?;
    String::from_utf8(bytes)
        .map_err(|_| PackageLoadError::new(kind, "text file is not UTF-8").at(logical))
}

fn component_stylesheet_css_error(
    package: &ResolvedPackage,
    component: Option<&ComponentName>,
    path: &ComponentStylesheetPath,
    error: crate::stylesheet::ComponentCssError,
) -> PackageLoadError {
    let kind = match error.kind {
        ComponentCssErrorKind::Parse | ComponentCssErrorKind::IdSelector => {
            PackageErrorKind::ComponentStylesheetParseFailure
        }
        ComponentCssErrorKind::Import => PackageErrorKind::ComponentStylesheetForbiddenImport,
        ComponentCssErrorKind::UrlResource => {
            PackageErrorKind::ComponentStylesheetForbiddenUrlResource
        }
        ComponentCssErrorKind::FontResource => {
            PackageErrorKind::ComponentStylesheetForbiddenFontResource
        }
        ComponentCssErrorKind::HostSelector => {
            PackageErrorKind::ComponentStylesheetForbiddenHostSelector
        }
        ComponentCssErrorKind::SlottedSelector => {
            PackageErrorKind::ComponentStylesheetForbiddenSlottedSelector
        }
        ComponentCssErrorKind::ShadowSelector => {
            PackageErrorKind::ComponentStylesheetForbiddenShadowSelector
        }
    };
    let owner = component
        .map(|name| format!("component `{name}`"))
        .unwrap_or_else(|| "component stylesheet".to_owned());
    PackageLoadError::new(
        kind,
        format!(
            "{owner} stylesheet `{path}` at {}:{}: {}",
            error.line, error.column, error.message
        ),
    )
    .in_package(package.id().to_string())
    .at(path.as_str())
}

fn ensure_existing_path_has_no_symlink(
    file_system: &dyn ReadOnlyPackageFileSystem,
    path: &Path,
    final_is_manifest: bool,
) -> Result<(), PackageLoadError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                io_package_error(
                    PackageErrorKind::Io,
                    "resolve current directory",
                    Path::new("."),
                    error,
                )
            })?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::RootDir | Component::Prefix(_)) {
            continue;
        }
        let metadata = file_system.metadata(&current).map_err(|error| {
            let kind = if error.kind() == io::ErrorKind::NotFound {
                if final_is_manifest {
                    PackageErrorKind::ManifestMissing
                } else {
                    PackageErrorKind::MissingRoot
                }
            } else {
                PackageErrorKind::Io
            };
            io_package_error(kind, "inspect package path", Path::new("."), error)
        })?;
        if metadata.kind == PackageFileKind::Symlink {
            return Err(PackageLoadError::new(
                if final_is_manifest && current == absolute {
                    PackageErrorKind::ManifestSymlink
                } else {
                    PackageErrorKind::DependencySymlink
                },
                "package path contains a symbolic link",
            )
            .at(if final_is_manifest {
                PACKAGE_MANIFEST_FILE
            } else {
                "."
            }));
        }
    }
    Ok(())
}

fn output_scope(raw: RawOutputScope) -> OutputScope {
    match raw {
        RawOutputScope::All => OutputScope::All,
    }
}

fn json_error(error: serde_json::Error, path: &Path) -> PackageLoadError {
    let message = error.to_string();
    let kind = if message.contains("unknown field") {
        PackageErrorKind::UnknownField
    } else {
        PackageErrorKind::MalformedJson
    };
    PackageLoadError::new(kind, format!("invalid JSON at {error}")).at(path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(PACKAGE_MANIFEST_FILE))
}

fn io_package_error(
    default_kind: PackageErrorKind,
    operation: &str,
    logical_path: &Path,
    error: io::Error,
) -> PackageLoadError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => default_kind,
        io::ErrorKind::PermissionDenied => PackageErrorKind::PermissionDenied,
        _ => default_kind,
    };
    PackageLoadError::new(kind, format!("cannot {operation}: {error}"))
        .at(path_to_logical(logical_path))
}

fn logical_location(composition_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(composition_root).unwrap_or(path);
    let relative = if relative
        .file_name()
        .is_some_and(|name| name == PACKAGE_MANIFEST_FILE)
    {
        relative.parent().unwrap_or(Path::new(""))
    } else {
        relative
    };
    let logical = path_to_logical(relative);
    if logical.is_empty() {
        ".".into()
    } else {
        logical
    }
}

fn path_to_logical(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            Component::CurDir => Some("."),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn bounded_message(mut value: String) -> String {
    const MAX_ERROR_BYTES: usize = 2048;
    truncate_utf8(&mut value, MAX_ERROR_BYTES);
    value
}

fn bounded_path(mut value: String) -> String {
    truncate_utf8(&mut value, MAX_PACKAGE_PATH_BYTES);
    value
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "htmshell-package-test-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write_root(&self, manifest: &str) {
            self.write_package(".", manifest);
            fs::write(self.root.join("index.html"), "<main>headless</main>").unwrap();
            fs::write(self.root.join("panel.html"), "<main>panel</main>").unwrap();
            fs::write(self.root.join("overlay.html"), "<main>overlay</main>").unwrap();
        }

        fn write_library(&self, relative: &str, manifest: &str) {
            self.write_package(relative, manifest);
        }

        fn write_package(&self, relative: &str, manifest: &str) {
            let root = self.root.join(relative);
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join(PACKAGE_MANIFEST_FILE), manifest).unwrap();
        }

        fn manifest(&self) -> PathBuf {
            self.root.join(PACKAGE_MANIFEST_FILE)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn surfaces() -> &'static str {
        r#"[
          {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true},
          {"id":"overlay","kind":"overlay","document":"overlay.html","outputs":"all","initiallyOpen":false}
        ]"#
    }

    fn v1(id: &str) -> String {
        format!(r#"{{"version":1,"id":"{id}","surfaces":{}}}"#, surfaces())
    }

    fn v2_shell(id: &str, version: Option<&str>, dependencies: &str) -> String {
        let version = version
            .map(|value| format!(r#","version":"{value}""#))
            .unwrap_or_default();
        format!(
            r#"{{"version":2,"package":{{"id":"{id}","kind":"shell"{version}}},"dependencies":{dependencies},"surfaces":{}}}"#,
            surfaces()
        )
    }

    fn v2_library(id: &str, version: Option<&str>, dependencies: &str) -> String {
        let version = version
            .map(|value| format!(r#","version":"{value}""#))
            .unwrap_or_default();
        format!(
            r#"{{"version":2,"package":{{"id":"{id}","kind":"library"{version}}},"dependencies":{dependencies}}}"#
        )
    }

    fn dependency(alias: &str, id: &str, path: &str) -> String {
        format!(r#"{{"alias":"{alias}","id":"{id}","path":"{path}"}}"#)
    }

    fn styled_component_shell(styles: &str) -> String {
        format!(
            r#"{{"version":2,"package":{{"id":"org.example.shell","kind":"shell"}},"dependencies":[],"components":[{{"name":"status-card","source":"components/status-card.html","inputs":[],"slots":[],"styles":{styles}}}],"surfaces":{}}}"#,
            surfaces()
        )
    }

    #[test]
    fn package_id_accepts_the_approved_grammar() {
        let maximum = [
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(63),
        ]
        .join(".");
        assert_eq!(maximum.len(), MAX_PACKAGE_ID_BYTES);
        for id in [
            "org.example.shell",
            "dev.coastlinesec.htmshell.controls",
            "com.example.audio-widgets",
            &format!("a.{}", "b".repeat(63)),
            &maximum,
        ] {
            assert_eq!(PackageId::parse(id).unwrap().as_str(), id);
        }
    }

    #[test]
    fn package_id_rejects_invalid_and_reserved_forms() {
        for id in [
            "",
            "example",
            "Org.Example.Shell",
            "org..example",
            "org.example.",
            ".org.example",
            "local.user-package",
            "org.example.-controls",
            "org.example.controls-",
            "org.example white",
            "org.exämple.shell",
            &format!("org.{}.shell", "a".repeat(64)),
            &format!("org.{}", "a".repeat(MAX_PACKAGE_ID_BYTES)),
        ] {
            assert!(PackageId::parse(id).is_err(), "accepted `{id}`");
        }
    }

    #[test]
    fn package_alias_accepts_and_rejects_the_approved_grammar() {
        let maximum = format!("a{}", "b".repeat(MAX_PACKAGE_ALIAS_BYTES - 1));
        for alias in ["a", "controls", "audio-controls", "a1", &maximum] {
            assert_eq!(PackageAlias::parse(alias).unwrap().as_str(), alias);
        }
        for alias in [
            "",
            "Controls",
            "audio.controls",
            "-audio",
            "audio-",
            "audio controls",
            "self",
            "root",
            "input",
            "state",
            "action",
            "service",
            "surface",
            "slot",
            "htm",
        ] {
            assert!(PackageAlias::parse(alias).is_err(), "accepted `{alias}`");
        }
    }

    #[test]
    fn semver_metadata_is_complete_and_descriptive() {
        for version in [
            "0.0.0",
            "1.2.3",
            "1.2.3-alpha.1",
            "1.2.3+build.7",
            "1.2.3-alpha.1+build.7",
        ] {
            assert_eq!(PackageVersion::parse(version).unwrap().to_string(), version);
        }
        for version in ["", "1", "1.2", "01.2.3", "1.02.3", "1.2.03", "1.2.x"] {
            assert!(
                PackageVersion::parse(version).is_err(),
                "accepted `{version}`"
            );
        }
        assert!(PackageVersion::parse(&"1".repeat(MAX_PACKAGE_VERSION_BYTES + 1)).is_err());
    }

    #[test]
    fn schema_v1_normalizes_without_changing_topology() {
        let fixture = Fixture::new();
        fixture.write_root(&v1("portable-shell-demo"));
        let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
        assert_eq!(manifest.manifest().version, 1);
        assert_eq!(manifest.manifest().id, "portable-shell-demo");
        assert_eq!(manifest.manifest().surfaces.len(), 2);
        assert_eq!(
            manifest.snapshot().root_package().id().as_str(),
            "local.portable-shell-demo"
        );
        assert_eq!(
            manifest.snapshot().root_package().schema_source(),
            PackageSchemaSource::SchemaV1
        );
        assert!(
            manifest
                .snapshot()
                .root_package()
                .compatibility_normalized()
        );
        assert_eq!(
            manifest.surface("panel").unwrap().namespace(),
            "htmshell-portable-shell-demo-panel"
        );
    }

    #[test]
    fn schema_v1_legacy_ids_remain_compatible() {
        let fixture = Fixture::new();
        fixture.write_root(&v1("1-shell"));
        let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
        assert_eq!(
            manifest.snapshot().root_package().id().as_str(),
            "local.1-shell"
        );
    }

    #[test]
    fn legacy_headless_root_uses_a_reserved_path_independent_identity() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("index.html"), "<main>legacy</main>").unwrap();
        let mut loader = PackageSnapshotLoader::new();
        let snapshot = loader.load_headless(&fixture.root).unwrap();
        assert_eq!(snapshot.root_package().id().as_str(), LEGACY_HEADLESS_ID);
        assert_eq!(
            snapshot.root_package().schema_source(),
            PackageSchemaSource::LegacyHeadless
        );
        assert_eq!(
            snapshot.headless_entry().unwrap().html(),
            "<main>legacy</main>"
        );
    }

    #[test]
    fn schema_v2_shell_and_library_graph_resolve() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            Some("0.1.0"),
            &format!(
                "[{}]",
                dependency("controls", "org.example.controls", "packages/controls")
            ),
        ));
        fixture.write_library(
            "packages/controls",
            &v2_library("org.example.controls", Some("1.2.3"), "[]"),
        );
        let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
        let packages = manifest.snapshot().packages();
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].id().as_str(), "org.example.controls");
        assert_eq!(packages[1].id().as_str(), "org.example.shell");
        assert_eq!(packages[0].kind(), PackageKind::Library);
        assert_eq!(packages[1].kind(), PackageKind::Shell);
        assert_eq!(packages[0].version().unwrap().to_string(), "1.2.3");
        assert_eq!(manifest.manifest().version, 2);
    }

    #[test]
    fn dependency_declaration_order_and_root_last_are_stable() {
        let fixture = Fixture::new();
        let dependencies = format!(
            "[{},{}]",
            dependency("beta", "org.example.beta", "packages/beta"),
            dependency("alpha", "org.example.alpha", "packages/alpha")
        );
        fixture.write_root(&v2_shell("org.example.shell", None, &dependencies));
        fixture.write_library("packages/beta", &v2_library("org.example.beta", None, "[]"));
        fixture.write_library(
            "packages/alpha",
            &v2_library("org.example.alpha", None, "[]"),
        );
        let first = ValidatedManifest::load(fixture.manifest()).unwrap();
        let second = ValidatedManifest::load(fixture.manifest()).unwrap();
        let ids = |snapshot: &PackageSnapshot| {
            snapshot
                .packages()
                .iter()
                .map(|package| package.id().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(first.snapshot()),
            ["org.example.beta", "org.example.alpha", "org.example.shell"]
        );
        assert_eq!(ids(first.snapshot()), ids(second.snapshot()));
    }

    #[test]
    fn diamond_dependency_is_resolved_once() {
        let fixture = Fixture::new();
        let root_dependencies = format!(
            "[{},{}]",
            dependency("controls", "org.example.controls", "packages/controls"),
            dependency("shared", "org.example.shared", "packages/controls/shared")
        );
        fixture.write_root(&v2_shell("org.example.shell", None, &root_dependencies));
        fixture.write_library(
            "packages/controls",
            &v2_library(
                "org.example.controls",
                None,
                &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
            ),
        );
        fixture.write_library(
            "packages/controls/shared",
            &v2_library("org.example.shared", None, "[]"),
        );
        let snapshot = ValidatedManifest::load(fixture.manifest())
            .unwrap()
            .snapshot;
        assert_eq!(snapshot.packages().len(), 3);
        assert_eq!(
            snapshot
                .packages()
                .iter()
                .filter(|package| package.id().as_str() == "org.example.shared")
                .count(),
            1
        );
    }

    #[test]
    fn library_topology_and_imported_shell_are_rejected() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fixture.write_library(
            "packages/library",
            r#"{"version":2,"package":{"id":"org.example.library","kind":"library"},"dependencies":[],"surfaces":[]}"#,
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::LibraryTopologyViolation
        );

        fixture.write_library(
            "packages/library",
            &v2_shell("org.example.library", None, "[]"),
        );
        fs::write(
            fixture.root.join("packages/library/panel.html"),
            "<main>panel</main>",
        )
        .unwrap();
        fs::write(
            fixture.root.join("packages/library/overlay.html"),
            "<main>overlay</main>",
        )
        .unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ImportedShellPackage
        );
    }

    #[test]
    fn schema_v2_library_cannot_be_a_root() {
        let fixture = Fixture::new();
        fixture.write_package(".", &v2_library("org.example.library", None, "[]"));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::InvalidPackageKind
        );
    }

    #[test]
    fn schema_v2_rejects_invalid_kinds_and_component_entry_extensions() {
        let fixture = Fixture::new();
        fixture.write_package(
            ".",
            &v2_library("org.example.library", None, "[]").replace("\"library\"", "\"theme\""),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::InvalidPackageKind
        );

        fixture.write_root(
            &v2_shell("org.example.shell", None, "[]").replacen(
                "\"dependencies\"",
                "\"components\":[{\"name\":\"status-card\",\"source\":\"components/status-card.html\",\"styling\":[]}],\"dependencies\"",
                1,
            ),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::UnknownField
        );
    }

    #[test]
    fn component_stylesheets_load_parse_share_and_activate_by_reachability() {
        use blitz_dom::{DocumentConfig, StyleThreading};
        use blitz_html::HtmlProvider;
        use style_traits::ToCss;

        let fixture = Fixture::new();
        fixture.write_root(&styled_component_shell(
            r#"["components/status-card.css","components/status-card-override.css"]"#,
        ));
        fs::create_dir_all(fixture.root.join("components")).unwrap();
        fs::write(
            fixture.root.join("components/status-card.html"),
            r#"<template data-htm-component="status-card"><div class="shared" data-case="styled">Styled</div></template>"#,
        )
        .unwrap();
        fs::write(
            fixture.root.join("components/status-card.css"),
            ".shared { color: rgb(1, 2, 3); }",
        )
        .unwrap();
        fs::write(
            fixture.root.join("components/status-card-override.css"),
            ".shared { color: rgb(4, 5, 6); }",
        )
        .unwrap();
        fs::write(
            fixture.root.join("panel.html"),
            r#"<main><htm-use component="status-card"></htm-use></main>"#,
        )
        .unwrap();
        fs::write(fixture.root.join("overlay.html"), "<main>Plain</main>").unwrap();

        let snapshot = PackageSnapshotLoader::new()
            .load_manifest(fixture.manifest())
            .unwrap();
        assert_eq!(snapshot.component_styles().sources().len(), 2);
        assert_eq!(snapshot.component_styles().associations().len(), 2);
        let totals = snapshot.component_styles().totals();
        assert_eq!(totals.source_count, 2);
        assert_eq!(totals.source_read_count, 2);
        assert_eq!(totals.source_parse_count, 2);
        assert_eq!(totals.association_count, 2);
        assert!(totals.bytes_read > 0);

        let manifest = snapshot.root_manifest().unwrap();
        let prepared_panel = manifest
            .surfaces
            .iter()
            .find(|surface| surface.id() == "panel")
            .unwrap()
            .prepared_document()
            .unwrap();
        let prepared_overlay = manifest
            .surfaces
            .iter()
            .find(|surface| surface.id() == "overlay")
            .unwrap()
            .prepared_document()
            .unwrap();
        let mut panel = snapshot
            .instantiate_document(
                prepared_panel,
                1,
                DocumentConfig {
                    html_parser_provider: Some(Arc::new(HtmlProvider)),
                    style_threading: StyleThreading::Sequential,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(panel.style_activation.as_str(), "ownership-aware");
        let evidence = crate::style_owner::activate_style_ownership(
            &mut panel.document,
            &panel.style_ownership,
            &panel.style_activation,
        )
        .unwrap();
        assert_eq!(evidence.parsed_stylesheets, 2);
        assert_eq!(evidence.stylesheet_associations, 2);
        panel.document.resolve(0.0);
        let slot = panel
            .document
            .tree()
            .iter()
            .find_map(|(slot, node)| {
                node.element_data()
                    .and_then(|element| {
                        element
                            .attrs()
                            .iter()
                            .find(|attribute| attribute.name.local.as_ref() == "data-case")
                    })
                    .is_some_and(|attribute| attribute.value.as_str() == "styled")
                    .then_some(slot)
            })
            .unwrap();
        assert_eq!(
            panel
                .document
                .get_node(slot)
                .unwrap()
                .primary_styles()
                .unwrap()
                .clone_color()
                .to_css_string(),
            "rgb(4, 5, 6)"
        );
        let first_owner = panel.style_ownership.node(slot).unwrap().owner().clone();

        let mut second_panel = snapshot
            .instantiate_document(
                prepared_panel,
                2,
                DocumentConfig {
                    html_parser_provider: Some(Arc::new(HtmlProvider)),
                    style_threading: StyleThreading::Sequential,
                    ..Default::default()
                },
            )
            .unwrap();
        let second_evidence = crate::style_owner::activate_style_ownership(
            &mut second_panel.document,
            &second_panel.style_ownership,
            &second_panel.style_activation,
        )
        .unwrap();
        assert_eq!(second_evidence.parsed_stylesheets, 2);
        second_panel.document.resolve(0.0);
        let second_slot = second_panel
            .document
            .tree()
            .iter()
            .find_map(|(slot, node)| {
                node.element_data()
                    .and_then(|element| {
                        element
                            .attrs()
                            .iter()
                            .find(|attribute| attribute.name.local.as_ref() == "data-case")
                    })
                    .is_some_and(|attribute| attribute.value.as_str() == "styled")
                    .then_some(slot)
            })
            .unwrap();
        assert_ne!(
            &first_owner,
            second_panel
                .style_ownership
                .node(second_slot)
                .unwrap()
                .owner()
        );
        assert_eq!(
            second_panel
                .document
                .get_node(second_slot)
                .unwrap()
                .primary_styles()
                .unwrap()
                .clone_color()
                .to_css_string(),
            "rgb(4, 5, 6)"
        );
        assert_eq!(snapshot.component_styles().totals().source_parse_count, 2);

        let overlay = snapshot
            .instantiate_document(prepared_overlay, 3, DocumentConfig::default())
            .unwrap();
        assert_eq!(overlay.style_activation.as_str(), "legacy-document-global");
    }

    #[test]
    fn duplicate_alias_unknown_fields_and_id_mismatch_are_rejected() {
        let fixture = Fixture::new();
        let duplicate = format!(
            "[{},{}]",
            dependency("shared", "org.example.one", "packages/one"),
            dependency("shared", "org.example.two", "packages/two")
        );
        fixture.write_root(&v2_shell("org.example.shell", None, &duplicate));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DuplicateAlias
        );

        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("one", "org.example.expected", "packages/one")
            ),
        ));
        fixture.write_library(
            "packages/one",
            &v2_library("org.example.actual", None, "[]"),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DependencyIdMismatch
        );

        fixture.write_root(&v2_shell("org.example.shell", None, "[]").replacen(
            "\"dependencies\"",
            "\"unexpected\":true,\"dependencies\"",
            1,
        ));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::UnknownField
        );
    }

    #[test]
    fn identity_and_version_conflicts_reject_the_candidate() {
        let fixture = Fixture::new();
        let dependencies = format!(
            "[{},{}]",
            dependency("one", "org.example.shared", "packages/one"),
            dependency("two", "org.example.shared", "packages/two")
        );
        fixture.write_root(&v2_shell("org.example.shell", None, &dependencies));
        fixture.write_library(
            "packages/one",
            &v2_library("org.example.shared", Some("1.0.0"), "[]"),
        );
        fixture.write_library(
            "packages/two",
            &v2_library("org.example.shared", Some("2.0.0"), "[]"),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::PackageVersionConflict
        );

        fixture.write_library(
            "packages/two",
            &v2_library("org.example.shared", Some("1.0.0"), "[]"),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DuplicatePackageId
        );
    }

    #[test]
    fn dependency_paths_are_normalized_and_contained() {
        assert_eq!(
            validate_dependency_path("packages/./controls").unwrap(),
            "packages/controls"
        );
        assert_eq!(
            validate_dependency_path("packages/contrôles").unwrap(),
            "packages/contrôles"
        );
        for path in [
            "",
            ".",
            "..",
            "../controls",
            "packages/../controls",
            "/packages/controls",
            "C:\\packages\\controls",
            "packages\\controls",
            "packages//controls",
            "https://example.test/controls",
            "//server/controls",
        ] {
            assert!(validate_dependency_path(path).is_err(), "accepted `{path}`");
        }
        let bounded = bounded_path("é".repeat(MAX_PACKAGE_PATH_BYTES));
        assert!(bounded.len() <= MAX_PACKAGE_PATH_BYTES);
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn missing_and_non_directory_dependencies_are_typed() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DependencyMissing
        );

        fs::create_dir_all(fixture.root.join("packages")).unwrap();
        fs::write(fixture.root.join("packages/library"), b"not a directory").unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::SpecialFile
        );
    }

    #[test]
    fn one_location_cannot_be_claimed_as_two_package_ids() {
        let fixture = Fixture::new();
        let dependencies = format!(
            "[{},{}]",
            dependency("one", "org.example.library", "packages/library"),
            dependency("two", "org.example.other", "packages/./library")
        );
        fixture.write_root(&v2_shell("org.example.shell", None, &dependencies));
        fixture.write_library(
            "packages/library",
            &v2_library("org.example.library", None, "[]"),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::PackageLocationConflict
        );
    }

    #[derive(Debug)]
    struct PermissionDeniedFileSystem {
        denied: PathBuf,
    }

    impl ReadOnlyPackageFileSystem for PermissionDeniedFileSystem {
        fn metadata(&self, path: &Path) -> io::Result<PackageFileMetadata> {
            if path == self.denied {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected permission denial",
                ));
            }
            LocalPackageFileSystem.metadata(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            LocalPackageFileSystem.canonicalize(path)
        }

        fn read_bounded(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
            LocalPackageFileSystem.read_bounded(path, max_bytes)
        }
    }

    #[test]
    fn permission_failures_are_typed_without_host_permissions() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fs::create_dir_all(fixture.root.join("packages/library")).unwrap();
        let loader =
            PackageSnapshotLoader::with_file_system(Arc::new(PermissionDeniedFileSystem {
                denied: fixture.root.join("packages/library"),
            }));
        assert_eq!(
            loader
                .build_manifest_candidate(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::PermissionDenied
        );
    }

    #[cfg(unix)]
    #[test]
    fn dependency_directory_and_manifest_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        let outside = Fixture::new();
        outside.write_library(".", &v2_library("org.example.library", None, "[]"));
        fs::create_dir_all(fixture.root.join("packages")).unwrap();
        symlink(&outside.root, fixture.root.join("packages/library")).unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DependencySymlink
        );

        fs::remove_file(fixture.root.join("packages/library")).unwrap();
        fs::create_dir_all(fixture.root.join("packages/library")).unwrap();
        symlink(
            outside.root.join(PACKAGE_MANIFEST_FILE),
            fixture
                .root
                .join("packages/library")
                .join(PACKAGE_MANIFEST_FILE),
        )
        .unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ManifestSymlink
        );
    }

    #[cfg(unix)]
    #[test]
    fn composition_root_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        fixture.write_root(&v2_shell("org.example.shell", None, "[]"));
        let alias = fixture.root.with_extension("symlink");
        symlink(&fixture.root, &alias).unwrap();
        let error = ValidatedManifest::load(alias.join(PACKAGE_MANIFEST_FILE)).unwrap_err();
        fs::remove_file(alias).unwrap();
        assert_eq!(error.kind(), PackageErrorKind::DependencySymlink);
    }

    #[derive(Debug)]
    struct SpecialManifestFileSystem {
        manifest: PathBuf,
    }

    impl ReadOnlyPackageFileSystem for SpecialManifestFileSystem {
        fn metadata(&self, path: &Path) -> io::Result<PackageFileMetadata> {
            if path == self.manifest {
                return Ok(PackageFileMetadata {
                    kind: PackageFileKind::Special,
                    len: 0,
                });
            }
            LocalPackageFileSystem.metadata(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            LocalPackageFileSystem.canonicalize(path)
        }

        fn read_bounded(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
            LocalPackageFileSystem.read_bounded(path, max_bytes)
        }
    }

    #[test]
    fn special_dependency_manifest_is_rejected() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fs::create_dir_all(fixture.root.join("packages/library")).unwrap();
        let special_manifest = fixture.root.join("packages/library/shell.json");
        fs::write(&special_manifest, b"not read").unwrap();
        let loader = PackageSnapshotLoader::with_file_system(Arc::new(SpecialManifestFileSystem {
            manifest: special_manifest,
        }));
        assert_eq!(
            loader
                .build_manifest_candidate(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ManifestSpecialFile
        );
    }

    #[test]
    fn manifest_and_direct_dependency_limits_are_enforced() {
        let fixture = Fixture::new();
        fs::write(
            fixture.manifest(),
            vec![b' '; MAX_PACKAGE_MANIFEST_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::ManifestTooLarge
        );

        let dependencies = (0..=MAX_DIRECT_DEPENDENCIES)
            .map(|index| {
                dependency(
                    &format!("d{index}"),
                    &format!("org.example.d{index}"),
                    &format!("packages/d{index}"),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!("[{dependencies}]"),
        ));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DirectDependencyLimit
        );
    }

    fn write_wide_graph(fixture: &Fixture, nested_count: usize) {
        let root_dependencies = (0..MAX_DIRECT_DEPENDENCIES)
            .map(|index| {
                dependency(
                    &format!("p{index}"),
                    &format!("org.example.p{index}"),
                    &format!("packages/p{index}"),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!("[{root_dependencies}]"),
        ));
        for index in 0..MAX_DIRECT_DEPENDENCIES {
            fixture.write_library(
                &format!("packages/p{index}"),
                &v2_library(&format!("org.example.p{index}"), None, "[]"),
            );
        }
        let nested = (0..nested_count)
            .map(|index| {
                dependency(
                    &format!("n{index}"),
                    &format!("org.example.n{index}"),
                    &format!("nested/n{index}"),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fixture.write_library(
            "packages/p0",
            &v2_library("org.example.p0", None, &format!("[{nested}]")),
        );
        for index in 0..nested_count {
            fixture.write_library(
                &format!("packages/p0/nested/n{index}"),
                &v2_library(&format!("org.example.n{index}"), None, "[]"),
            );
        }
    }

    #[test]
    fn package_count_and_direct_dependency_boundaries_are_exact() {
        let fixture = Fixture::new();
        write_wide_graph(
            &fixture,
            MAX_PACKAGES_PER_GRAPH - MAX_DIRECT_DEPENDENCIES - 1,
        );
        let snapshot = ValidatedManifest::load(fixture.manifest())
            .unwrap()
            .snapshot;
        assert_eq!(snapshot.packages().len(), MAX_PACKAGES_PER_GRAPH);
        assert_eq!(
            snapshot.root_package().dependencies().len(),
            MAX_DIRECT_DEPENDENCIES
        );

        write_wide_graph(&fixture, MAX_PACKAGES_PER_GRAPH - MAX_DIRECT_DEPENDENCIES);
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::PackageCountLimit
        );
    }

    fn write_chain(fixture: &Fixture, library_count: usize) {
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!("[{}]", dependency("d0", "org.example.d0", "d0")),
        ));
        let mut relative = PathBuf::new();
        for depth in 0..library_count {
            relative.push(format!("d{depth}"));
            let dependencies = if depth + 1 == library_count {
                "[]".to_owned()
            } else {
                format!(
                    "[{}]",
                    dependency(
                        &format!("d{}", depth + 1),
                        &format!("org.example.d{}", depth + 1),
                        &format!("d{}", depth + 1)
                    )
                )
            };
            fixture.write_library(
                relative.to_str().unwrap(),
                &v2_library(&format!("org.example.d{depth}"), None, &dependencies),
            );
        }
    }

    #[test]
    fn maximum_dependency_depth_is_valid() {
        let fixture = Fixture::new();
        write_chain(&fixture, MAX_DEPENDENCY_DEPTH);
        let snapshot = ValidatedManifest::load(fixture.manifest())
            .unwrap()
            .snapshot;
        assert_eq!(snapshot.packages().len(), MAX_DEPENDENCY_DEPTH + 1);
        assert_eq!(snapshot.root_package().id().as_str(), "org.example.shell");
    }

    #[test]
    fn dependency_depth_limit_is_enforced_at_the_boundary() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!("[{}]", dependency("d0", "org.example.d0", "d0")),
        ));
        let mut relative = PathBuf::new();
        for depth in 0..=MAX_DEPENDENCY_DEPTH {
            relative.push(format!("d{depth}"));
            let manifest = if depth == MAX_DEPENDENCY_DEPTH {
                v2_library(
                    &format!("org.example.d{depth}"),
                    None,
                    &format!(
                        "[{}]",
                        dependency("overflow", "org.example.overflow", "overflow")
                    ),
                )
            } else {
                v2_library(
                    &format!("org.example.d{depth}"),
                    None,
                    &format!(
                        "[{}]",
                        dependency(
                            &format!("d{}", depth + 1),
                            &format!("org.example.d{}", depth + 1),
                            &format!("d{}", depth + 1)
                        )
                    ),
                )
            };
            fixture.write_library(relative.to_str().unwrap(), &manifest);
        }
        let overflow = relative.join("overflow");
        fixture.write_library(
            overflow.to_str().unwrap(),
            &v2_library("org.example.overflow", None, "[]"),
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::DependencyDepthLimit
        );
    }

    #[test]
    fn read_budget_rejects_overflow_without_allocating() {
        let mut budget = ReadBudget {
            bytes: MAX_CANDIDATE_BYTES,
        };
        assert_eq!(
            budget.account(1).unwrap_err().kind(),
            PackageErrorKind::TotalReadLimit
        );
    }

    #[test]
    fn publication_is_atomic_and_generational() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell("org.example.shell", None, "[]"));
        let mut loader = PackageSnapshotLoader::new();
        let first = loader
            .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
            .unwrap();
        assert_eq!(first.generation().get(), 1);
        let current = Arc::clone(loader.current().unwrap());

        fixture.write_root(r#"{"version":2,"package":{"id":"broken","kind":"shell"}}"#);
        assert!(loader.build_manifest_candidate(fixture.manifest()).is_err());
        assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
        assert_eq!(loader.current().unwrap().generation().get(), 1);

        fixture.write_root(&v2_shell("org.example.shell", None, "[]"));
        let second = loader
            .publish(loader.build_manifest_candidate(fixture.manifest()).unwrap())
            .unwrap();
        assert_eq!(second.generation().get(), 2);
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(first.root_package().id(), second.root_package().id());
        let first_node = first
            .node_identity(&PackageId::parse("org.example.shell").unwrap())
            .unwrap();
        let second_node = second
            .node_identity(&PackageId::parse("org.example.shell").unwrap())
            .unwrap();
        assert!(first.contains_node_identity(&first_node));
        assert!(!second.contains_node_identity(&first_node));
        assert!(second.contains_node_identity(&second_node));
    }

    #[test]
    fn snapshot_generation_overflow_preserves_the_current_snapshot() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell("org.example.shell", None, "[]"));
        let mut loader = PackageSnapshotLoader::new();
        let first = loader.load_manifest(fixture.manifest()).unwrap();
        loader.next_generation = u64::MAX;
        let candidate = loader.build_manifest_candidate(fixture.manifest()).unwrap();
        assert_eq!(
            loader.publish(candidate).unwrap_err().kind(),
            PackageErrorKind::SnapshotGenerationOverflow
        );
        assert!(Arc::ptr_eq(loader.current().unwrap(), &first));
    }

    #[test]
    fn repeated_candidate_builds_failures_and_publications_remain_bounded() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell("org.example.shell", None, "[]"));
        let loader = PackageSnapshotLoader::new();
        for _ in 0..1_000 {
            let candidate = loader.build_manifest_candidate(fixture.manifest()).unwrap();
            assert_eq!(candidate.package_count(), 1);
        }

        fixture.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fixture.write_library(
            "packages/library",
            &v2_library("org.example.library", None, "[]"),
        );
        for _ in 0..500 {
            let candidate = loader.build_manifest_candidate(fixture.manifest()).unwrap();
            assert_eq!(candidate.package_count(), 2);
        }

        let mut loader = PackageSnapshotLoader::new();
        let mut old = None;
        for generation in 1..=500 {
            let snapshot = loader.load_manifest(fixture.manifest()).unwrap();
            assert_eq!(snapshot.generation().get(), generation);
            if let Some(weak) = old.replace(Arc::downgrade(&snapshot)) {
                assert!(weak.upgrade().is_none());
            }
        }
        let current = Arc::clone(loader.current().unwrap());
        fixture.write_root(r#"{"version":2,"package":{"id":"broken","kind":"shell"}}"#);
        for _ in 0..500 {
            assert!(loader.build_manifest_candidate(fixture.manifest()).is_err());
            assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
        }
        for _ in 0..500 {
            assert!(validate_dependency_path("../escape").is_err());
        }
    }

    #[test]
    fn deterministic_serialization_contains_only_logical_state() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            Some("1.0.0"),
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fixture.write_library(
            "packages/library",
            &v2_library("org.example.library", None, "[]"),
        );
        let manifest = ValidatedManifest::load(fixture.manifest()).unwrap();
        let first = manifest.deterministic_package_graph_json().unwrap();
        let second = manifest.deterministic_package_graph_json().unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"snapshot_generation\": 1"));
        assert!(first.contains("\"logical_location\": \"packages/library\""));
        assert!(!first.contains(fixture.root.to_str().unwrap()));
        assert!(!first.contains("0x"));
    }

    #[test]
    fn headless_and_live_manifest_loaders_share_the_graph_semantics() {
        let fixture = Fixture::new();
        fixture.write_root(&v2_shell(
            "org.example.shell",
            Some("1.0.0"),
            &format!(
                "[{}]",
                dependency("library", "org.example.library", "packages/library")
            ),
        ));
        fixture.write_library(
            "packages/library",
            &v2_library("org.example.library", Some("2.0.0"), "[]"),
        );
        let live = ValidatedManifest::load(fixture.manifest()).unwrap();
        let mut loader = PackageSnapshotLoader::new();
        let headless = loader.load_headless(&fixture.root).unwrap();
        let summarize = |snapshot: &PackageSnapshot| {
            snapshot
                .packages()
                .iter()
                .map(|package| {
                    (
                        package.id().to_string(),
                        package.kind(),
                        package.version().map(ToString::to_string),
                        package.logical_location().to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(summarize(live.snapshot()), summarize(&headless));
        assert_eq!(
            live.snapshot().root_package().dependencies(),
            headless.root_package().dependencies()
        );

        fixture.write_library(
            "packages/library",
            r#"{"version":2,"package":{"id":"org.example.library","kind":"library"},"dependencies":"invalid"}"#,
        );
        let live_error = ValidatedManifest::load(fixture.manifest())
            .unwrap_err()
            .kind();
        let mut loader = PackageSnapshotLoader::new();
        let headless_error = loader.load_headless(&fixture.root).unwrap_err().kind();
        assert_eq!(live_error, headless_error);
        assert_eq!(live_error, PackageErrorKind::MalformedJson);
    }

    #[test]
    fn schema_v1_unknown_fields_and_topology_rules_remain_strict() {
        let fixture = Fixture::new();
        fixture.write_root(&v1("portable-shell-demo").replacen(
            "\"id\"",
            "\"extra\":true,\"id\"",
            1,
        ));
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::UnknownField
        );
        fixture.write_root(
            r#"{"version":1,"id":"portable-shell-demo","surfaces":[
              {"id":"panel","kind":"panel","document":"panel.html","outputs":"all","edge":"top","thickness":52,"reserveSpace":true}
            ]}"#,
        );
        assert_eq!(
            ValidatedManifest::load(fixture.manifest())
                .unwrap_err()
                .kind(),
            PackageErrorKind::RootTopologyFailure
        );
    }

    #[derive(Debug)]
    struct CycleFileSystem {
        metadata: BTreeMap<PathBuf, PackageFileMetadata>,
        canonical: BTreeMap<PathBuf, PathBuf>,
        files: BTreeMap<PathBuf, Vec<u8>>,
        root_manifest: PathBuf,
        cycle_enabled: AtomicBool,
    }

    impl CycleFileSystem {
        fn new() -> Self {
            let root = PathBuf::from("/composition");
            let a = root.join("a");
            let b = root.join("b");
            let mut metadata = BTreeMap::new();
            for directory in [
                PathBuf::from("/composition"),
                a.clone(),
                b.clone(),
                a.join("b"),
                b.join("a"),
            ] {
                metadata.insert(
                    directory,
                    PackageFileMetadata {
                        kind: PackageFileKind::Directory,
                        len: 0,
                    },
                );
            }
            let root_manifest = root.join(PACKAGE_MANIFEST_FILE);
            let a_manifest = a.join(PACKAGE_MANIFEST_FILE);
            let b_manifest = b.join(PACKAGE_MANIFEST_FILE);
            let panel = root.join("panel.html");
            let overlay = root.join("overlay.html");
            for file in [&root_manifest, &a_manifest, &b_manifest, &panel, &overlay] {
                metadata.insert(
                    file.clone(),
                    PackageFileMetadata {
                        kind: PackageFileKind::File,
                        len: 1,
                    },
                );
            }
            let mut canonical = BTreeMap::new();
            canonical.insert(root.clone(), root.clone());
            canonical.insert(root_manifest.clone(), root_manifest.clone());
            canonical.insert(a.clone(), a.clone());
            canonical.insert(a.join("b"), b.clone());
            canonical.insert(b.join("a"), a.clone());
            canonical.insert(panel.clone(), panel.clone());
            canonical.insert(overlay.clone(), overlay.clone());
            let mut files = BTreeMap::new();
            files.insert(
                root_manifest,
                v2_shell(
                    "org.example.shell",
                    None,
                    &format!("[{}]", dependency("a", "org.example.a", "a")),
                )
                .into_bytes(),
            );
            files.insert(
                a_manifest,
                v2_library(
                    "org.example.a",
                    None,
                    &format!("[{}]", dependency("b", "org.example.b", "b")),
                )
                .into_bytes(),
            );
            files.insert(
                b_manifest,
                v2_library(
                    "org.example.b",
                    None,
                    &format!("[{}]", dependency("a", "org.example.a", "a")),
                )
                .into_bytes(),
            );
            files.insert(panel, b"<main>panel</main>".to_vec());
            files.insert(overlay, b"<main>overlay</main>".to_vec());
            for (path, bytes) in &files {
                metadata.get_mut(path).unwrap().len = bytes.len() as u64;
            }
            Self {
                metadata,
                canonical,
                files,
                root_manifest: root.join(PACKAGE_MANIFEST_FILE),
                cycle_enabled: AtomicBool::new(true),
            }
        }

        fn set_cycle_enabled(&self, enabled: bool) {
            self.cycle_enabled.store(enabled, Ordering::Relaxed);
        }
    }

    impl ReadOnlyPackageFileSystem for CycleFileSystem {
        fn metadata(&self, path: &Path) -> io::Result<PackageFileMetadata> {
            self.metadata
                .get(path)
                .copied()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture path"))
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.canonical
                .get(path)
                .cloned()
                .or_else(|| self.metadata.contains_key(path).then(|| path.to_path_buf()))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture path"))
        }

        fn read_bounded(&self, path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
            if path == self.root_manifest && !self.cycle_enabled.load(Ordering::Relaxed) {
                return Ok(v2_shell("org.example.shell", None, "[]").into_bytes());
            }
            let bytes = self
                .files
                .get(path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture file"))?;
            Ok(bytes
                .iter()
                .copied()
                .take(max_bytes.saturating_add(1) as usize)
                .collect())
        }
    }

    #[test]
    fn cycle_detection_is_independent_from_physical_tree_enumeration() {
        let file_system = Arc::new(CycleFileSystem::new());
        let loader = PackageSnapshotLoader::with_file_system(file_system);
        let error = loader
            .build_manifest_candidate("/composition/shell.json")
            .unwrap_err();
        assert_eq!(error.kind(), PackageErrorKind::DependencyCycle);
        assert!(
            error
                .to_string()
                .contains("org.example.shell -> org.example.a -> org.example.b -> org.example.a")
        );
    }

    #[test]
    fn dependency_cycle_candidate_cannot_replace_last_known_good() {
        let file_system = Arc::new(CycleFileSystem::new());
        file_system.set_cycle_enabled(false);
        let mut loader = PackageSnapshotLoader::with_file_system(file_system.clone());
        let current = loader.load_manifest("/composition/shell.json").unwrap();
        file_system.set_cycle_enabled(true);
        assert_eq!(
            loader
                .build_manifest_candidate("/composition/shell.json")
                .unwrap_err()
                .kind(),
            PackageErrorKind::DependencyCycle
        );
        assert!(Arc::ptr_eq(loader.current().unwrap(), &current));
        assert_eq!(current.generation().get(), 1);
    }

    #[test]
    fn repeated_cycle_detection_does_not_grow_diagnostics() {
        let file_system = Arc::new(CycleFileSystem::new());
        let loader = PackageSnapshotLoader::with_file_system(file_system);
        for _ in 0..500 {
            let error = loader
                .build_manifest_candidate("/composition/shell.json")
                .unwrap_err();
            assert_eq!(error.kind(), PackageErrorKind::DependencyCycle);
            assert!(error.to_string().len() <= 2_048 + MAX_PACKAGE_PATH_BYTES);
        }
    }

    #[test]
    #[ignore = "release measurement probe"]
    fn package_candidate_measurement_probe() {
        fn average_us(iterations: u64, mut operation: impl FnMut()) -> u128 {
            let started = Instant::now();
            for _ in 0..iterations {
                operation();
            }
            started.elapsed().as_micros() / u128::from(iterations)
        }

        let v1_fixture = Fixture::new();
        v1_fixture.write_root(&v1("measurement-shell"));
        let v1_loader = PackageSnapshotLoader::new();
        let v1_us = average_us(100, || {
            v1_loader
                .build_manifest_candidate(v1_fixture.manifest())
                .unwrap();
        });

        let v2_fixture = Fixture::new();
        v2_fixture.write_root(&v2_shell("org.example.measurement", None, "[]"));
        let v2_loader = PackageSnapshotLoader::new();
        let v2_us = average_us(100, || {
            v2_loader
                .build_manifest_candidate(v2_fixture.manifest())
                .unwrap();
        });

        let chain8 = Fixture::new();
        write_chain(&chain8, 8);
        let chain8_loader = PackageSnapshotLoader::new();
        let chain8_us = average_us(50, || {
            chain8_loader
                .build_manifest_candidate(chain8.manifest())
                .unwrap();
        });

        let chain16 = Fixture::new();
        write_chain(&chain16, MAX_DEPENDENCY_DEPTH);
        let chain16_loader = PackageSnapshotLoader::new();
        let chain16_us = average_us(50, || {
            chain16_loader
                .build_manifest_candidate(chain16.manifest())
                .unwrap();
        });

        let wide = Fixture::new();
        write_wide_graph(&wide, MAX_PACKAGES_PER_GRAPH - MAX_DIRECT_DEPENDENCIES - 1);
        let wide_loader = PackageSnapshotLoader::new();
        let wide_us = average_us(20, || {
            wide_loader
                .build_manifest_candidate(wide.manifest())
                .unwrap();
        });

        let diamond = Fixture::new();
        diamond.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{},{}]",
                dependency("controls", "org.example.controls", "controls"),
                dependency("shared", "org.example.shared", "controls/shared")
            ),
        ));
        diamond.write_library(
            "controls",
            &v2_library(
                "org.example.controls",
                None,
                &format!("[{}]", dependency("shared", "org.example.shared", "shared")),
            ),
        );
        diamond.write_library(
            "controls/shared",
            &v2_library("org.example.shared", None, "[]"),
        );
        let diamond_loader = PackageSnapshotLoader::new();
        let diamond_us = average_us(100, || {
            diamond_loader
                .build_manifest_candidate(diamond.manifest())
                .unwrap();
        });

        let cycle_file_system = Arc::new(CycleFileSystem::new());
        let cycle_loader = PackageSnapshotLoader::with_file_system(cycle_file_system);
        let cycle_us = average_us(100, || {
            assert_eq!(
                cycle_loader
                    .build_manifest_candidate("/composition/shell.json")
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::DependencyCycle
            );
        });

        let conflict = Fixture::new();
        conflict.write_root(&v2_shell(
            "org.example.shell",
            None,
            &format!(
                "[{},{}]",
                dependency("one", "org.example.shared", "one"),
                dependency("two", "org.example.shared", "two")
            ),
        ));
        conflict.write_library("one", &v2_library("org.example.shared", None, "[]"));
        conflict.write_library("two", &v2_library("org.example.shared", None, "[]"));
        let conflict_loader = PackageSnapshotLoader::new();
        let conflict_us = average_us(100, || {
            assert_eq!(
                conflict_loader
                    .build_manifest_candidate(conflict.manifest())
                    .unwrap_err()
                    .kind(),
                PackageErrorKind::DuplicatePackageId
            );
        });

        let candidate = wide_loader
            .build_manifest_candidate(wide.manifest())
            .unwrap();
        let mut publication_loader = PackageSnapshotLoader::new();
        let publication_started = Instant::now();
        let published = publication_loader.publish(candidate).unwrap();
        let publication_us = publication_started.elapsed().as_micros();
        let serialization_us = {
            let started = Instant::now();
            published.deterministic_json().unwrap();
            started.elapsed().as_micros()
        };
        let edge_count = published
            .packages()
            .iter()
            .map(|package| package.dependencies().len())
            .sum::<usize>();

        eprintln!(
            "package_measurements_us v1={v1_us} v2={v2_us} chain8={chain8_us} chain16={chain16_us} packages64={wide_us} diamond={diamond_us} cycle={cycle_us} conflict={conflict_us} publication={publication_us} serialization={serialization_us} bytes={} packages={} edges={edge_count}",
            published.bytes_read(),
            published.packages().len()
        );
    }
}
