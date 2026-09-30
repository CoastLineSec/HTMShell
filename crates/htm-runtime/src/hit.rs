//! Read-only control hit testing using the renderer's paint-child topology.
//! Blitz supplies inline glyph hits; HTMShell additionally enforces paint clips.

use blitz_dom::Node;
use blitz_html::HtmlDocument;
use kurbo::Point;
use std::collections::BTreeSet;
use stylo::computed_values::{pointer_events::T as PointerEvents, visibility::T as Visibility};

pub(crate) fn control_hit_path(document: &HtmlDocument, x: f32, y: f32) -> Vec<usize> {
    let viewport = document.viewport();
    let scale = viewport.scale_f64();
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.0
        || y < 0.0
        || f64::from(x) >= f64::from(viewport.window_size.0) / scale
        || f64::from(y) >= f64::from(viewport.window_size.1) / scale
    {
        return Vec::new();
    }
    let Some(root) = document.try_root_element() else {
        return Vec::new();
    };
    let mut visited = BTreeSet::new();
    let Some(mut slot) = hit_node(document, root, x, y, scale, &mut visited) else {
        return Vec::new();
    };
    let mut path = Vec::new();
    loop {
        path.push(slot);
        let Some(parent) = document.get_node(slot).and_then(|node| node.parent) else {
            break;
        };
        if path.contains(&parent) {
            return Vec::new();
        }
        slot = parent;
    }
    path
}

pub(crate) fn control_accepts_pointer(node: &Node) -> bool {
    node.primary_styles().is_none_or(|style| {
        style.clone_pointer_events() != PointerEvents::None
            && style.clone_visibility() == Visibility::Visible
    })
}

fn hit_node(
    document: &HtmlDocument,
    node: &Node,
    parent_x: f32,
    parent_y: f32,
    scale: f64,
    visited: &mut BTreeSet<usize>,
) -> Option<usize> {
    if !visited.insert(node.id) {
        return None;
    }
    if format!("{:?}", node.style.display).eq_ignore_ascii_case("none")
        || node
            .primary_styles()
            .is_some_and(|style| style.get_effects().opacity == 0.0)
    {
        return None;
    }
    let mut x = parent_x - node.final_layout.location.x + node.scroll_offset.x as f32;
    let mut y = parent_y - node.final_layout.location.y + node.scroll_offset.y as f32;
    if let Some(transform) = node.transform {
        let point = transform.inverse() * Point::new(f64::from(x) * scale, f64::from(y) * scale);
        x = (point.x / scale) as f32;
        y = (point.y / scale) as f32;
    }
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let box_x = x - node.scroll_offset.x as f32;
    let box_y = y - node.scroll_offset.y as f32;
    let width = node.final_layout.size.width;
    let height = node.final_layout.size.height;
    let inside = box_x >= 0.0 && box_y >= 0.0 && box_x <= width && box_y <= height;
    let clips = format!("{:?}", node.style.overflow.x) != "Visible"
        || format!("{:?}", node.style.overflow.y) != "Visible";
    let children_visible = !clips || (inside && inside_clip_corners(node, box_x, box_y));

    if children_visible {
        if node.flags.is_inline_root() {
            x -= node.final_layout.padding.left + node.final_layout.border.left;
            y -= node.final_layout.padding.top + node.final_layout.border.top;
        }
        if let Some(stacking) = &node.stacking_context {
            for child in stacking.pos_z_hoisted_children().rev() {
                if let Some(hit) = document.get_node(child.node_id).and_then(|node| {
                    hit_node(
                        document,
                        node,
                        x - child.position.x,
                        y - child.position.y,
                        scale,
                        visited,
                    )
                }) {
                    return Some(hit);
                }
            }
        }
        for child in node.paint_children.borrow().iter().flatten().rev() {
            if let Some(hit) = document
                .get_node(*child)
                .and_then(|node| hit_node(document, node, x, y, scale, visited))
            {
                return Some(hit);
            }
        }
        if let Some(stacking) = &node.stacking_context {
            for child in stacking.neg_z_hoisted_children().rev() {
                if let Some(hit) = document.get_node(child.node_id).and_then(|node| {
                    hit_node(
                        document,
                        node,
                        x - child.position.x,
                        y - child.position.y,
                        scale,
                        visited,
                    )
                }) {
                    return Some(hit);
                }
            }
        }
        // Inline glyph topology is owned by Blitz, not approximated as boxes.
        if node.flags.is_inline_root()
            && let Some(hit) = node.hit(parent_x, parent_y, scale)
            && hit.is_text
        {
            return Some(hit.node_id);
        }
    }
    (inside && control_accepts_pointer(node) && inside_clip_corners(node, box_x, box_y))
        .then_some(node.id)
}

fn inside_clip_corners(node: &Node, x: f32, y: f32) -> bool {
    let Some(radii) = crate::adapter::border_radii(node) else {
        return true;
    };
    let width = node.final_layout.size.width;
    let height = node.final_layout.size.height;
    for (radius, right, bottom) in [
        (radii.top_left, false, false),
        (radii.top_right, true, false),
        (radii.bottom_right, true, true),
        (radii.bottom_left, false, true),
    ] {
        let rx = radius[0].min(width / 2.0);
        let ry = radius[1].min(height / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            continue;
        }
        let px = if right { width - x } else { x };
        let py = if bottom { height - y } else { y };
        if px < rx && py < ry && ((px - rx) / rx).powi(2) + ((py - ry) / ry).powi(2) > 1.0 {
            return false;
        }
    }
    true
}
