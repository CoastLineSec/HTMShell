use super::stable_hash_bytes;
use crate::model::LogicalRect;
use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity};
use serde::{Deserialize, Serialize};
use std::fmt;
use stylo::color::{AbsoluteColor, ColorSpace};
use stylo::values::computed::Filter as ComputedFilter;

pub const MAX_FOREGROUND_EFFECT_FUNCTIONS: usize = 16;
pub const MAX_FOREGROUND_EFFECT_SERIALIZED_BYTES: usize = 1_024;
pub const MAX_FILTER_DECLARATIONS_PER_DOCUMENT: usize = 256;
pub const MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE: usize = 256;
pub const MAX_FILTER_NESTING_DEPTH: usize = 8;
pub const MAX_FOREGROUND_EFFECT_FACTOR: f32 = 8.0;
pub const MAX_HUE_ROTATION_TURNS: f32 = 100.0;
pub const MAX_FOREGROUND_BLUR_SIGMA: f32 = 64.0;
pub const MAX_FOREGROUND_SHADOW_OFFSET: f32 = 256.0;
pub const MAX_FOREGROUND_EFFECT_EXPANSION: f32 = 512.0;
pub const MAX_EFFECT_LAYER_DIMENSION: u32 = 4_096;
pub const MAX_EFFECT_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EFFECT_SURFACE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_EFFECT_PIPELINE_VARIANTS: usize = 32;

const MAX_COLOR_MATRIX_COEFFICIENT: f64 = 1.0e18;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalF32(u32);

impl CanonicalF32 {
    pub fn new(value: f32) -> Result<Self, ForegroundEffectRejection> {
        if !value.is_finite() {
            return Err(ForegroundEffectRejection::NonfiniteValue);
        }
        Ok(Self(if value == 0.0 {
            0.0f32.to_bits()
        } else {
            value.to_bits()
        }))
    }

    pub fn get(self) -> f32 {
        f32::from_bits(self.0)
    }

    fn write_canonical(self, output: &mut String) {
        let value = self.get();
        if value == 0.0 {
            output.push('0');
            return;
        }
        output.push_str(&value.to_string());
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForegroundEffectVersion(pub u64);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectRole {
    ForegroundFilter,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForegroundEffectId {
    pub document: ExperimentalDocumentIdentity,
    pub dom: ExperimentalNodeIdentity,
    pub role: ForegroundEffectRole,
}

impl ForegroundEffectId {
    pub fn for_node(document: ExperimentalDocumentIdentity, dom: ExperimentalNodeIdentity) -> Self {
        Self {
            document,
            dom,
            role: ForegroundEffectRole::ForegroundFilter,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectColorSpace {
    EncodedSrgb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectAlphaModel {
    StraightRgba,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectCompositionStage {
    DescendantEffects,
    SourceGraphic,
    FilterFunctions,
    ExternalClip,
    ElementOpacity,
    ElementTransform,
    ParentFilteringAndStacking,
}

pub const FOREGROUND_EFFECT_COMPOSITION_ORDER: [ForegroundEffectCompositionStage; 7] = [
    ForegroundEffectCompositionStage::DescendantEffects,
    ForegroundEffectCompositionStage::SourceGraphic,
    ForegroundEffectCompositionStage::FilterFunctions,
    ForegroundEffectCompositionStage::ExternalClip,
    ForegroundEffectCompositionStage::ElementOpacity,
    ForegroundEffectCompositionStage::ElementTransform,
    ForegroundEffectCompositionStage::ParentFilteringAndStacking,
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectBackendCoverage {
    Pending,
    CpuFrameFallbackRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundEffectCoverage {
    pub model_ready: bool,
    pub cpu: ForegroundEffectBackendCoverage,
    pub gpu: ForegroundEffectBackendCoverage,
}

impl ForegroundEffectCoverage {
    pub const MODEL_ONLY: Self = Self {
        model_ready: true,
        cpu: ForegroundEffectBackendCoverage::Pending,
        gpu: ForegroundEffectBackendCoverage::CpuFrameFallbackRequired,
    };
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundEffectLayerMetadata {
    pub offscreen_layer_required: bool,
    pub maximum_dimension: u32,
    pub maximum_image_bytes: usize,
    pub maximum_surface_bytes: usize,
    pub maximum_pipeline_variants: usize,
}

impl ForegroundEffectLayerMetadata {
    pub fn for_list(list: &ForegroundEffectList) -> Self {
        Self {
            offscreen_layer_required: !list.is_visual_identity(),
            maximum_dimension: MAX_EFFECT_LAYER_DIMENSION,
            maximum_image_bytes: MAX_EFFECT_IMAGE_BYTES,
            maximum_surface_bytes: MAX_EFFECT_SURFACE_BYTES,
            maximum_pipeline_variants: MAX_EFFECT_PIPELINE_VARIANTS,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColorEffectKind {
    Brightness,
    Contrast,
    Grayscale,
    HueRotate,
    Invert,
    Opacity,
    Saturate,
    Sepia,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColorEffect {
    pub kind: ColorEffectKind,
    pub value: CanonicalF32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlurEffect {
    pub sigma: CanonicalF32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectColor {
    pub red: CanonicalF32,
    pub green: CanonicalF32,
    pub blue: CanonicalF32,
    pub alpha: CanonicalF32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropShadowEffect {
    pub offset_x: CanonicalF32,
    pub offset_y: CanonicalF32,
    pub sigma: CanonicalF32,
    pub color: EffectColor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ForegroundEffect {
    Color(ColorEffect),
    Blur(BlurEffect),
    DropShadow(DropShadowEffect),
}

impl ForegroundEffect {
    pub fn is_visual_identity(&self) -> bool {
        match self {
            Self::Color(effect) => match effect.kind {
                ColorEffectKind::Brightness
                | ColorEffectKind::Contrast
                | ColorEffectKind::Opacity
                | ColorEffectKind::Saturate => effect.value.get() == 1.0,
                ColorEffectKind::Grayscale
                | ColorEffectKind::HueRotate
                | ColorEffectKind::Invert
                | ColorEffectKind::Sepia => effect.value.get() == 0.0,
            },
            Self::Blur(effect) => effect.sigma.get() == 0.0,
            Self::DropShadow(effect) => effect.color.alpha.get() == 0.0,
        }
    }

    pub fn color_matrix(&self) -> Result<Option<ColorMatrix>, ForegroundEffectRejection> {
        match self {
            Self::Color(effect) => Ok(Some(ColorMatrix::for_effect(*effect)?)),
            Self::Blur(_) | Self::DropShadow(_) => Ok(None),
        }
    }

    pub(crate) fn variant_name(&self) -> &'static str {
        match self {
            Self::Color(effect) => match effect.kind {
                ColorEffectKind::Brightness => "brightness",
                ColorEffectKind::Contrast => "contrast",
                ColorEffectKind::Grayscale => "grayscale",
                ColorEffectKind::HueRotate => "hue_rotate",
                ColorEffectKind::Invert => "invert",
                ColorEffectKind::Opacity => "opacity",
                ColorEffectKind::Saturate => "saturate",
                ColorEffectKind::Sepia => "sepia",
            },
            Self::Blur(_) => "blur",
            Self::DropShadow(_) => "drop_shadow",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundEffectList {
    pub id: ForegroundEffectId,
    pub version: ForegroundEffectVersion,
    pub color_space: ForegroundEffectColorSpace,
    pub alpha_model: ForegroundEffectAlphaModel,
    pub functions: Vec<ForegroundEffect>,
}

impl ForegroundEffectList {
    pub(crate) fn from_functions(
        id: ForegroundEffectId,
        functions: Vec<ForegroundEffect>,
    ) -> Result<Self, ForegroundEffectRejection> {
        if functions.len() > MAX_FOREGROUND_EFFECT_FUNCTIONS {
            return Err(ForegroundEffectRejection::FunctionCount);
        }
        if functions
            .iter()
            .filter(|effect| matches!(effect, ForegroundEffect::DropShadow(_)))
            .count()
            > 1
        {
            return Err(ForegroundEffectRejection::DropShadowCount);
        }
        for effect in &functions {
            validate_normalized_effect(effect)?;
        }
        let mut list = Self {
            id,
            version: ForegroundEffectVersion(0),
            color_space: ForegroundEffectColorSpace::EncodedSrgb,
            alpha_model: ForegroundEffectAlphaModel::StraightRgba,
            functions,
        };
        let canonical = list.serialize_semantics();
        if canonical.len() > MAX_FOREGROUND_EFFECT_SERIALIZED_BYTES {
            return Err(ForegroundEffectRejection::SerializedLength);
        }
        list.propagated_bounds(&LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        })?;
        list.version = ForegroundEffectVersion(stable_hash_bytes(canonical.as_bytes()));
        Ok(list)
    }

    pub fn is_visual_identity(&self) -> bool {
        self.functions
            .iter()
            .all(ForegroundEffect::is_visual_identity)
    }

    pub fn semantically_eq(&self, other: &Self) -> bool {
        self.color_space == other.color_space
            && self.alpha_model == other.alpha_model
            && self.functions == other.functions
    }

    pub fn expands_geometry(&self) -> bool {
        self.functions.iter().any(|effect| match effect {
            ForegroundEffect::Color(_) => false,
            ForegroundEffect::Blur(effect) => effect.sigma.get() > 0.0,
            ForegroundEffect::DropShadow(effect) => effect.color.alpha.get() > 0.0,
        })
    }

    pub fn structurally_compatible_with(&self, other: &Self) -> bool {
        self.functions.len() == other.functions.len()
            && self
                .functions
                .iter()
                .zip(&other.functions)
                .all(|(left, right)| {
                    matches!(
                        (left, right),
                        (
                            ForegroundEffect::Color(ColorEffect { kind: left, .. }),
                            ForegroundEffect::Color(ColorEffect { kind: right, .. })
                        ) if left == right
                    ) || matches!(
                        (left, right),
                        (ForegroundEffect::Blur(_), ForegroundEffect::Blur(_))
                            | (
                                ForegroundEffect::DropShadow(_),
                                ForegroundEffect::DropShadow(_)
                            )
                    )
                })
    }

    pub fn serialize_semantics(&self) -> String {
        let mut output = String::from("foreground_effects_v1[");
        for (index, effect) in self.functions.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            output.push_str(effect.variant_name());
            output.push('(');
            match effect {
                ForegroundEffect::Color(effect) => {
                    effect.value.write_canonical(&mut output);
                }
                ForegroundEffect::Blur(effect) => {
                    effect.sigma.write_canonical(&mut output);
                }
                ForegroundEffect::DropShadow(effect) => {
                    effect.offset_x.write_canonical(&mut output);
                    output.push(',');
                    effect.offset_y.write_canonical(&mut output);
                    output.push(',');
                    effect.sigma.write_canonical(&mut output);
                    for value in [
                        effect.color.red,
                        effect.color.green,
                        effect.color.blue,
                        effect.color.alpha,
                    ] {
                        output.push(',');
                        value.write_canonical(&mut output);
                    }
                }
            }
            output.push(')');
        }
        output.push(']');
        output
    }

    pub fn color_matrix_runs(&self) -> Result<Vec<ColorMatrixRun>, ForegroundEffectRejection> {
        let mut runs = Vec::new();
        let mut start = None;
        let mut matrix = ColorMatrix::identity();
        for (index, effect) in self.functions.iter().enumerate() {
            if let Some(next) = effect.color_matrix()? {
                start.get_or_insert(index);
                matrix = next.then(matrix)?;
            } else if let Some(start) = start.take() {
                runs.push(ColorMatrixRun {
                    start,
                    function_count: index - start,
                    matrix,
                });
                matrix = ColorMatrix::identity();
            }
        }
        if let Some(start) = start {
            runs.push(ColorMatrixRun {
                start,
                function_count: self.functions.len() - start,
                matrix,
            });
        }
        Ok(runs)
    }

    pub fn propagated_bounds(
        &self,
        source: &LogicalRect,
    ) -> Result<LogicalRect, ForegroundEffectRejection> {
        if !valid_rect(source) {
            return Err(ForegroundEffectRejection::InvalidBounds);
        }
        if source.width == 0.0 || source.height == 0.0 {
            return Ok(source.clone());
        }
        let mut bounds = source.clone();
        for effect in &self.functions {
            bounds = match effect {
                ForegroundEffect::Color(_) => bounds,
                ForegroundEffect::Blur(effect) => {
                    expand_rect(&bounds, blur_support(effect.sigma.get()))?
                }
                ForegroundEffect::DropShadow(effect) if effect.color.alpha.get() == 0.0 => bounds,
                ForegroundEffect::DropShadow(effect) => {
                    let expanded = expand_rect(&bounds, blur_support(effect.sigma.get()))?;
                    let shadow =
                        translate_rect(&expanded, effect.offset_x.get(), effect.offset_y.get())?;
                    union_rect(&bounds, &shadow)?
                }
            };
        }
        validate_expansion(source, &bounds)?;
        Ok(bounds)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColorMatrix {
    coefficients: [[CanonicalF32; 5]; 4],
}

impl ColorMatrix {
    pub fn identity() -> Self {
        Self::from_rows([
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
        ])
        .expect("identity color matrix is finite")
    }

    pub fn coefficients(&self) -> [[f32; 5]; 4] {
        self.coefficients.map(|row| row.map(CanonicalF32::get))
    }

    pub fn transform(&self, input: [f32; 4]) -> [f32; 4] {
        self.coefficients().map(|row| {
            row[0].mul_add(
                input[0],
                row[1].mul_add(
                    input[1],
                    row[2].mul_add(input[2], row[3].mul_add(input[3], row[4])),
                ),
            )
        })
    }

    pub fn then(self, previous: Self) -> Result<Self, ForegroundEffectRejection> {
        let left = self.coefficients();
        let right = previous.coefficients();
        let mut output = [[0.0f64; 5]; 4];
        for row in 0..4 {
            for column in 0..4 {
                output[row][column] = (0..4)
                    .map(|middle| f64::from(left[row][middle]) * f64::from(right[middle][column]))
                    .sum();
            }
            output[row][4] = f64::from(left[row][4])
                + (0..4)
                    .map(|middle| f64::from(left[row][middle]) * f64::from(right[middle][4]))
                    .sum::<f64>();
        }
        Self::from_f64_rows(output)
    }

    pub fn serialize_canonical(&self) -> String {
        let mut output = String::from("rgba_affine_4x5_v1[");
        for (row_index, row) in self.coefficients.iter().enumerate() {
            if row_index > 0 {
                output.push(';');
            }
            for (column_index, value) in row.iter().enumerate() {
                if column_index > 0 {
                    output.push(',');
                }
                value.write_canonical(&mut output);
            }
        }
        output.push(']');
        output
    }

    fn for_effect(effect: ColorEffect) -> Result<Self, ForegroundEffectRejection> {
        let amount = f64::from(effect.value.get());
        let identity = Self::identity();
        match effect.kind {
            ColorEffectKind::Brightness => Self::from_f64_rows([
                [amount, 0.0, 0.0, 0.0, 0.0],
                [0.0, amount, 0.0, 0.0, 0.0],
                [0.0, 0.0, amount, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ]),
            ColorEffectKind::Contrast => {
                let offset = 0.5 * (1.0 - amount);
                Self::from_f64_rows([
                    [amount, 0.0, 0.0, 0.0, offset],
                    [0.0, amount, 0.0, 0.0, offset],
                    [0.0, 0.0, amount, 0.0, offset],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ])
            }
            ColorEffectKind::Grayscale => {
                let full = [
                    [0.2126, 0.7152, 0.0722, 0.0, 0.0],
                    [0.2126, 0.7152, 0.0722, 0.0, 0.0],
                    [0.2126, 0.7152, 0.0722, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ];
                identity.interpolate(full, amount)
            }
            ColorEffectKind::HueRotate => {
                let cos = amount.cos();
                let sin = amount.sin();
                Self::from_f64_rows([
                    [
                        0.213 + cos * 0.787 - sin * 0.213,
                        0.715 - cos * 0.715 - sin * 0.715,
                        0.072 - cos * 0.072 + sin * 0.928,
                        0.0,
                        0.0,
                    ],
                    [
                        0.213 - cos * 0.213 + sin * 0.143,
                        0.715 + cos * 0.285 + sin * 0.140,
                        0.072 - cos * 0.072 - sin * 0.283,
                        0.0,
                        0.0,
                    ],
                    [
                        0.213 - cos * 0.213 - sin * 0.787,
                        0.715 - cos * 0.715 + sin * 0.715,
                        0.072 + cos * 0.928 + sin * 0.072,
                        0.0,
                        0.0,
                    ],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ])
            }
            ColorEffectKind::Invert => {
                let slope = 1.0 - 2.0 * amount;
                Self::from_f64_rows([
                    [slope, 0.0, 0.0, 0.0, amount],
                    [0.0, slope, 0.0, 0.0, amount],
                    [0.0, 0.0, slope, 0.0, amount],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ])
            }
            ColorEffectKind::Opacity => Self::from_f64_rows([
                [1.0, 0.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, amount, 0.0],
            ]),
            ColorEffectKind::Saturate => Self::from_f64_rows([
                [
                    0.213 + 0.787 * amount,
                    0.715 - 0.715 * amount,
                    0.072 - 0.072 * amount,
                    0.0,
                    0.0,
                ],
                [
                    0.213 - 0.213 * amount,
                    0.715 + 0.285 * amount,
                    0.072 - 0.072 * amount,
                    0.0,
                    0.0,
                ],
                [
                    0.213 - 0.213 * amount,
                    0.715 - 0.715 * amount,
                    0.072 + 0.928 * amount,
                    0.0,
                    0.0,
                ],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ]),
            ColorEffectKind::Sepia => {
                let full = [
                    [0.393, 0.769, 0.189, 0.0, 0.0],
                    [0.349, 0.686, 0.168, 0.0, 0.0],
                    [0.272, 0.534, 0.131, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0, 0.0],
                ];
                identity.interpolate(full, amount)
            }
        }
    }

    fn interpolate(
        self,
        target: [[f64; 5]; 4],
        amount: f64,
    ) -> Result<Self, ForegroundEffectRejection> {
        let source = self.coefficients();
        let mut output = [[0.0; 5]; 4];
        for row in 0..4 {
            for column in 0..5 {
                output[row][column] =
                    f64::from(source[row][column]) * (1.0 - amount) + target[row][column] * amount;
            }
        }
        Self::from_f64_rows(output)
    }

    fn from_rows(rows: [[f32; 5]; 4]) -> Result<Self, ForegroundEffectRejection> {
        Ok(Self {
            coefficients: rows.map(|row| {
                row.map(|value| {
                    CanonicalF32::new(value).expect("validated finite color-matrix coefficient")
                })
            }),
        })
    }

    fn from_f64_rows(rows: [[f64; 5]; 4]) -> Result<Self, ForegroundEffectRejection> {
        let mut output = [[CanonicalF32(0); 5]; 4];
        for row in 0..4 {
            for column in 0..5 {
                let value = rows[row][column];
                if !value.is_finite() || value.abs() > MAX_COLOR_MATRIX_COEFFICIENT {
                    return Err(ForegroundEffectRejection::MatrixCoefficient);
                }
                output[row][column] = CanonicalF32::new(value as f32)?;
            }
        }
        Ok(Self {
            coefficients: output,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColorMatrixRun {
    pub start: usize,
    pub function_count: usize,
    pub matrix: ColorMatrix,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundEffectRejection {
    FunctionCount,
    DropShadowCount,
    DeclarationCount,
    ActiveElementCount,
    NestingDepth,
    FactorRange,
    HueRange,
    BlurRange,
    ShadowOffsetRange,
    SerializedLength,
    ExpansionLimit,
    NonfiniteValue,
    MatrixCoefficient,
    InvalidBounds,
    UnsupportedComputedValue,
}

impl fmt::Display for ForegroundEffectRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FunctionCount => "foreground filter exceeds the function-count limit",
            Self::DropShadowCount => "foreground filter contains more than one drop shadow",
            Self::DeclarationCount => {
                "document exceeds the normalized foreground-filter declaration limit"
            }
            Self::ActiveElementCount => {
                "surface exceeds the active foreground-filter element limit"
            }
            Self::NestingDepth => "foreground filter exceeds the nesting-depth limit",
            Self::FactorRange => "foreground filter factor exceeds the supported range",
            Self::HueRange => "foreground hue rotation exceeds the supported range",
            Self::BlurRange => "foreground blur exceeds the supported range",
            Self::ShadowOffsetRange => "foreground shadow offset exceeds the supported range",
            Self::SerializedLength => "foreground filter exceeds the serialized-size limit",
            Self::ExpansionLimit => "foreground filter exceeds the damage-expansion limit",
            Self::NonfiniteValue => "foreground filter contains a nonfinite value",
            Self::MatrixCoefficient => "foreground filter matrix exceeds the coefficient limit",
            Self::InvalidBounds => "foreground filter received invalid logical bounds",
            Self::UnsupportedComputedValue => {
                "foreground filter contains an unsupported computed value"
            }
        })
    }
}

pub(crate) fn normalize_computed_filter(
    filters: &[ComputedFilter],
    current_color: &AbsoluteColor,
    id: ForegroundEffectId,
) -> Result<ForegroundEffectList, ForegroundEffectRejection> {
    if filters.len() > MAX_FOREGROUND_EFFECT_FUNCTIONS {
        return Err(ForegroundEffectRejection::FunctionCount);
    }
    let mut functions = Vec::with_capacity(filters.len());
    let mut drop_shadow_seen = false;
    for filter in filters {
        let effect = match filter {
            ComputedFilter::Blur(value) => {
                let sigma = bounded(value.0.px(), 0.0, MAX_FOREGROUND_BLUR_SIGMA)
                    .map_err(|_| ForegroundEffectRejection::BlurRange)?;
                ForegroundEffect::Blur(BlurEffect {
                    sigma: CanonicalF32::new(sigma)?,
                })
            }
            ComputedFilter::Brightness(value) => color_factor(
                ColorEffectKind::Brightness,
                value.0,
                MAX_FOREGROUND_EFFECT_FACTOR,
            )?,
            ComputedFilter::Contrast(value) => color_factor(
                ColorEffectKind::Contrast,
                value.0,
                MAX_FOREGROUND_EFFECT_FACTOR,
            )?,
            ComputedFilter::Grayscale(value) => color_amount(ColorEffectKind::Grayscale, value.0)?,
            ComputedFilter::HueRotate(value) => {
                let radians = value.radians64();
                let limit = f64::from(MAX_HUE_ROTATION_TURNS) * std::f64::consts::TAU;
                if !radians.is_finite() || radians.abs() > limit {
                    return Err(ForegroundEffectRejection::HueRange);
                }
                ForegroundEffect::Color(ColorEffect {
                    kind: ColorEffectKind::HueRotate,
                    value: CanonicalF32::new(radians as f32)?,
                })
            }
            ComputedFilter::Invert(value) => color_amount(ColorEffectKind::Invert, value.0)?,
            ComputedFilter::Opacity(value) => color_amount(ColorEffectKind::Opacity, value.0)?,
            ComputedFilter::Saturate(value) => color_factor(
                ColorEffectKind::Saturate,
                value.0,
                MAX_FOREGROUND_EFFECT_FACTOR,
            )?,
            ComputedFilter::Sepia(value) => color_amount(ColorEffectKind::Sepia, value.0)?,
            ComputedFilter::DropShadow(shadow) => {
                if drop_shadow_seen {
                    return Err(ForegroundEffectRejection::DropShadowCount);
                }
                drop_shadow_seen = true;
                let offset_x = bounded(
                    shadow.horizontal.px(),
                    -MAX_FOREGROUND_SHADOW_OFFSET,
                    MAX_FOREGROUND_SHADOW_OFFSET,
                )
                .map_err(|_| ForegroundEffectRejection::ShadowOffsetRange)?;
                let offset_y = bounded(
                    shadow.vertical.px(),
                    -MAX_FOREGROUND_SHADOW_OFFSET,
                    MAX_FOREGROUND_SHADOW_OFFSET,
                )
                .map_err(|_| ForegroundEffectRejection::ShadowOffsetRange)?;
                let sigma = bounded(shadow.blur.0.px(), 0.0, MAX_FOREGROUND_BLUR_SIGMA)
                    .map_err(|_| ForegroundEffectRejection::BlurRange)?;
                let color = shadow
                    .color
                    .resolve_to_absolute(current_color)
                    .to_color_space(ColorSpace::Srgb);
                let [red, green, blue, alpha] = *color.raw_components();
                ForegroundEffect::DropShadow(DropShadowEffect {
                    offset_x: CanonicalF32::new(offset_x)?,
                    offset_y: CanonicalF32::new(offset_y)?,
                    sigma: CanonicalF32::new(sigma)?,
                    color: EffectColor {
                        red: CanonicalF32::new(red.clamp(0.0, 1.0))?,
                        green: CanonicalF32::new(green.clamp(0.0, 1.0))?,
                        blue: CanonicalF32::new(blue.clamp(0.0, 1.0))?,
                        alpha: CanonicalF32::new(alpha.clamp(0.0, 1.0))?,
                    },
                })
            }
            ComputedFilter::Url(_) => {
                return Err(ForegroundEffectRejection::UnsupportedComputedValue);
            }
        };
        functions.push(effect);
    }
    ForegroundEffectList::from_functions(id, functions)
}

pub(crate) fn validate_future_effect_layer_limits(
    width: u32,
    height: u32,
    image_bytes: usize,
    surface_bytes: usize,
    pipeline_variants: usize,
) -> Result<(), ForegroundEffectRejection> {
    if width > MAX_EFFECT_LAYER_DIMENSION
        || height > MAX_EFFECT_LAYER_DIMENSION
        || image_bytes > MAX_EFFECT_IMAGE_BYTES
        || surface_bytes > MAX_EFFECT_SURFACE_BYTES
        || pipeline_variants > MAX_EFFECT_PIPELINE_VARIANTS
    {
        return Err(ForegroundEffectRejection::ExpansionLimit);
    }
    Ok(())
}

fn color_factor(
    kind: ColorEffectKind,
    value: f32,
    maximum: f32,
) -> Result<ForegroundEffect, ForegroundEffectRejection> {
    let value = bounded(value, 0.0, maximum).map_err(|_| ForegroundEffectRejection::FactorRange)?;
    Ok(ForegroundEffect::Color(ColorEffect {
        kind,
        value: CanonicalF32::new(value)?,
    }))
}

fn validate_normalized_effect(effect: &ForegroundEffect) -> Result<(), ForegroundEffectRejection> {
    match effect {
        ForegroundEffect::Color(effect) => {
            let value = effect.value.get();
            if effect.kind == ColorEffectKind::HueRotate {
                return if value.abs() <= MAX_HUE_ROTATION_TURNS * 2.0 * std::f32::consts::PI {
                    Ok(())
                } else {
                    Err(ForegroundEffectRejection::HueRange)
                };
            }
            let valid = match effect.kind {
                ColorEffectKind::Brightness
                | ColorEffectKind::Contrast
                | ColorEffectKind::Saturate => {
                    (0.0..=MAX_FOREGROUND_EFFECT_FACTOR).contains(&value)
                }
                ColorEffectKind::Grayscale
                | ColorEffectKind::Invert
                | ColorEffectKind::Opacity
                | ColorEffectKind::Sepia => (0.0..=1.0).contains(&value),
                ColorEffectKind::HueRotate => unreachable!("hue rotation handled above"),
            };
            if valid {
                Ok(())
            } else {
                Err(ForegroundEffectRejection::FactorRange)
            }
        }
        ForegroundEffect::Blur(effect) => {
            if (0.0..=MAX_FOREGROUND_BLUR_SIGMA).contains(&effect.sigma.get()) {
                Ok(())
            } else {
                Err(ForegroundEffectRejection::BlurRange)
            }
        }
        ForegroundEffect::DropShadow(effect) => {
            if !(-MAX_FOREGROUND_SHADOW_OFFSET..=MAX_FOREGROUND_SHADOW_OFFSET)
                .contains(&effect.offset_x.get())
                || !(-MAX_FOREGROUND_SHADOW_OFFSET..=MAX_FOREGROUND_SHADOW_OFFSET)
                    .contains(&effect.offset_y.get())
            {
                return Err(ForegroundEffectRejection::ShadowOffsetRange);
            }
            if !(0.0..=MAX_FOREGROUND_BLUR_SIGMA).contains(&effect.sigma.get()) {
                return Err(ForegroundEffectRejection::BlurRange);
            }
            if [
                effect.color.red,
                effect.color.green,
                effect.color.blue,
                effect.color.alpha,
            ]
            .into_iter()
            .any(|value| !(0.0..=1.0).contains(&value.get()))
            {
                return Err(ForegroundEffectRejection::FactorRange);
            }
            Ok(())
        }
    }
}

fn color_amount(
    kind: ColorEffectKind,
    value: f32,
) -> Result<ForegroundEffect, ForegroundEffectRejection> {
    if !value.is_finite() || value < 0.0 {
        return Err(ForegroundEffectRejection::FactorRange);
    }
    Ok(ForegroundEffect::Color(ColorEffect {
        kind,
        value: CanonicalF32::new(value.min(1.0))?,
    }))
}

fn bounded(value: f32, minimum: f32, maximum: f32) -> Result<f32, ()> {
    if value.is_finite() && value >= minimum && value <= maximum {
        Ok(if value == 0.0 { 0.0 } else { value })
    } else {
        Err(())
    }
}

fn blur_support(sigma: f32) -> f32 {
    (sigma * 3.0).ceil()
}

fn valid_rect(rect: &LogicalRect) -> bool {
    [
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        rect.x + rect.width,
        rect.y + rect.height,
    ]
    .into_iter()
    .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
}

fn expand_rect(rect: &LogicalRect, amount: f32) -> Result<LogicalRect, ForegroundEffectRejection> {
    rect_from_edges(
        f64::from(rect.x) - f64::from(amount),
        f64::from(rect.y) - f64::from(amount),
        f64::from(rect.x + rect.width) + f64::from(amount),
        f64::from(rect.y + rect.height) + f64::from(amount),
    )
}

fn translate_rect(
    rect: &LogicalRect,
    x: f32,
    y: f32,
) -> Result<LogicalRect, ForegroundEffectRejection> {
    rect_from_edges(
        f64::from(rect.x) + f64::from(x),
        f64::from(rect.y) + f64::from(y),
        f64::from(rect.x + rect.width) + f64::from(x),
        f64::from(rect.y + rect.height) + f64::from(y),
    )
}

fn union_rect(
    left: &LogicalRect,
    right: &LogicalRect,
) -> Result<LogicalRect, ForegroundEffectRejection> {
    rect_from_edges(
        f64::from(left.x.min(right.x)),
        f64::from(left.y.min(right.y)),
        f64::from((left.x + left.width).max(right.x + right.width)),
        f64::from((left.y + left.height).max(right.y + right.height)),
    )
}

fn rect_from_edges(
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) -> Result<LogicalRect, ForegroundEffectRejection> {
    let values = [left, top, right, bottom, right - left, bottom - top];
    if values
        .into_iter()
        .any(|value| !value.is_finite() || value < f32::MIN as f64 || value > f32::MAX as f64)
    {
        return Err(ForegroundEffectRejection::InvalidBounds);
    }
    let rect = LogicalRect {
        x: left as f32,
        y: top as f32,
        width: (right - left).max(0.0) as f32,
        height: (bottom - top).max(0.0) as f32,
    };
    if valid_rect(&rect) {
        Ok(rect)
    } else {
        Err(ForegroundEffectRejection::InvalidBounds)
    }
}

fn validate_expansion(
    source: &LogicalRect,
    filtered: &LogicalRect,
) -> Result<(), ForegroundEffectRejection> {
    let outsets = [
        f64::from(source.x) - f64::from(filtered.x),
        f64::from(source.y) - f64::from(filtered.y),
        f64::from(filtered.x + filtered.width) - f64::from(source.x + source.width),
        f64::from(filtered.y + filtered.height) - f64::from(source.y + source.height),
    ];
    if outsets
        .into_iter()
        .any(|outset| !outset.is_finite() || outset > f64::from(MAX_FOREGROUND_EFFECT_EXPANSION))
    {
        Err(ForegroundEffectRejection::ExpansionLimit)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ForegroundEffectId {
        ForegroundEffectId::for_node(
            ExperimentalDocumentIdentity { serial: 7 },
            ExperimentalNodeIdentity {
                slot: 3,
                generation: 2,
            },
        )
    }

    fn scalar(kind: ColorEffectKind, value: f32) -> ForegroundEffect {
        ForegroundEffect::Color(ColorEffect {
            kind,
            value: CanonicalF32::new(value).unwrap(),
        })
    }

    fn list(functions: Vec<ForegroundEffect>) -> ForegroundEffectList {
        ForegroundEffectList::from_functions(id(), functions).unwrap()
    }

    #[test]
    fn canonical_float_normalizes_negative_zero() {
        assert_eq!(
            CanonicalF32::new(-0.0).unwrap(),
            CanonicalF32::new(0.0).unwrap()
        );
        assert_eq!(CanonicalF32::new(-0.0).unwrap().get().to_bits(), 0);
        assert!(CanonicalF32::new(f32::NAN).is_err());
        assert!(CanonicalF32::new(f32::INFINITY).is_err());

        let mut zero = String::new();
        CanonicalF32::new(0.0).unwrap().write_canonical(&mut zero);
        let mut smallest = String::new();
        CanonicalF32::new(f32::from_bits(1))
            .unwrap()
            .write_canonical(&mut smallest);
        assert_eq!(zero, "0");
        assert_eq!(smallest, "0.000000000000000000000000000000000000000000001");
    }

    #[test]
    fn semantic_serialization_preserves_order_and_repetition() {
        let first = list(vec![
            scalar(ColorEffectKind::Brightness, 1.0),
            scalar(ColorEffectKind::Contrast, 0.5),
            scalar(ColorEffectKind::Brightness, 1.0),
        ]);
        let second = list(vec![
            scalar(ColorEffectKind::Contrast, 0.5),
            scalar(ColorEffectKind::Brightness, 1.0),
            scalar(ColorEffectKind::Brightness, 1.0),
        ]);
        assert_eq!(
            first.serialize_semantics(),
            "foreground_effects_v1[brightness(1),contrast(0.5),brightness(1)]"
        );
        assert_ne!(first.version, second.version);
        assert_ne!(first, second);
    }

    #[test]
    fn identity_structure_remains_distinct_from_none() {
        let empty = list(Vec::new());
        let identity = list(vec![scalar(ColorEffectKind::Brightness, 1.0)]);
        assert!(empty.is_visual_identity());
        assert!(identity.is_visual_identity());
        assert_eq!(empty.alpha_model, ForegroundEffectAlphaModel::StraightRgba);
        assert_ne!(empty.version, identity.version);
        assert_ne!(empty, identity);
    }

    #[test]
    fn semantic_equality_is_independent_from_effect_instance_identity() {
        let first = list(vec![scalar(ColorEffectKind::Saturate, 1.25)]);
        let second = ForegroundEffectList::from_functions(
            ForegroundEffectId::for_node(
                ExperimentalDocumentIdentity { serial: 9 },
                ExperimentalNodeIdentity {
                    slot: 10,
                    generation: 11,
                },
            ),
            first.functions.clone(),
        )
        .unwrap();
        assert_ne!(first.id, second.id);
        assert_ne!(first, second);
        assert!(first.semantically_eq(&second));
        assert_eq!(first.version, second.version);
    }

    #[test]
    fn interpolation_compatibility_is_structural() {
        let a = list(vec![
            scalar(ColorEffectKind::Brightness, 1.0),
            ForegroundEffect::Blur(BlurEffect {
                sigma: CanonicalF32::new(2.0).unwrap(),
            }),
        ]);
        let b = list(vec![
            scalar(ColorEffectKind::Brightness, 2.0),
            ForegroundEffect::Blur(BlurEffect {
                sigma: CanonicalF32::new(4.0).unwrap(),
            }),
        ]);
        let reordered = list(vec![
            ForegroundEffect::Blur(BlurEffect {
                sigma: CanonicalF32::new(4.0).unwrap(),
            }),
            scalar(ColorEffectKind::Brightness, 2.0),
        ]);
        assert!(a.structurally_compatible_with(&b));
        assert!(!a.structurally_compatible_with(&reordered));
    }

    #[test]
    fn composition_order_and_future_layer_metadata_are_explicit() {
        assert_eq!(
            FOREGROUND_EFFECT_COMPOSITION_ORDER,
            [
                ForegroundEffectCompositionStage::DescendantEffects,
                ForegroundEffectCompositionStage::SourceGraphic,
                ForegroundEffectCompositionStage::FilterFunctions,
                ForegroundEffectCompositionStage::ExternalClip,
                ForegroundEffectCompositionStage::ElementOpacity,
                ForegroundEffectCompositionStage::ElementTransform,
                ForegroundEffectCompositionStage::ParentFilteringAndStacking,
            ]
        );
        let identity = list(vec![scalar(ColorEffectKind::Brightness, 1.0)]);
        let active = list(vec![scalar(ColorEffectKind::Brightness, 1.1)]);
        assert!(!ForegroundEffectLayerMetadata::for_list(&identity).offscreen_layer_required);
        assert!(ForegroundEffectLayerMetadata::for_list(&active).offscreen_layer_required);
    }

    #[test]
    fn matrices_preserve_alpha_except_opacity() {
        for kind in [
            ColorEffectKind::Brightness,
            ColorEffectKind::Contrast,
            ColorEffectKind::Grayscale,
            ColorEffectKind::HueRotate,
            ColorEffectKind::Invert,
            ColorEffectKind::Saturate,
            ColorEffectKind::Sepia,
        ] {
            let effect = scalar(
                kind,
                if kind == ColorEffectKind::HueRotate {
                    0.5
                } else {
                    0.4
                },
            );
            let output = effect
                .color_matrix()
                .unwrap()
                .unwrap()
                .transform([0.2, 0.4, 0.6, 0.3]);
            assert!((output[3] - 0.3).abs() < 0.000_001, "{kind:?}");
        }
        let output = scalar(ColorEffectKind::Opacity, 0.4)
            .color_matrix()
            .unwrap()
            .unwrap()
            .transform([0.2, 0.4, 0.6, 0.5]);
        assert!((output[3] - 0.2).abs() < 0.000_001);
    }

    #[test]
    fn consecutive_matrices_compose_left_to_right() {
        let effects = list(vec![
            scalar(ColorEffectKind::Brightness, 2.0),
            scalar(ColorEffectKind::Contrast, 0.5),
            ForegroundEffect::Blur(BlurEffect {
                sigma: CanonicalF32::new(2.0).unwrap(),
            }),
            scalar(ColorEffectKind::Opacity, 0.5),
        ]);
        let runs = effects.color_matrix_runs().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].start, runs[0].function_count), (0, 2));
        assert_eq!((runs[1].start, runs[1].function_count), (3, 1));
        let output = runs[0].matrix.transform([0.25, 0.5, 0.75, 1.0]);
        assert!((output[0] - 0.5).abs() < 0.000_001);
        assert!((output[1] - 0.75).abs() < 0.000_001);
        assert!((output[2] - 1.0).abs() < 0.000_001);
    }

    #[test]
    fn matrix_serialization_is_stable() {
        assert_eq!(
            scalar(ColorEffectKind::Opacity, 0.5)
                .color_matrix()
                .unwrap()
                .unwrap()
                .serialize_canonical(),
            "rgba_affine_4x5_v1[1,0,0,0,0;0,1,0,0,0;0,0,1,0,0;0,0,0,0.5,0]"
        );
    }

    #[test]
    fn ordered_bounds_propagate_each_stage() {
        let source = LogicalRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let shadow = ForegroundEffect::DropShadow(DropShadowEffect {
            offset_x: CanonicalF32::new(5.0).unwrap(),
            offset_y: CanonicalF32::new(-4.0).unwrap(),
            sigma: CanonicalF32::new(2.0).unwrap(),
            color: EffectColor {
                red: CanonicalF32::new(0.0).unwrap(),
                green: CanonicalF32::new(0.0).unwrap(),
                blue: CanonicalF32::new(0.0).unwrap(),
                alpha: CanonicalF32::new(1.0).unwrap(),
            },
        });
        let blur = ForegroundEffect::Blur(BlurEffect {
            sigma: CanonicalF32::new(1.0).unwrap(),
        });
        let shadow_then_blur = list(vec![shadow.clone(), blur.clone()])
            .propagated_bounds(&source)
            .unwrap();
        let blur_then_shadow = list(vec![blur, shadow]).propagated_bounds(&source).unwrap();
        assert_eq!(
            shadow_then_blur,
            LogicalRect {
                x: 6.0,
                y: 7.0,
                width: 48.0,
                height: 58.0,
            }
        );
        assert_eq!(
            blur_then_shadow,
            LogicalRect {
                x: 6.0,
                y: 7.0,
                width: 48.0,
                height: 58.0,
            }
        );

        let transparent_shadow = ForegroundEffect::DropShadow(DropShadowEffect {
            offset_x: CanonicalF32::new(MAX_FOREGROUND_SHADOW_OFFSET).unwrap(),
            offset_y: CanonicalF32::new(-MAX_FOREGROUND_SHADOW_OFFSET).unwrap(),
            sigma: CanonicalF32::new(MAX_FOREGROUND_BLUR_SIGMA).unwrap(),
            color: EffectColor {
                red: CanonicalF32::new(1.0).unwrap(),
                green: CanonicalF32::new(1.0).unwrap(),
                blue: CanonicalF32::new(1.0).unwrap(),
                alpha: CanonicalF32::new(0.0).unwrap(),
            },
        });
        assert_eq!(
            list(vec![transparent_shadow])
                .propagated_bounds(&source)
                .unwrap(),
            source
        );
    }

    #[test]
    fn bounds_support_empty_negative_and_fractional_sources() {
        let effects = list(vec![ForegroundEffect::Blur(BlurEffect {
            sigma: CanonicalF32::new(0.5).unwrap(),
        })]);
        assert_eq!(
            effects
                .propagated_bounds(&LogicalRect {
                    x: -1.25,
                    y: 2.5,
                    width: 4.5,
                    height: 5.25,
                })
                .unwrap(),
            LogicalRect {
                x: -3.25,
                y: 0.5,
                width: 8.5,
                height: 9.25,
            }
        );
        let empty = LogicalRect {
            x: 1.0,
            y: 1.0,
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(effects.propagated_bounds(&empty).unwrap(), empty);
    }

    #[test]
    fn list_limits_and_future_metadata_are_bounded() {
        assert!(matches!(
            ForegroundEffectList::from_functions(
                id(),
                vec![scalar(ColorEffectKind::Brightness, 1.0); MAX_FOREGROUND_EFFECT_FUNCTIONS + 1]
            ),
            Err(ForegroundEffectRejection::FunctionCount)
        ));

        let smallest = CanonicalF32::new(f32::from_bits(1)).unwrap();
        let mut serialized_overflow = vec![scalar(ColorEffectKind::Brightness, smallest.get()); 15];
        serialized_overflow.push(ForegroundEffect::DropShadow(DropShadowEffect {
            offset_x: smallest,
            offset_y: smallest,
            sigma: smallest,
            color: EffectColor {
                red: smallest,
                green: smallest,
                blue: smallest,
                alpha: smallest,
            },
        }));
        assert_eq!(
            ForegroundEffectList::from_functions(id(), serialized_overflow),
            Err(ForegroundEffectRejection::SerializedLength)
        );
        assert_eq!(
            ForegroundEffectList::from_functions(
                id(),
                vec![
                    ForegroundEffect::Blur(BlurEffect {
                        sigma: CanonicalF32::new(MAX_FOREGROUND_BLUR_SIGMA).unwrap(),
                    });
                    3
                ],
            ),
            Err(ForegroundEffectRejection::ExpansionLimit)
        );

        assert!(
            validate_future_effect_layer_limits(
                MAX_EFFECT_LAYER_DIMENSION,
                MAX_EFFECT_LAYER_DIMENSION,
                MAX_EFFECT_IMAGE_BYTES,
                MAX_EFFECT_SURFACE_BYTES,
                MAX_EFFECT_PIPELINE_VARIANTS,
            )
            .is_ok()
        );
        assert!(
            validate_future_effect_layer_limits(MAX_EFFECT_LAYER_DIMENSION + 1, 1, 4, 4, 1,)
                .is_err()
        );
    }
}
