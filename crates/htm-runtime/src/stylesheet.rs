use crate::RuntimeError;
use blitz_html::HtmlDocument;
use cssparser::SourceLocation;
use selectors::matching::QuirksMode;
use std::path::{Component, Path};
use std::sync::Mutex;
use stylo::error_reporting::{ContextualParseError, ParseErrorReporter};
use stylo::media_queries::MediaList;
use stylo::servo_arc::Arc as ServoArc;
use stylo::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use url::Url;

const MAX_STYLESHEET_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CssParseIssue {
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) message: String,
}

#[derive(Default)]
struct CollectingReporter {
    issues: Mutex<Vec<CssParseIssue>>,
}

impl ParseErrorReporter for CollectingReporter {
    fn report_error(
        &self,
        _url: &UrlExtraData,
        location: SourceLocation,
        error: ContextualParseError,
    ) {
        self.issues
            .lock()
            .expect("CSS parse issue collector poisoned")
            .push(CssParseIssue {
                line: location.line,
                column: location.column,
                message: error.to_string(),
            });
    }
}

pub(crate) fn load_candidate_css(
    package_root: &Path,
    relative_path: &Path,
) -> Result<String, RuntimeError> {
    let raw = relative_path.to_string_lossy();
    if raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("//")
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(RuntimeError::StylesheetRejected(format!(
            "candidate path `{}` is not package-relative",
            relative_path.display()
        )));
    }
    let candidate = package_root.join(relative_path);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| RuntimeError::io("resolve replacement stylesheet", &candidate, error))?;
    if !canonical.starts_with(package_root) {
        return Err(RuntimeError::StylesheetRejected(format!(
            "candidate path `{}` escapes the package root",
            relative_path.display()
        )));
    }
    let metadata = canonical
        .metadata()
        .map_err(|error| RuntimeError::io("inspect replacement stylesheet", &canonical, error))?;
    if !metadata.is_file() {
        return Err(RuntimeError::StylesheetRejected(format!(
            "candidate `{}` is not a regular file",
            relative_path.display()
        )));
    }
    if metadata.len() > MAX_STYLESHEET_BYTES {
        return Err(RuntimeError::LimitExceeded(format!(
            "replacement stylesheet is {} bytes; limit is {MAX_STYLESHEET_BYTES}",
            metadata.len()
        )));
    }
    std::fs::read_to_string(&canonical)
        .map_err(|error| RuntimeError::io("read replacement stylesheet as UTF-8", canonical, error))
}

/// Parses a complete author sheet without attaching it to the live document.
/// Any Stylo parse diagnostic rejects the candidate conservatively.
pub(crate) fn prepare_author_stylesheet(
    document: &HtmlDocument,
    css: &str,
    source_name: &str,
) -> Result<DocumentStyleSheet, RuntimeError> {
    let reporter = CollectingReporter::default();
    let url = Url::parse(&format!("htm-local://package/root/{source_name}"))
        .map_err(|error| RuntimeError::StylesheetRejected(error.to_string()))?;
    let guard = document.guard().clone();
    let media = ServoArc::new(guard.wrap(MediaList::empty()));
    let stylesheet = Stylesheet::from_str(
        css,
        UrlExtraData::from(url),
        Origin::Author,
        media,
        guard,
        None,
        Some(&reporter),
        QuirksMode::NoQuirks,
        AllowImportRules::No,
    );
    let mut issues = reporter
        .issues
        .into_inner()
        .expect("CSS parse issue collector poisoned");
    issues.sort();
    issues.dedup();
    if !issues.is_empty() {
        let details = issues
            .into_iter()
            .map(|issue| format!("{}:{}: {}", issue.line, issue.column, issue.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(RuntimeError::StylesheetRejected(format!(
            "candidate `{source_name}` produced CSS parse errors: {details}"
        )));
    }
    Ok(DocumentStyleSheet(ServoArc::new(stylesheet)))
}

pub(crate) fn replace_author_stylesheet(
    document: &mut HtmlDocument,
    owner_node: usize,
    stylesheet: DocumentStyleSheet,
) -> Result<(), RuntimeError> {
    let node = document.get_node(owner_node).ok_or_else(|| {
        RuntimeError::InvalidMutationTarget(format!(
            "stylesheet owner Blitz slot {owner_node} is not live"
        ))
    })?;
    if node.element_data().is_none() {
        return Err(RuntimeError::InvalidMutationTarget(format!(
            "stylesheet owner Blitz slot {owner_node} is not an element"
        )));
    }
    document.add_stylesheet_for_node(stylesheet, owner_node);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::DocumentConfig;
    use blitz_html::HtmlProvider;
    use std::sync::Arc;

    fn document() -> HtmlDocument {
        HtmlDocument::from_html(
            "<!doctype html><html><head></head><body></body></html>",
            DocumentConfig {
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                ..Default::default()
            },
        )
    }

    #[test]
    fn candidate_preflight_accepts_clean_css_and_rejects_errors() {
        let document = document();
        assert!(prepare_author_stylesheet(&document, "body { color: white; }", "ok.css").is_ok());
        assert!(prepare_author_stylesheet(&document, "body { color: ; }", "broken.css").is_err());
        assert!(
            prepare_author_stylesheet(
                &document,
                "@import 'https://example.invalid/theme.css'; body { color: white; }",
                "remote-import.css",
            )
            .is_err()
        );
    }

    #[test]
    fn candidate_loader_rejects_url_and_parent_syntax_before_io() {
        let root = std::env::temp_dir();
        for path in [
            "http://example.invalid/a.css",
            "https://example.invalid/a.css",
            "../a.css",
        ] {
            assert!(matches!(
                load_candidate_css(&root, Path::new(path)),
                Err(RuntimeError::StylesheetRejected(_))
            ));
        }
    }
}
