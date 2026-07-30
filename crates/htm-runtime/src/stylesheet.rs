use crate::RuntimeError;
use blitz_html::HtmlDocument;
use cssparser::{Parser, ParserInput, SourceLocation, Token};
use selectors::matching::QuirksMode;
use selectors::parser::{Combinator, Component as SelectorComponent};
use selectors::visitor::SelectorVisitor;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use stylo::error_reporting::{ContextualParseError, ParseErrorReporter};
use stylo::media_queries::MediaList;
use stylo::selector_parser::SelectorImpl;
use stylo::servo_arc::Arc as ServoArc;
use stylo::shared_lock::{SharedRwLock, ToCssWithGuard};
use stylo::stylesheets::{
    AllowImportRules, CssRule, DocumentStyleSheet, Origin, Stylesheet, StylesheetInDocument,
    UrlExtraData,
};
use url::Url;

const MAX_STYLESHEET_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CssParseIssue {
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentCssErrorKind {
    Parse,
    Import,
    UrlResource,
    FontResource,
    HostSelector,
    SlottedSelector,
    ShadowSelector,
    IdSelector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComponentCssError {
    pub(crate) kind: ComponentCssErrorKind,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedAuthorStylesheet {
    stylesheet: DocumentStyleSheet,
    semantic_version: Arc<str>,
    rule_count: usize,
    selector_count: usize,
}

impl PreparedAuthorStylesheet {
    pub(crate) fn stylesheet(&self) -> &DocumentStyleSheet {
        &self.stylesheet
    }

    pub(crate) fn semantic_version(&self) -> &str {
        &self.semantic_version
    }

    pub(crate) const fn rule_count(&self) -> usize {
        self.rule_count
    }

    pub(crate) const fn selector_count(&self) -> usize {
        self.selector_count
    }
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

/// Parses immutable package-owned CSS under a read-only Stylo lock. The
/// resulting sheet can be shared by every document retaining the package
/// snapshot because no document can mutate its rule storage.
pub(crate) fn prepare_component_author_stylesheet(
    css: &str,
    source_name: &str,
) -> Result<PreparedAuthorStylesheet, ComponentCssError> {
    if let Some(error) = forbidden_component_css_token(css) {
        return Err(error);
    }

    let reporter = CollectingReporter::default();
    let mut url =
        Url::parse("htm-component://package/").expect("the component stylesheet URL base is valid");
    url.set_path(source_name);
    let lock = SharedRwLock::read_only();
    let media = ServoArc::new(lock.wrap(MediaList::empty()));
    let stylesheet = Stylesheet::from_str(
        css,
        UrlExtraData::from(url),
        Origin::Author,
        media,
        lock,
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
    if let Some(issue) = issues.first() {
        return Err(ComponentCssError {
            kind: ComponentCssErrorKind::Parse,
            line: issue.line,
            column: issue.column,
            message: issue.message.clone(),
        });
    }

    let stylesheet = DocumentStyleSheet(ServoArc::new(stylesheet));
    let guard = stylesheet.0.shared_lock.read();
    let rules = stylesheet.contents(&guard).rules(&guard);
    let mut selector_check = SelectorRestriction::default();
    let (rule_count, selector_count) = inspect_component_rules(rules, &guard, &mut selector_check);
    if let Some(error) = selector_check.error {
        return Err(error);
    }
    let mut canonical = String::new();
    for rule in rules {
        if !canonical.is_empty() {
            canonical.push('\n');
        }
        canonical.push_str(&rule.to_css_string(&guard));
    }
    let semantic_version: Arc<str> = format!(
        "component-css-v1:{:016x}",
        stable_css_hash(canonical.as_bytes())
    )
    .into();
    drop(guard);
    Ok(PreparedAuthorStylesheet {
        stylesheet,
        semantic_version,
        rule_count,
        selector_count,
    })
}

fn stable_css_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn forbidden_component_css_token(css: &str) -> Option<ComponentCssError> {
    fn visit(parser: &mut Parser<'_, '_>, found: &mut Option<ComponentCssError>) {
        let mut consecutive_colons = 0u8;
        while found.is_none() {
            let location = parser.current_source_location();
            let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
                break;
            };
            let finding = match &token {
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => Some((
                    ComponentCssErrorKind::Import,
                    "CSS @import is not supported in component stylesheets",
                )),
                Token::AtKeyword(name) if name.eq_ignore_ascii_case("font-face") => Some((
                    ComponentCssErrorKind::FontResource,
                    "CSS @font-face is not supported in component stylesheets",
                )),
                Token::UnquotedUrl(_) => Some((
                    ComponentCssErrorKind::UrlResource,
                    "CSS URL resources are not supported in component stylesheets",
                )),
                Token::Function(name) if name.eq_ignore_ascii_case("url") => Some((
                    ComponentCssErrorKind::UrlResource,
                    "CSS URL resources are not supported in component stylesheets",
                )),
                Token::Ident(name)
                    if consecutive_colons == 1 && name.eq_ignore_ascii_case("host") =>
                {
                    Some((
                        ComponentCssErrorKind::HostSelector,
                        "the :host selector is not supported",
                    ))
                }
                Token::Function(name)
                    if consecutive_colons == 1 && name.eq_ignore_ascii_case("host") =>
                {
                    Some((
                        ComponentCssErrorKind::HostSelector,
                        "the :host() selector is not supported",
                    ))
                }
                Token::Function(name)
                    if consecutive_colons >= 2 && name.eq_ignore_ascii_case("slotted") =>
                {
                    Some((
                        ComponentCssErrorKind::SlottedSelector,
                        "the ::slotted() selector is not supported",
                    ))
                }
                Token::Function(name)
                    if consecutive_colons >= 2 && name.eq_ignore_ascii_case("part") =>
                {
                    Some((
                        ComponentCssErrorKind::ShadowSelector,
                        "shadow-tree selectors are not supported",
                    ))
                }
                _ => None,
            };
            if let Some((kind, message)) = finding {
                *found = Some(ComponentCssError {
                    kind,
                    line: location.line,
                    column: location.column,
                    message: message.to_owned(),
                });
                break;
            }
            if matches!(token, Token::Colon) {
                consecutive_colons = consecutive_colons.saturating_add(1);
                continue;
            }
            consecutive_colons = 0;
            if matches!(
                token,
                Token::Function(_)
                    | Token::ParenthesisBlock
                    | Token::SquareBracketBlock
                    | Token::CurlyBracketBlock
            ) {
                let _ = parser.parse_nested_block(|nested| {
                    visit(nested, found);
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
            }
        }
    }

    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut found = None;
    visit(&mut parser, &mut found);
    found
}

#[derive(Default)]
struct SelectorRestriction {
    error: Option<ComponentCssError>,
    line: u32,
    column: u32,
}

impl SelectorVisitor for SelectorRestriction {
    type Impl = SelectorImpl;

    fn visit_simple_selector(&mut self, selector: &SelectorComponent<Self::Impl>) -> bool {
        let finding = match selector {
            SelectorComponent::Host(_) => Some((
                ComponentCssErrorKind::HostSelector,
                "the :host selector is not supported",
            )),
            SelectorComponent::Slotted(_) => Some((
                ComponentCssErrorKind::SlottedSelector,
                "the ::slotted() selector is not supported",
            )),
            SelectorComponent::Part(_) => Some((
                ComponentCssErrorKind::ShadowSelector,
                "shadow-tree selectors are not supported",
            )),
            SelectorComponent::ID(_) => Some((
                ComponentCssErrorKind::IdSelector,
                "component-local ID selectors are not supported",
            )),
            _ => None,
        };
        if let Some((kind, message)) = finding {
            self.error = Some(ComponentCssError {
                kind,
                line: self.line,
                column: self.column,
                message: message.to_owned(),
            });
            return false;
        }
        true
    }

    fn visit_complex_selector(&mut self, combinator_to_right: Option<Combinator>) -> bool {
        if matches!(
            combinator_to_right,
            Some(Combinator::Part | Combinator::SlotAssignment)
        ) {
            self.error = Some(ComponentCssError {
                kind: ComponentCssErrorKind::ShadowSelector,
                line: self.line,
                column: self.column,
                message: "shadow-tree selector traversal is not supported".to_owned(),
            });
            return false;
        }
        true
    }
}

fn inspect_component_rules(
    rules: &[CssRule],
    guard: &stylo::shared_lock::SharedRwLockReadGuard<'_>,
    restriction: &mut SelectorRestriction,
) -> (usize, usize) {
    let mut rule_count = 0usize;
    let mut selector_count = 0usize;
    for rule in rules {
        rule_count = rule_count.saturating_add(1);
        if let CssRule::FontFace(_) = rule {
            restriction.error = Some(ComponentCssError {
                kind: ComponentCssErrorKind::FontResource,
                line: 0,
                column: 0,
                message: "CSS @font-face is not supported in component stylesheets".to_owned(),
            });
            return (rule_count, selector_count);
        }
        if let CssRule::Import(_) = rule {
            restriction.error = Some(ComponentCssError {
                kind: ComponentCssErrorKind::Import,
                line: 0,
                column: 0,
                message: "CSS @import is not supported in component stylesheets".to_owned(),
            });
            return (rule_count, selector_count);
        }
        if let CssRule::Style(style) = rule {
            let style = style.read_with(guard);
            restriction.line = style.source_location.line;
            restriction.column = style.source_location.column;
            selector_count = selector_count.saturating_add(style.selectors.slice().len());
            for selector in style.selectors.slice() {
                if !selector.visit(restriction) {
                    return (rule_count, selector_count);
                }
            }
        }
        let (child_rules, child_selectors) =
            inspect_component_rules(rule.children(guard), guard, restriction);
        rule_count = rule_count.saturating_add(child_rules);
        selector_count = selector_count.saturating_add(child_selectors);
        if restriction.error.is_some() {
            return (rule_count, selector_count);
        }
    }
    (rule_count, selector_count)
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

    #[test]
    fn component_stylesheet_is_immutable_counted_and_semantically_versioned() {
        let first = prepare_component_author_stylesheet(
            ".card, article[data-density=\"compact\"] { color: rgb(1, 2, 3); }",
            "components/card.css",
        )
        .unwrap();
        let equivalent = prepare_component_author_stylesheet(
            ".card,article[data-density=compact]{color:rgb(1,2,3)}",
            "components/equivalent.css",
        )
        .unwrap();
        assert_eq!(first.rule_count(), 1);
        assert_eq!(first.selector_count(), 2);
        assert_eq!(first.semantic_version(), equivalent.semantic_version());
    }

    #[test]
    fn component_stylesheet_rejects_resource_and_scope_features_by_token_or_ast() {
        let cases = [
            ("@import \"theme.css\";", ComponentCssErrorKind::Import),
            (
                ".card { background-image: url(image.png); }",
                ComponentCssErrorKind::UrlResource,
            ),
            (
                "@font-face { font-family: local; src: url(font.woff2); }",
                ComponentCssErrorKind::FontResource,
            ),
            (":host { color: red; }", ComponentCssErrorKind::HostSelector),
            (
                "::slotted(.item) { color: red; }",
                ComponentCssErrorKind::SlottedSelector,
            ),
            (
                "::part(label) { color: red; }",
                ComponentCssErrorKind::ShadowSelector,
            ),
            ("#local { color: red; }", ComponentCssErrorKind::IdSelector),
        ];
        for (css, expected) in cases {
            let error =
                prepare_component_author_stylesheet(css, "components/rejected.css").unwrap_err();
            assert_eq!(
                error.kind, expected,
                "unexpected classification for `{css}`"
            );
        }
        assert!(
            prepare_component_author_stylesheet(
                ".card::before { content: \"url(example.png) @import\"; }",
                "components/literal.css",
            )
            .is_ok()
        );
    }
}
