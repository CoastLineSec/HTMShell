use htm_runtime::{LiveDocument, LiveDocumentKind};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "htmshell-root-svg-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn document(&self, content: &str) {
        std::fs::write(self.0.join("panel.html"), format!(r#"<!doctype html><html><head><style>html,body{{margin:0;background:black}}img{{width:32px;height:32px}}</style></head><body><main id="panel-root">{content}<button id="overlay-toggle" style="display:none">Details</button></main></body></html>"#)).unwrap();
    }
    fn load(&self) -> Result<LiveDocument, htm_runtime::RuntimeError> {
        LiveDocument::load_surface_document(&self.0, "panel.html", LiveDocumentKind::Panel, 32, 32)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn renamed_svg_secondary_images_do_not_reach_the_decoder() {
    let fixture = Fixture::new();
    // A harmless known repository image lies outside this fixture's authority.
    let outside = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/package-graph/packages/controls/assets/status-orb.png");
    assert!(outside.is_file());
    for extension in ["svg", "bin", "png"] {
        std::fs::write(fixture.0.join(format!("outer.{extension}")), format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image href="{}" width="32" height="32"/></svg>"#, outside.display())).unwrap();
        fixture.document(&format!(r#"<img src="outer.{extension}">"#));
        let mut live = fixture.load().unwrap();
        let pixels = live.render().unwrap().premultiplied_rgba;
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == [0, 0, 0]),
            "{extension}"
        );
    }
}

#[test]
fn inline_svg_subresources_reject_before_layout() {
    let fixture = Fixture::new();
    fixture.document(r#"<svg width="32" height="32"><image href="/outside.png"/></svg>"#);
    assert!(fixture.load().is_err());
    fixture
        .document(r#"<svg width="32" height="32"><rect width="32" height="32" fill="red"/></svg>"#);
    let mut live = fixture.load().unwrap();
    assert!(
        live.render()
            .unwrap()
            .premultiplied_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0)
    );
}
