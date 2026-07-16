use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum RuntimeError {
    InvalidPackage(String),
    LimitExceeded(String),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Serialization(serde_json::Error),
    Png(String),
    EnginePanic(String),
}

impl RuntimeError {
    pub(crate) fn io(
        operation: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackage(message) => write!(f, "invalid shell package: {message}"),
            Self::LimitExceeded(message) => write!(f, "fixture limit exceeded: {message}"),
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "failed to {operation} {}: {source}", path.display()),
            Self::Serialization(source) => write!(f, "failed to serialize diagnostics: {source}"),
            Self::Png(message) => write!(f, "failed to encode PNG: {message}"),
            Self::EnginePanic(message) => write!(f, "Blitz runtime panicked: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Serialization(source) => Some(source),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for RuntimeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}
