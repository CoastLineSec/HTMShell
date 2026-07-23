use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ShellHostError {
    Manifest(String),
    Wayland(String),
    MissingGlobal(&'static str),
    MissingPointerCapability,
    UnsupportedShmFormat,
    InvalidDimensions(String),
    Buffer(String),
    Clock(String),
    Runtime(htm_runtime::RuntimeError),
    Io {
        operation: &'static str,
        path: Option<PathBuf>,
        source: std::io::Error,
    },
}

impl ShellHostError {
    pub(crate) fn wayland(error: impl fmt::Display) -> Self {
        Self::Wayland(error.to_string())
    }

    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io {
            operation,
            path: None,
            source,
        }
    }
}

impl fmt::Display for ShellHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(message) => write!(f, "shell manifest error: {message}"),
            Self::Wayland(message) => write!(f, "Wayland error: {message}"),
            Self::MissingGlobal(interface) => {
                write!(f, "required Wayland global `{interface}` is unavailable")
            }
            Self::MissingPointerCapability => {
                write!(f, "the selected Wayland seat has no pointer capability")
            }
            Self::UnsupportedShmFormat => {
                write!(f, "the compositor did not advertise WL_SHM_FORMAT_ARGB8888")
            }
            Self::InvalidDimensions(message) => write!(f, "invalid surface dimensions: {message}"),
            Self::Buffer(message) => write!(f, "shared-memory buffer error: {message}"),
            Self::Clock(message) => write!(f, "clock service error: {message}"),
            Self::Runtime(error) => write!(f, "HTMShell runtime error: {error}"),
            Self::Io {
                operation,
                path,
                source,
            } => match path {
                Some(path) => write!(f, "failed to {operation} {}: {source}", path.display()),
                None => write!(f, "failed to {operation}: {source}"),
            },
        }
    }
}

impl std::error::Error for ShellHostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<htm_runtime::RuntimeError> for ShellHostError {
    fn from(value: htm_runtime::RuntimeError) -> Self {
        Self::Runtime(value)
    }
}
