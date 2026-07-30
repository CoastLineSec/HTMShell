use crate::component::{ComponentDefinitionKey, ComponentInstanceId};
use crate::package::{PackageErrorKind, PackageId, PackageLoadError, PackageSnapshotGeneration};
use image::codecs::jpeg::JpegDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{DynamicImage, ImageDecoder};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

pub const MAX_COMPONENT_RESOURCE_DECLARATIONS: usize = 32;
pub const MAX_COMPONENT_RESOURCE_ASSOCIATIONS_PER_PACKAGE: usize = 4_096;
pub const MAX_COMPONENT_RESOURCE_SOURCES_PER_PACKAGE: usize = 256;
pub const MAX_COMPONENT_RESOURCE_NAME_BYTES: usize = 64;
pub const MAX_COMPONENT_RESOURCE_PATH_BYTES: usize = 512;
pub const MAX_COMPONENT_RESOURCE_PATH_COMPONENTS: usize = 32;
pub const MAX_COMPONENT_RASTER_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_COMPONENT_RASTER_WIDTH: u32 = 4_096;
pub const MAX_COMPONENT_RASTER_HEIGHT: u32 = 4_096;
pub const MAX_COMPONENT_RASTER_PIXELS: u64 = 16_777_216;
pub const MAX_COMPONENT_RASTER_DECODED_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_COMPONENT_RESOURCE_SNAPSHOT_DECODED_BYTES: u64 = 256 * 1024 * 1024;

const RESERVED_NAMES: [&str; 11] = [
    "resource",
    "component",
    "input",
    "slot",
    "style",
    "state",
    "action",
    "service",
    "surface",
    "host",
    "repeat",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentResourceName(Arc<str>);

impl ComponentResourceName {
    pub fn parse(value: &str) -> Result<Self, PackageLoadError> {
        let valid_length = !value.is_empty() && value.len() <= MAX_COMPONENT_RESOURCE_NAME_BYTES;
        let bytes = value.as_bytes();
        let valid_token = valid_length
            && value.is_ascii()
            && bytes.first().is_some_and(u8::is_ascii_lowercase)
            && bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
            && !value.contains("--");
        if !valid_token {
            return Err(PackageLoadError::new(
                PackageErrorKind::InvalidComponentResourceName,
                format!(
                    "component resource name must contain 1..={MAX_COMPONENT_RESOURCE_NAME_BYTES} lowercase ASCII bytes using letters, digits, and single interior hyphens"
                ),
            ));
        }
        if RESERVED_NAMES.contains(&value)
            || ["htm-", "xml-", "xlink-"]
                .iter()
                .any(|prefix| value.starts_with(prefix))
        {
            return Err(PackageLoadError::new(
                PackageErrorKind::ReservedComponentResourceName,
                format!("component resource name `{value}` is reserved"),
            ));
        }
        Ok(Self(Arc::from(value)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentResourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentResourceKind {
    Raster,
}

impl ComponentResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raster => "raster",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ComponentRasterFormat {
    Png,
    Jpeg,
    WebP,
}

impl ComponentRasterFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::WebP => "webp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentResourcePath(Arc<str>);

impl ComponentResourcePath {
    pub(crate) fn new(value: String) -> Self {
        Self(Arc::from(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentResourcePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentResourceDeclaration {
    name: ComponentResourceName,
    kind: ComponentResourceKind,
    source: ComponentResourcePath,
}

impl ComponentResourceDeclaration {
    pub(crate) fn new(
        name: ComponentResourceName,
        kind: ComponentResourceKind,
        source: ComponentResourcePath,
    ) -> Self {
        Self { name, kind, source }
    }

    pub fn name(&self) -> &ComponentResourceName {
        &self.name
    }

    pub fn kind(&self) -> ComponentResourceKind {
        self.kind
    }

    pub fn source(&self) -> &ComponentResourcePath {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentResourceSourceId {
    package_id: PackageId,
    path: ComponentResourcePath,
}

impl ComponentResourceSourceId {
    pub(crate) fn new(package_id: PackageId, path: ComponentResourcePath) -> Self {
        Self { package_id, path }
    }

    pub fn package_id(&self) -> &PackageId {
        &self.package_id
    }

    pub fn path(&self) -> &ComponentResourcePath {
        &self.path
    }

    pub fn deterministic_string(&self, generation: PackageSnapshotGeneration) -> String {
        format!(
            "component-raster-source:{}:{}@{}",
            self.package_id,
            self.path,
            generation.get()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentResourceSemanticVersion(Arc<str>);

impl ComponentResourceSemanticVersion {
    pub fn deterministic_string(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct ComponentRasterSource {
    id: ComponentResourceSourceId,
    format: ComponentRasterFormat,
    encoded_bytes: u64,
    width: u32,
    height: u32,
    rgba8: Arc<Vec<u8>>,
    semantic_version: ComponentResourceSemanticVersion,
}

impl ComponentRasterSource {
    pub(crate) fn decode(
        id: ComponentResourceSourceId,
        encoded: &[u8],
        remaining_snapshot_decoded_bytes: u64,
    ) -> Result<Self, RasterDecodeError> {
        let (format, image) = decode_raster(encoded, remaining_snapshot_decoded_bytes)?;
        let width = image.width();
        let height = image.height();
        validate_dimensions(width, height)?;
        let rgba8 = image.into_rgba8().into_raw();
        let decoded_bytes =
            u64::try_from(rgba8.len()).map_err(|_| RasterDecodeError::decoded_limit())?;
        if decoded_bytes > MAX_COMPONENT_RASTER_DECODED_BYTES {
            return Err(RasterDecodeError::decoded_limit());
        }
        if decoded_bytes > remaining_snapshot_decoded_bytes {
            return Err(RasterDecodeError::snapshot_decoded_limit());
        }
        let semantic_version = ComponentResourceSemanticVersion(Arc::from(semantic_version(
            format, width, height, &rgba8,
        )));
        Ok(Self {
            id,
            format,
            encoded_bytes: encoded.len() as u64,
            width,
            height,
            rgba8: Arc::new(rgba8),
            semantic_version,
        })
    }

    pub fn id(&self) -> &ComponentResourceSourceId {
        &self.id
    }

    pub fn package_id(&self) -> &PackageId {
        self.id.package_id()
    }

    pub fn path(&self) -> &ComponentResourcePath {
        self.id.path()
    }

    pub fn format(&self) -> ComponentRasterFormat {
        self.format
    }

    pub fn encoded_bytes(&self) -> u64 {
        self.encoded_bytes
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn decoded_bytes(&self) -> u64 {
        self.rgba8.len() as u64
    }

    pub fn rgba8(&self) -> &Arc<Vec<u8>> {
        &self.rgba8
    }

    pub fn semantic_version(&self) -> &ComponentResourceSemanticVersion {
        &self.semantic_version
    }
}

#[derive(Debug, Clone)]
pub struct ComponentResourceAssociation {
    definition: ComponentDefinitionKey,
    name: ComponentResourceName,
    source: Arc<ComponentRasterSource>,
    ordinal: u16,
}

impl ComponentResourceAssociation {
    pub(crate) fn new(
        definition: ComponentDefinitionKey,
        name: ComponentResourceName,
        source: Arc<ComponentRasterSource>,
        ordinal: u16,
    ) -> Self {
        Self {
            definition,
            name,
            source,
            ordinal,
        }
    }

    pub fn definition(&self) -> &ComponentDefinitionKey {
        &self.definition
    }

    pub fn name(&self) -> &ComponentResourceName {
        &self.name
    }

    pub fn source(&self) -> &Arc<ComponentRasterSource> {
        &self.source
    }

    pub fn ordinal(&self) -> u16 {
        self.ordinal
    }

    pub fn deterministic_id(&self, generation: PackageSnapshotGeneration) -> String {
        format!(
            "component-raster-association:{}:{}:{}:{}@{}",
            self.definition,
            self.name,
            self.source.path(),
            self.ordinal,
            generation.get()
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentResourceValidationTotals {
    pub source_count: usize,
    pub source_read_count: usize,
    pub source_decode_count: usize,
    pub association_count: usize,
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
}

#[derive(Debug, Default)]
pub struct ComponentResourceCatalog {
    sources: Arc<[Arc<ComponentRasterSource>]>,
    associations: Arc<[ComponentResourceAssociation]>,
    by_definition_and_name:
        BTreeMap<(ComponentDefinitionKey, ComponentResourceName), ComponentResourceAssociation>,
    totals: ComponentResourceValidationTotals,
}

impl ComponentResourceCatalog {
    pub(crate) fn new(
        sources: Vec<Arc<ComponentRasterSource>>,
        associations: Vec<ComponentResourceAssociation>,
        totals: ComponentResourceValidationTotals,
    ) -> Self {
        let by_definition_and_name = associations
            .iter()
            .cloned()
            .map(|association| {
                (
                    (association.definition.clone(), association.name.clone()),
                    association,
                )
            })
            .collect();
        Self {
            sources: sources.into(),
            associations: associations.into(),
            by_definition_and_name,
            totals,
        }
    }

    pub fn sources(&self) -> &[Arc<ComponentRasterSource>] {
        &self.sources
    }

    pub fn associations(&self) -> &[ComponentResourceAssociation] {
        &self.associations
    }

    pub fn association(
        &self,
        definition: &ComponentDefinitionKey,
        name: &ComponentResourceName,
    ) -> Option<&ComponentResourceAssociation> {
        self.by_definition_and_name
            .get(&(definition.clone(), name.clone()))
    }

    pub fn totals(&self) -> &ComponentResourceValidationTotals {
        &self.totals
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentResourceUsageId(Arc<str>);

impl ComponentResourceUsageId {
    pub fn deterministic_string(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ComponentResourceUsage {
    id: ComponentResourceUsageId,
    instance: ComponentInstanceId,
    node_slot: usize,
    association: ComponentResourceAssociation,
    template_source_ordinal: u32,
}

impl ComponentResourceUsage {
    pub(crate) fn new(
        generation: PackageSnapshotGeneration,
        document_serial: u64,
        instance: ComponentInstanceId,
        node_slot: usize,
        association: ComponentResourceAssociation,
        template_source_ordinal: u32,
    ) -> Self {
        let id = ComponentResourceUsageId(Arc::from(format!(
            "component-raster-usage:{}:{}:{}:{}:{}@{}",
            document_serial,
            instance.deterministic_string(),
            node_slot,
            association.name(),
            template_source_ordinal,
            generation.get()
        )));
        Self {
            id,
            instance,
            node_slot,
            association,
            template_source_ordinal,
        }
    }

    pub fn id(&self) -> &ComponentResourceUsageId {
        &self.id
    }

    pub fn instance(&self) -> &ComponentInstanceId {
        &self.instance
    }

    pub fn node_slot(&self) -> usize {
        self.node_slot
    }

    pub fn association(&self) -> &ComponentResourceAssociation {
        &self.association
    }

    pub fn source(&self) -> &Arc<ComponentRasterSource> {
        self.association.source()
    }

    pub fn template_source_ordinal(&self) -> u32 {
        self.template_source_ordinal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RasterDecodeErrorKind {
    Unsupported,
    Animated,
    Dimensions,
    DimensionLimit,
    PixelLimit,
    DecodedLimit,
    SnapshotDecodedLimit,
    Decode,
}

#[derive(Debug)]
pub(crate) struct RasterDecodeError {
    pub kind: RasterDecodeErrorKind,
    pub message: &'static str,
}

impl RasterDecodeError {
    fn decoded_limit() -> Self {
        Self {
            kind: RasterDecodeErrorKind::DecodedLimit,
            message: "component raster decoded bytes exceed the limit",
        }
    }

    fn snapshot_decoded_limit() -> Self {
        Self {
            kind: RasterDecodeErrorKind::SnapshotDecodedLimit,
            message: "component raster decoded snapshot bytes exceed the limit",
        }
    }
}

fn decode_raster(
    encoded: &[u8],
    remaining_snapshot_decoded_bytes: u64,
) -> Result<(ComponentRasterFormat, DynamicImage), RasterDecodeError> {
    if encoded.starts_with(b"GIF87a") || encoded.starts_with(b"GIF89a") {
        return Err(RasterDecodeError {
            kind: RasterDecodeErrorKind::Unsupported,
            message: "GIF component raster resources are not supported",
        });
    }
    if encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        let decoder = PngDecoder::new(Cursor::new(encoded)).map_err(decode_error)?;
        if decoder.is_apng().map_err(decode_error)? {
            return Err(RasterDecodeError {
                kind: RasterDecodeErrorKind::Animated,
                message: "animated PNG component raster resources are not supported",
            });
        }
        validate_dimensions(decoder.dimensions().0, decoder.dimensions().1)?;
        validate_snapshot_decoded_budget(
            decoder.dimensions().0,
            decoder.dimensions().1,
            remaining_snapshot_decoded_bytes,
        )?;
        return DynamicImage::from_decoder(decoder)
            .map(|image| (ComponentRasterFormat::Png, image))
            .map_err(decode_error);
    }
    if encoded.starts_with(b"\xff\xd8\xff") {
        let decoder = JpegDecoder::new(Cursor::new(encoded)).map_err(decode_error)?;
        validate_dimensions(decoder.dimensions().0, decoder.dimensions().1)?;
        validate_snapshot_decoded_budget(
            decoder.dimensions().0,
            decoder.dimensions().1,
            remaining_snapshot_decoded_bytes,
        )?;
        return DynamicImage::from_decoder(decoder)
            .map(|image| (ComponentRasterFormat::Jpeg, image))
            .map_err(decode_error);
    }
    if encoded.len() >= 12 && &encoded[..4] == b"RIFF" && &encoded[8..12] == b"WEBP" {
        let decoder = WebPDecoder::new(Cursor::new(encoded)).map_err(decode_error)?;
        if decoder.has_animation() {
            return Err(RasterDecodeError {
                kind: RasterDecodeErrorKind::Animated,
                message: "animated WebP component raster resources are not supported",
            });
        }
        validate_dimensions(decoder.dimensions().0, decoder.dimensions().1)?;
        validate_snapshot_decoded_budget(
            decoder.dimensions().0,
            decoder.dimensions().1,
            remaining_snapshot_decoded_bytes,
        )?;
        return DynamicImage::from_decoder(decoder)
            .map(|image| (ComponentRasterFormat::WebP, image))
            .map_err(decode_error);
    }
    Err(RasterDecodeError {
        kind: RasterDecodeErrorKind::Unsupported,
        message: "component raster format must be PNG, JPEG, or static WebP",
    })
}

fn validate_snapshot_decoded_budget(
    width: u32,
    height: u32,
    remaining: u64,
) -> Result<(), RasterDecodeError> {
    let decoded = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(RasterDecodeError::snapshot_decoded_limit)?;
    if decoded > remaining {
        return Err(RasterDecodeError::snapshot_decoded_limit());
    }
    Ok(())
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), RasterDecodeError> {
    if width == 0 || height == 0 {
        return Err(RasterDecodeError {
            kind: RasterDecodeErrorKind::Dimensions,
            message: "component raster dimensions must be nonzero",
        });
    }
    if width > MAX_COMPONENT_RASTER_WIDTH || height > MAX_COMPONENT_RASTER_HEIGHT {
        return Err(RasterDecodeError {
            kind: RasterDecodeErrorKind::DimensionLimit,
            message: "component raster dimensions exceed the axis limit",
        });
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(RasterDecodeError {
            kind: RasterDecodeErrorKind::PixelLimit,
            message: "component raster pixel count overflowed",
        })?;
    if pixels > MAX_COMPONENT_RASTER_PIXELS {
        return Err(RasterDecodeError {
            kind: RasterDecodeErrorKind::PixelLimit,
            message: "component raster pixel count exceeds the limit",
        });
    }
    let decoded = pixels
        .checked_mul(4)
        .ok_or_else(RasterDecodeError::decoded_limit)?;
    if decoded > MAX_COMPONENT_RASTER_DECODED_BYTES {
        return Err(RasterDecodeError::decoded_limit());
    }
    Ok(())
}

fn decode_error(_: image::ImageError) -> RasterDecodeError {
    RasterDecodeError {
        kind: RasterDecodeErrorKind::Decode,
        message: "component raster could not be decoded",
    }
}

fn semantic_version(
    format: ComponentRasterFormat,
    width: u32,
    height: u32,
    rgba8: &[u8],
) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x84222325cbf29ce4u64;
    for byte in format
        .as_str()
        .bytes()
        .chain(width.to_le_bytes())
        .chain(height.to_le_bytes())
        .chain(rgba8.iter().copied())
    {
        first ^= u64::from(byte);
        first = first.wrapping_mul(0x100000001b3);
        second ^= u64::from(byte);
        second = second.rotate_left(7).wrapping_mul(0x9e3779b185ebca87);
    }
    format!("component-raster-v1:{first:016x}{second:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_limits_are_checked_without_decoded_allocation() {
        assert!(validate_dimensions(1, 1).is_ok());
        assert!(validate_dimensions(1, MAX_COMPONENT_RASTER_HEIGHT).is_ok());
        assert!(validate_dimensions(MAX_COMPONENT_RASTER_WIDTH, 1).is_ok());
        assert!(
            validate_dimensions(MAX_COMPONENT_RASTER_WIDTH, MAX_COMPONENT_RASTER_HEIGHT).is_ok()
        );
        assert_eq!(
            validate_dimensions(0, 1).unwrap_err().kind,
            RasterDecodeErrorKind::Dimensions
        );
        assert_eq!(
            validate_dimensions(1, 0).unwrap_err().kind,
            RasterDecodeErrorKind::Dimensions
        );
        assert_eq!(
            validate_dimensions(MAX_COMPONENT_RASTER_WIDTH + 1, 1)
                .unwrap_err()
                .kind,
            RasterDecodeErrorKind::DimensionLimit
        );
        assert_eq!(
            validate_dimensions(1, MAX_COMPONENT_RASTER_HEIGHT + 1)
                .unwrap_err()
                .kind,
            RasterDecodeErrorKind::DimensionLimit
        );
        assert_eq!(
            u64::from(MAX_COMPONENT_RASTER_WIDTH) * u64::from(MAX_COMPONENT_RASTER_HEIGHT),
            MAX_COMPONENT_RASTER_PIXELS
        );
        assert_eq!(
            MAX_COMPONENT_RASTER_PIXELS * 4,
            MAX_COMPONENT_RASTER_DECODED_BYTES
        );
        assert!(validate_snapshot_decoded_budget(4_096, 4_096, 64 * 1024 * 1024).is_ok());
        assert_eq!(
            validate_snapshot_decoded_budget(4_096, 4_096, 64 * 1024 * 1024 - 1)
                .unwrap_err()
                .kind,
            RasterDecodeErrorKind::SnapshotDecodedLimit
        );
    }

    #[test]
    fn semantic_version_tracks_canonical_semantics() {
        let pixels = [255, 0, 0, 255];
        let first = semantic_version(ComponentRasterFormat::Png, 1, 1, &pixels);
        assert_eq!(
            first,
            semantic_version(ComponentRasterFormat::Png, 1, 1, &pixels)
        );
        assert_ne!(
            first,
            semantic_version(ComponentRasterFormat::Jpeg, 1, 1, &pixels)
        );
        assert_ne!(
            first,
            semantic_version(ComponentRasterFormat::Png, 2, 1, &pixels)
        );
        assert_ne!(
            first,
            semantic_version(ComponentRasterFormat::Png, 1, 1, &[0, 0, 0, 0])
        );
        assert!(first.starts_with("component-raster-v1:"));
    }
}
