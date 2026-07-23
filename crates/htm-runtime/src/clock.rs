use crate::RuntimeError;
use std::fmt;

pub const MAX_CLOCK_FORMAT_BYTES: usize = 128;
pub const MAX_CLOCK_OUTPUT_BYTES: usize = 256;
pub const MAX_CLOCK_DECLARATIONS_PER_DOCUMENT: usize = 64;
pub const MAX_CLOCK_DECLARATIONS_PER_PROCESS: usize = 256;
pub const MAX_CLOCK_FORMATS_PER_PROCESS: usize = 128;
pub const MAX_CLOCK_ZONES_PER_PROCESS: usize = 64;
pub const MAX_CLOCK_ZONE_BYTES: usize = 128;

pub const CLOCK_PUBLIC_ATTRIBUTES: [&str; 7] = [
    "data-htm-format",
    "data-htm-time-zone",
    "data-htm-enabled",
    "data-htm-target",
    "datetime",
    "data-htm-state",
    "data-htm-element",
];

pub const CLOCK_FORMAT_CONVERSIONS: &[&str] = &[
    "%%", "%A", "%a", "%B", "%b", "%h", "%P", "%p", "%C", "%D", "%d", "%e", "%F", "%G", "%g", "%j",
    "%m", "%q", "%U", "%u", "%V", "%W", "%w", "%Y", "%y", "%H", "%I", "%k", "%l", "%M", "%R", "%S",
    "%T", "%Q", "%:Q", "%Z", "%z", "%:z", "%::z", "%:::z",
];

pub const CLOCK_FORMAT_FLAGS: &[char] = &['-', '_', '0', '^', '#'];

pub const CLOCK_REJECTED_CONVERSIONS: &[&str] = &[
    "%c", "%r", "%X", "%x", "%f", "%.f", "%N", "%s", "%n", "%t", "%+", "%E", "%O",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClockCadence {
    Static,
    ZoneTransitionOnly,
    Day,
    Hour,
    Minute,
    Second,
}

impl ClockCadence {
    fn include(self, other: Self) -> Self {
        use ClockCadence::{Day, Hour, Minute, Second, Static, ZoneTransitionOnly};
        match (self, other) {
            (Second, _) | (_, Second) => Second,
            (Minute, _) | (_, Minute) => Minute,
            (Hour, _) | (_, Hour) => Hour,
            (Day, _) | (_, Day) => Day,
            (ZoneTransitionOnly, _) | (_, ZoneTransitionOnly) => ZoneTransitionOnly,
            (Static, Static) => Static,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockFormatFlag {
    NoPadding,
    SpacePadding,
    ZeroPadding,
    Uppercase,
    Swapcase,
}

impl ClockFormatFlag {
    const fn as_char(self) -> char {
        match self {
            Self::NoPadding => '-',
            Self::SpacePadding => '_',
            Self::ZeroPadding => '0',
            Self::Uppercase => '^',
            Self::Swapcase => '#',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockDirective {
    pub flag: Option<ClockFormatFlag>,
    pub width: Option<u8>,
    pub colons: u8,
    pub conversion: char,
}

impl ClockDirective {
    pub fn source(self) -> String {
        let mut source = String::from("%");
        if let Some(flag) = self.flag {
            source.push(flag.as_char());
        }
        if let Some(width) = self.width {
            source.push_str(&width.to_string());
        }
        for _ in 0..self.colons {
            source.push(':');
        }
        source.push(self.conversion);
        source
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockFormatPart {
    Literal(String),
    Directive(ClockDirective),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockFormat {
    source: String,
    parts: Vec<ClockFormatPart>,
    cadence: ClockCadence,
    observes_zone_transition: bool,
    output_bound: usize,
}

impl ClockFormat {
    pub fn compile(source: &str) -> Result<Self, ClockFormatError> {
        if source.is_empty() {
            return Err(ClockFormatError::new("format must not be empty"));
        }
        if source.len() > MAX_CLOCK_FORMAT_BYTES {
            return Err(ClockFormatError::new(format!(
                "format exceeds {MAX_CLOCK_FORMAT_BYTES} UTF-8 bytes"
            )));
        }
        if source
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r' || ch == '\t' || ch.is_control())
        {
            return Err(ClockFormatError::new(
                "format contains an unsupported control character",
            ));
        }

        let bytes = source.as_bytes();
        let mut parts = Vec::new();
        let mut literal_start = 0usize;
        let mut index = 0usize;
        let mut cadence = ClockCadence::Static;
        let mut observes_zone_transition = false;
        let mut output_bound = 0usize;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                index += 1;
                continue;
            }
            if literal_start < index {
                let literal = source[literal_start..index].to_owned();
                output_bound = output_bound.saturating_add(literal.len());
                parts.push(ClockFormatPart::Literal(literal));
            }
            let directive_start = index;
            index += 1;
            if index >= bytes.len() {
                return Err(ClockFormatError::at(
                    directive_start,
                    "incomplete `%` conversion",
                ));
            }
            let flag = match bytes[index] {
                b'-' => Some(ClockFormatFlag::NoPadding),
                b'_' => Some(ClockFormatFlag::SpacePadding),
                b'0' => Some(ClockFormatFlag::ZeroPadding),
                b'^' => Some(ClockFormatFlag::Uppercase),
                b'#' => Some(ClockFormatFlag::Swapcase),
                _ => None,
            };
            if flag.is_some() {
                index += 1;
                if index >= bytes.len() {
                    return Err(ClockFormatError::at(
                        directive_start,
                        "format flag has no conversion",
                    ));
                }
            }
            let width_start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let width = if index > width_start {
                let width = source[width_start..index].parse::<u8>().map_err(|_| {
                    ClockFormatError::at(directive_start, "format width is invalid")
                })?;
                if !(1..=20).contains(&width) {
                    return Err(ClockFormatError::at(
                        directive_start,
                        "format width must be from 1 through 20",
                    ));
                }
                Some(width)
            } else {
                None
            };
            let mut colons = 0u8;
            while index < bytes.len() && bytes[index] == b':' {
                colons = colons.saturating_add(1);
                index += 1;
            }
            if index >= bytes.len() {
                return Err(ClockFormatError::at(
                    directive_start,
                    "format modifier has no conversion",
                ));
            }
            let conversion = bytes[index] as char;
            if !bytes[index].is_ascii() {
                return Err(ClockFormatError::at(
                    directive_start,
                    "format conversion must be ASCII",
                ));
            }
            index += 1;
            let directive = ClockDirective {
                flag,
                width,
                colons,
                conversion,
            };
            validate_directive(directive, directive_start)?;
            cadence = cadence.include(directive_cadence(directive));
            observes_zone_transition |= directive_observes_zone(directive);
            output_bound = output_bound.saturating_add(directive_output_bound(directive));
            parts.push(ClockFormatPart::Directive(directive));
            literal_start = index;
        }
        if literal_start < source.len() {
            let literal = source[literal_start..].to_owned();
            output_bound = output_bound.saturating_add(literal.len());
            parts.push(ClockFormatPart::Literal(literal));
        }
        if output_bound > MAX_CLOCK_OUTPUT_BYTES {
            return Err(ClockFormatError::new(format!(
                "format can exceed the {MAX_CLOCK_OUTPUT_BYTES}-byte output limit"
            )));
        }
        Ok(Self {
            source: source.to_owned(),
            parts,
            cadence,
            observes_zone_transition,
            output_bound,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn parts(&self) -> &[ClockFormatPart] {
        &self.parts
    }

    pub const fn cadence(&self) -> ClockCadence {
        self.cadence
    }

    pub const fn observes_zone_transition(&self) -> bool {
        self.observes_zone_transition
    }

    pub const fn output_bound(&self) -> usize {
        self.output_bound
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockFormatError {
    message: String,
}

impl ClockFormatError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn at(byte: usize, message: impl fmt::Display) -> Self {
        Self::new(format!("format byte {byte}: {message}"))
    }
}

impl fmt::Display for ClockFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ClockFormatError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClockTimeZone {
    Local,
    Utc,
    Named(String),
}

impl ClockTimeZone {
    pub fn parse(value: Option<&str>) -> Result<Self, RuntimeError> {
        let value = value.unwrap_or("local");
        if value.is_empty() {
            return Err(RuntimeError::InvalidPackage(
                "clock time zone must not be empty".into(),
            ));
        }
        if value.len() > MAX_CLOCK_ZONE_BYTES {
            return Err(RuntimeError::LimitExceeded(format!(
                "clock time zone exceeds {MAX_CLOCK_ZONE_BYTES} UTF-8 bytes"
            )));
        }
        match value {
            "local" => return Ok(Self::Local),
            "UTC" => return Ok(Self::Utc),
            _ => {}
        }
        if value.eq_ignore_ascii_case("local") || value.eq_ignore_ascii_case("UTC") {
            return Err(RuntimeError::InvalidPackage(format!(
                "clock time zone `{value}` must use exact `local` or `UTC` spelling"
            )));
        }
        if value.starts_with('/')
            || value.starts_with('.')
            || value.contains('\\')
            || (!value.contains('/') && value.bytes().any(|byte| byte.is_ascii_digit()))
            || value.split('/').any(|part| {
                part.is_empty()
                    || part == "."
                    || part == ".."
                    || !part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+')
                    })
            })
        {
            return Err(RuntimeError::InvalidPackage(format!(
                "clock time zone `{value}` is not a local, UTC, or IANA identifier"
            )));
        }
        Ok(Self::Named(value.to_owned()))
    }

    pub fn declaration_value(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Utc => "UTC",
            Self::Named(value) => value,
        }
    }
}

fn validate_directive(directive: ClockDirective, byte: usize) -> Result<(), ClockFormatError> {
    let conversion = directive.conversion;
    let accepted = matches!(
        conversion,
        '%' | 'A'
            | 'a'
            | 'B'
            | 'b'
            | 'h'
            | 'P'
            | 'p'
            | 'C'
            | 'D'
            | 'd'
            | 'e'
            | 'F'
            | 'G'
            | 'g'
            | 'j'
            | 'm'
            | 'q'
            | 'U'
            | 'u'
            | 'V'
            | 'W'
            | 'w'
            | 'Y'
            | 'y'
            | 'H'
            | 'I'
            | 'k'
            | 'l'
            | 'M'
            | 'R'
            | 'S'
            | 'T'
            | 'Q'
            | 'Z'
            | 'z'
    );
    if !accepted {
        return Err(ClockFormatError::at(
            byte,
            format!("unsupported conversion `%{conversion}`"),
        ));
    }
    let valid_colons = match conversion {
        'Q' => directive.colons <= 1,
        'z' => directive.colons <= 3,
        _ => directive.colons == 0,
    };
    if !valid_colons {
        return Err(ClockFormatError::at(byte, "unsupported colon modifier"));
    }
    let numeric = matches!(
        conversion,
        'C' | 'd'
            | 'e'
            | 'G'
            | 'g'
            | 'j'
            | 'm'
            | 'q'
            | 'U'
            | 'u'
            | 'V'
            | 'W'
            | 'w'
            | 'Y'
            | 'y'
            | 'H'
            | 'I'
            | 'k'
            | 'l'
            | 'M'
            | 'S'
    );
    let named = matches!(conversion, 'A' | 'a' | 'B' | 'b' | 'h' | 'P' | 'p' | 'Z');
    let extensions_ok = if numeric {
        directive.flag.is_none()
            || matches!(
                directive.flag,
                Some(
                    ClockFormatFlag::NoPadding
                        | ClockFormatFlag::SpacePadding
                        | ClockFormatFlag::ZeroPadding
                )
            )
    } else if named {
        directive.width.is_none()
            && (directive.flag.is_none()
                || matches!(
                    directive.flag,
                    Some(ClockFormatFlag::Uppercase | ClockFormatFlag::Swapcase)
                ))
    } else {
        directive.flag.is_none() && directive.width.is_none()
    };
    if !extensions_ok
        || (directive.width.is_some() && !numeric)
        || (directive.width.is_some() && directive.flag == Some(ClockFormatFlag::NoPadding))
    {
        return Err(ClockFormatError::at(
            byte,
            "flag or width does not affect this conversion",
        ));
    }
    Ok(())
}

fn directive_cadence(directive: ClockDirective) -> ClockCadence {
    match directive.conversion {
        'S' | 'T' => ClockCadence::Second,
        'M' | 'R' => ClockCadence::Minute,
        'H' | 'I' | 'k' | 'l' | 'p' | 'P' => ClockCadence::Hour,
        'C' | 'D' | 'd' | 'e' | 'F' | 'G' | 'g' | 'j' | 'm' | 'q' | 'U' | 'u' | 'V' | 'W' | 'w'
        | 'Y' | 'y' | 'A' | 'a' | 'B' | 'b' | 'h' => ClockCadence::Day,
        'Z' | 'z' => ClockCadence::ZoneTransitionOnly,
        '%' | 'Q' => ClockCadence::Static,
        _ => unreachable!("validated conversion"),
    }
}

fn directive_observes_zone(directive: ClockDirective) -> bool {
    matches!(directive.conversion, 'Z' | 'z')
}

fn directive_output_bound(directive: ClockDirective) -> usize {
    let natural = match directive.conversion {
        '%' => 1,
        'A' | 'B' => 9,
        'a' | 'b' | 'h' => 3,
        'P' | 'p' => 2,
        'C' | 'd' | 'e' | 'g' | 'm' | 'q' | 'U' | 'u' | 'V' | 'W' | 'w' | 'y' | 'H' | 'I' | 'k'
        | 'l' | 'M' | 'S' => 4,
        'G' | 'Y' => 7,
        'j' => 3,
        'D' => 8,
        'F' => 10,
        'R' => 5,
        'T' => 8,
        'Q' => MAX_CLOCK_ZONE_BYTES,
        'Z' => 16,
        'z' => 10,
        _ => 32,
    };
    directive
        .width
        .map_or(natural, |width| natural.max(usize::from(width)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_profile_compiles() {
        for conversion in CLOCK_FORMAT_CONVERSIONS {
            ClockFormat::compile(conversion)
                .unwrap_or_else(|error| panic!("{conversion} should compile: {error}"));
        }
        for source in ["%-I", "%_4d", "%04Y", "%^A", "%#p", "%20Y"] {
            ClockFormat::compile(source)
                .unwrap_or_else(|error| panic!("{source} should compile: {error}"));
        }
    }

    #[test]
    fn rejected_profile_and_ignored_extensions_fail() {
        for conversion in CLOCK_REJECTED_CONVERSIONS {
            assert!(
                ClockFormat::compile(conversion).is_err(),
                "{conversion} should fail"
            );
        }
        for source in [
            "", "%", "%21Y", "%0", "%_A", "%-4I", "%-z", "%3Q", "%:Y", "%::Q",
        ] {
            assert!(
                ClockFormat::compile(source).is_err(),
                "{source} should fail"
            );
        }
    }

    #[test]
    fn cadence_is_inferred_from_visible_fields() {
        for (source, cadence) in [
            ("HTMShell %%", ClockCadence::Static),
            ("%Q", ClockCadence::Static),
            ("%Z", ClockCadence::ZoneTransitionOnly),
            ("%F", ClockCadence::Day),
            ("%H %p", ClockCadence::Hour),
            ("%H:%M", ClockCadence::Minute),
            ("%F %T", ClockCadence::Second),
        ] {
            assert_eq!(ClockFormat::compile(source).unwrap().cadence(), cadence);
        }
    }

    #[test]
    fn time_zone_syntax_rejects_paths_and_posix_rules() {
        assert_eq!(ClockTimeZone::parse(None).unwrap(), ClockTimeZone::Local);
        assert_eq!(
            ClockTimeZone::parse(Some("UTC")).unwrap(),
            ClockTimeZone::Utc
        );
        assert!(matches!(
            ClockTimeZone::parse(Some("Asia/Tokyo")).unwrap(),
            ClockTimeZone::Named(_)
        ));
        for value in [
            "",
            "utc",
            "/usr/share/zoneinfo/UTC",
            "../UTC",
            "America/../UTC",
            "EST5EDT",
            "EST5EDT,M3.2.0,M11.1.0",
        ] {
            assert!(ClockTimeZone::parse(Some(value)).is_err(), "{value}");
        }
    }
}
