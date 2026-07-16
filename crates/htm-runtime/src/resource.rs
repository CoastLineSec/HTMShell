use crate::model::ResourceRecord;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const VIRTUAL_SCHEME: &str = "htm-local";
const VIRTUAL_HOST: &str = "package";
const VIRTUAL_PREFIX: &str = "/root/";
const MAX_RESOURCE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct ResourceAudit {
    records: Mutex<Vec<ResourceRecord>>,
    requests: AtomicUsize,
}

impl ResourceAudit {
    pub(crate) fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    pub(crate) fn records(&self) -> Vec<ResourceRecord> {
        let mut records = self
            .records
            .lock()
            .expect("resource audit poisoned")
            .clone();
        records.sort();
        records
    }

    fn push(&self, record: ResourceRecord) {
        self.records
            .lock()
            .expect("resource audit poisoned")
            .push(record);
    }
}

pub(crate) struct LocalOnlyResourceProvider {
    root: PathBuf,
    audit: Arc<ResourceAudit>,
}

impl LocalOnlyResourceProvider {
    pub(crate) fn new(root: PathBuf, audit: Arc<ResourceAudit>) -> Self {
        Self { root, audit }
    }

    pub(crate) fn virtual_document_url() -> &'static str {
        "htm-local://package/root/index.html"
    }

    fn reject(
        &self,
        url: &str,
        resource_kind: &str,
        decision: &str,
        detail: impl Into<String>,
        handler: Box<dyn NetHandler>,
    ) {
        self.audit.push(ResourceRecord {
            url: url.to_owned(),
            resource_kind: resource_kind.to_owned(),
            decision: decision.to_owned(),
            detail: detail.into(),
            byte_count: None,
        });
        handler.bytes(url.to_owned(), Bytes::new());
    }

    fn resource_kind(path: &str) -> &'static str {
        match Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("css") => "stylesheet",
            Some("svg") => "svg",
            Some("png" | "jpg" | "jpeg" | "gif" | "webp") => "image",
            Some("ttf" | "otf") => "font",
            _ => "resource",
        }
    }

    fn validate_svg(bytes: &[u8]) -> Result<(), &'static str> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Err("SVG is not UTF-8");
        };
        let lowered = text.to_ascii_lowercase();
        const REJECTED: [&str; 9] = [
            "<script",
            "<foreignobject",
            "href=\"http://",
            "href=\"https://",
            "href=\"//",
            "href='//",
            "url(http",
            "file://",
            "javascript:",
        ];
        if REJECTED.iter().any(|needle| lowered.contains(needle)) {
            return Err("SVG contains an active or external reference");
        }
        Ok(())
    }

    fn map_url(&self, request: &Request) -> Result<PathBuf, String> {
        let url = &request.url;
        if url.scheme() == "http" || url.scheme() == "https" {
            return Err("network URL rejected".to_owned());
        }
        if url.scheme() == "file" {
            return Err("absolute filesystem URL rejected".to_owned());
        }
        if url.scheme() != VIRTUAL_SCHEME {
            return Err(format!("unsupported URL scheme `{}`", url.scheme()));
        }
        if url.host_str() != Some(VIRTUAL_HOST) {
            return Err("protocol-relative or foreign package host rejected".to_owned());
        }
        if url.query().is_some() {
            return Err("resource query strings are outside this spike's profile".to_owned());
        }
        if url.path().contains('%') {
            return Err(
                "percent-encoded resource paths are outside this spike's profile".to_owned(),
            );
        }

        let relative = url
            .path()
            .strip_prefix(VIRTUAL_PREFIX)
            .ok_or_else(|| "path traversal or package-root escape rejected".to_owned())?;
        if relative.is_empty() {
            return Err("resource path resolves to the package root".to_owned());
        }

        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("local resource is missing or inaccessible: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err("symlink or canonical path escapes the package root".to_owned());
        }
        Ok(canonical)
    }
}

impl NetProvider for LocalOnlyResourceProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.audit.requests.fetch_add(1, Ordering::SeqCst);
        let url = request.url.to_string();
        let resource_kind = Self::resource_kind(request.url.path());

        if request
            .signal
            .as_ref()
            .is_some_and(|signal| signal.aborted())
        {
            self.reject(
                &url,
                resource_kind,
                "rejected",
                "request was aborted",
                handler,
            );
            return;
        }

        let path = match self.map_url(&request) {
            Ok(path) => path,
            Err(detail) => {
                let decision = if detail.contains("missing") {
                    "missing"
                } else {
                    "rejected"
                };
                self.reject(&url, resource_kind, decision, detail, handler);
                return;
            }
        };

        let metadata = match path.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                self.reject(
                    &url,
                    resource_kind,
                    "rejected",
                    "resource is not a regular file",
                    handler,
                );
                return;
            }
            Err(error) => {
                self.reject(
                    &url,
                    resource_kind,
                    "missing",
                    format!("could not inspect local resource: {error}"),
                    handler,
                );
                return;
            }
        };
        if metadata.len() > MAX_RESOURCE_BYTES {
            self.reject(
                &url,
                resource_kind,
                "rejected",
                format!("resource exceeds {MAX_RESOURCE_BYTES} bytes"),
                handler,
            );
            return;
        }

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.reject(
                    &url,
                    resource_kind,
                    "missing",
                    format!("could not read local resource: {error}"),
                    handler,
                );
                return;
            }
        };

        if resource_kind == "svg"
            && let Err(detail) = Self::validate_svg(&bytes)
        {
            self.reject(&url, resource_kind, "rejected", detail, handler);
            return;
        }

        self.audit.push(ResourceRecord {
            url: url.clone(),
            resource_kind: resource_kind.to_owned(),
            decision: "loaded".to_owned(),
            detail: path
                .strip_prefix(&self.root)
                .unwrap_or(&path)
                .display()
                .to_string(),
            byte_count: Some(bytes.len()),
        });
        handler.bytes(url, Bytes::from(bytes));
    }
}
