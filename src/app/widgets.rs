//! Small widgets and drawing helpers.

use super::*;

pub(super) fn section_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(15.0)
            .strong()
            .color(Color32::WHITE),
    );
    ui.add_space(4.0);
}

pub(super) fn stat(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.set_min_width(190.0);
        ui.add(
            egui::Label::new(RichText::new(label).color(TEXT_DIM).size(12.0))
                .wrap_mode(egui::TextWrapMode::Extend),
        );
        ui.add(
            egui::Label::new(
                RichText::new(value)
                    .size(20.0)
                    .strong()
                    .color(Color32::WHITE),
            )
            .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

pub(super) fn warning_box(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(80, 28, 28))
        .corner_radius(0.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(Color32::from_rgb(255, 205, 205)));
        });
}

pub(super) fn drive_card(ui: &mut Ui, d: &Drive) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(232.0, 96.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, if resp.hovered() { CARD_HOVER } else { CARD });
    if resp.hovered() {
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
    }
    let root = d.root.display().to_string();
    let title = if d.label.is_empty() {
        root.clone()
    } else {
        format!("{root}  {}", d.label)
    };
    p.with_clip_rect(rect.shrink(8.0)).text(
        rect.min + Vec2::new(14.0, 12.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(17.0),
        Color32::WHITE,
    );
    if d.total > 0 {
        let used = d.total.saturating_sub(d.free);
        let frac = used as f32 / d.total as f32;
        let bar = Rect::from_min_size(
            rect.min + Vec2::new(14.0, 44.0),
            Vec2::new(rect.width() - 28.0, 8.0),
        );
        p.rect_filled(bar, 4.0, Color32::from_rgb(55, 60, 74));
        let fill = Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, bar.height()));
        p.rect_filled(
            fill,
            4.0,
            if frac > 0.9 {
                Color32::from_rgb(229, 83, 83)
            } else {
                ACCENT
            },
        );
        let text = format!(
            "{} free of {}{}",
            format::bytes(d.free),
            format::bytes(d.total),
            if d.filesystem.is_empty() {
                String::new()
            } else {
                format!(" · {}", d.filesystem)
            }
        );
        p.text(
            rect.min + Vec2::new(14.0, 62.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(12.5),
            TEXT_DIM,
        );
    } else {
        p.text(
            rect.min + Vec2::new(14.0, 50.0),
            Align2::LEFT_TOP,
            "Click to scan",
            FontId::proportional(12.5),
            TEXT_DIM,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

pub(super) fn breadcrumbs(ui: &mut Ui, tree: &Tree, current: NodeId, actions: &mut Vec<Action>) {
    let mut chain = tree.ancestors(current);
    chain.reverse();
    let skip = chain.len().saturating_sub(6);
    if skip > 0 {
        ui.label(RichText::new("…").color(TEXT_DIM));
    }
    for (i, &n) in chain.iter().enumerate().skip(skip) {
        if i > skip || skip > 0 {
            ui.label(RichText::new("›").color(TEXT_DIM));
        }
        let name = if n == Tree::ROOT {
            tree.root_path.display().to_string()
        } else {
            tree.node(n).name.to_string()
        };
        let is_current = n == current;
        let text = if is_current {
            RichText::new(name).strong().color(Color32::WHITE)
        } else {
            RichText::new(name).color(ACCENT)
        };
        if ui.add(egui::Button::new(text).frame(false)).clicked() && !is_current {
            actions.push(Action::GoTo(n));
        }
    }
}

pub(super) fn list_row(
    ui: &mut Ui,
    n: &disktree::tree::Node,
    metric: Metric,
    parent_size: u64,
    selected: bool,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 0.0, Color32::from_rgb(48, 42, 28));
        p.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            0.0,
            SELECT,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, CARD);
    }
    let size = n.size(metric);
    let frac = if parent_size > 0 {
        size as f32 / parent_size as f32
    } else {
        0.0
    };
    // Share-of-folder bar behind the name.
    let bar_w = (rect.width() - 190.0).max(0.0) * frac.clamp(0.0, 1.0);
    if bar_w > 0.5 {
        p.rect_filled(
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, ROW_H - 3.0),
                Vec2::new(bar_w, 2.0),
            ),
            0.0,
            Color32::from_rgb(70, 110, 170),
        );
    }
    let icon = Rect::from_min_size(rect.min + Vec2::new(4.0, 6.0), Vec2::new(11.0, 10.0));
    match n.kind {
        NodeKind::Dir => {
            p.rect_filled(
                Rect::from_min_size(icon.min, Vec2::new(5.0, 3.0)),
                1.0,
                Color32::from_rgb(220, 180, 90),
            );
            p.rect_filled(
                Rect::from_min_max(icon.min + Vec2::new(0.0, 2.0), icon.max),
                1.5,
                Color32::from_rgb(220, 180, 90),
            );
        }
        NodeKind::File => {
            p.rect_filled(icon.shrink(1.0), 2.0, node_color(n));
        }
        NodeKind::Link => {
            p.rect_stroke(
                icon.shrink(1.0),
                2.0,
                Stroke::new(1.0, TEXT_DIM),
                StrokeKind::Inside,
            );
        }
    }
    let name_rect = Rect::from_min_max(
        rect.min + Vec2::new(22.0, 0.0),
        Pos2::new(rect.max.x - 178.0, rect.max.y),
    );
    let mut name = n.name.to_string();
    if n.kind == NodeKind::Link {
        name.push_str("  (link, not followed)");
    } else if n.is_unreadable() {
        name.push_str("  (access denied)");
    }
    let color = if n.kind == NodeKind::Link || n.is_unreadable() {
        TEXT_DIM
    } else {
        Color32::from_rgb(225, 228, 235)
    };
    p.with_clip_rect(name_rect).text(
        Pos2::new(name_rect.min.x, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.5),
        color,
    );
    p.text(
        Pos2::new(rect.max.x - 92.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format::bytes(size),
        FontId::proportional(13.0),
        Color32::WHITE,
    );
    p.text(
        Pos2::new(rect.max.x - 50.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format::percent(size, parent_size),
        FontId::proportional(12.0),
        TEXT_DIM,
    );
    let files = if n.is_dir() {
        format::count(n.files)
    } else {
        String::new()
    };
    p.text(
        Pos2::new(rect.max.x - 2.0, rect.center().y),
        Align2::RIGHT_CENTER,
        files,
        FontId::proportional(12.0),
        TEXT_DIM,
    );
    resp
}

pub(super) fn context_menu(
    ui: &mut Ui,
    id: NodeId,
    kind: NodeKind,
    busy: bool,
    actions: &mut Vec<Action>,
) {
    if kind == NodeKind::Dir && ui.button("Open").clicked() {
        actions.push(Action::Open(id));
        ui.close();
    }
    if ui.button("Show in Explorer").clicked() {
        actions.push(Action::Reveal(id));
        ui.close();
    }
    if ui.button("Copy path").clicked() {
        actions.push(Action::CopyPath(id));
        ui.close();
    }
    if kind == NodeKind::Dir
        && ui
            .add_enabled(!busy, egui::Button::new("Refresh this folder"))
            .clicked()
    {
        actions.push(Action::RefreshFolder(id));
        ui.close();
    }
    ui.separator();
    let can = !busy && kind != NodeKind::Link;
    if ui
        .add_enabled(can, egui::Button::new("Move to Recycle Bin…"))
        .clicked()
    {
        actions.push(Action::Delete(id, DeleteMode::RecycleBin));
        ui.close();
    }
    if ui
        .add_enabled(can, egui::Button::new("Delete permanently…"))
        .clicked()
    {
        actions.push(Action::Delete(id, DeleteMode::Permanent));
        ui.close();
    }
}

pub(super) fn sort_arrow(painter: &egui::Painter, r: Rect, desc: bool) {
    let c = Pos2::new(r.max.x - 8.0, r.center().y);
    let (a, b) = (4.0, 3.0);
    let pts = if desc {
        vec![
            Pos2::new(c.x - a, c.y - b),
            Pos2::new(c.x + a, c.y - b),
            Pos2::new(c.x, c.y + b),
        ]
    } else {
        vec![
            Pos2::new(c.x - a, c.y + b),
            Pos2::new(c.x + a, c.y + b),
            Pos2::new(c.x, c.y - b),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, ACCENT, Stroke::NONE));
}

pub(super) fn label_tile(painter: &egui::Painter, r: Rect, name: &str, size: u64, fill: Color32) {
    if r.width() < 44.0 || r.height() < 15.0 {
        return;
    }
    let text_color = if luminance(fill) > 0.55 {
        Color32::from_rgb(20, 20, 24)
    } else {
        Color32::WHITE
    };
    let clip = painter.with_clip_rect(r.shrink(2.0));
    if r.height() >= 32.0 {
        clip.text(
            Pos2::new(r.min.x + 5.0, r.min.y + 4.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(12.0),
            text_color,
        );
        clip.text(
            Pos2::new(r.min.x + 5.0, r.min.y + 18.0),
            Align2::LEFT_TOP,
            format::bytes(size),
            FontId::proportional(11.0),
            text_color.gamma_multiply(0.8),
        );
    } else {
        clip.text(
            Pos2::new(r.min.x + 4.0, r.center().y),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(11.0),
            text_color,
        );
    }
}

pub(super) fn dir_color(depth: u16, unreadable: bool) -> Color32 {
    if unreadable {
        return Color32::from_rgb(78, 44, 44);
    }
    let d = depth.min(8) as u8;
    Color32::from_rgb(34 + d * 7, 39 + d * 7, 50 + d * 7)
}

pub(super) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub(super) fn shade(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

pub(super) fn luminance(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

/// Colour of a file, or of a folder from the type that fills it.
pub(super) fn node_color(n: &disktree::tree::Node) -> Color32 {
    let [r, g, b] = category::rgb(n.category, n.ext);
    Color32::from_rgb(r, g, b)
}

/// A filled rectangle lit from the top left, like a cushion treemap.
pub(super) fn shaded_rect(mesh: &mut egui::Mesh, r: Rect, c: Color32) {
    if r.width() < 3.0 || r.height() < 3.0 {
        mesh.add_colored_rect(r, c);
        return;
    }
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(r.left_top(), mix(c, Color32::WHITE, 0.22));
    mesh.colored_vertex(r.right_top(), mix(c, Color32::WHITE, 0.06));
    mesh.colored_vertex(r.right_bottom(), shade(c, 0.68));
    mesh.colored_vertex(r.left_bottom(), shade(c, 0.86));
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

/// A type's short description: its category, or the extension for "Other".
pub(super) fn type_label(n: &disktree::tree::Node) -> String {
    if n.category == Category::Other {
        match n.name.rfind('.') {
            Some(i) if i > 0 && i + 1 < n.name.len() => format!(".{} file", &n.name[i + 1..]),
            _ => "File".to_owned(),
        }
    } else {
        n.category.label().to_owned()
    }
}
