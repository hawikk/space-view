//! The main view: top bar, status bar, side list and the treemap.

use super::*;

impl DiskTreeApp {
    pub(super) fn browser(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        self.refresh_list();
        self.top_bar(ui, actions);
        self.status_bar(ui, actions);
        self.side_panel(ui, actions);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(6.0))
            .show(ui, |ui| {
                self.treemap(ui, actions);
            });
    }

    pub(super) fn top_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(tree) = &self.tree else { return };
        let busy = self.task.is_some();
        egui::Panel::top("top").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(10, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("disktree").strong().size(17.0).color(Color32::WHITE));
                ui.add_space(6.0);
                if ui.add_enabled(!busy, egui::Button::new("New scan")).on_hover_text("Back to the drive list").clicked() {
                    actions.push(Action::NewScan);
                }
                if ui.add_enabled(!busy, egui::Button::new("Rescan")).on_hover_text("Scan everything again (F5)").clicked() {
                    actions.push(Action::Rescan);
                }
                let can_up = tree.node(self.current).parent != NO_NODE;
                if ui.add_enabled(can_up, egui::Button::new("Up")).on_hover_text("Parent folder (Backspace)").clicked() {
                    actions.push(Action::Up);
                }
                ui.separator();

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("?").on_hover_text("Keyboard shortcuts (F1)").clicked() {
                        actions.push(Action::ShowShortcuts);
                    }
                    ui.selectable_value(&mut self.metric, Metric::Logical, "Logical size")
                        .on_hover_text("The file length Explorer shows as \"Size\"");
                    ui.selectable_value(&mut self.metric, Metric::Allocated, "Size on disk")
                        .on_hover_text("Space actually allocated on disk: what deleting frees (accounts for compression, sparse files and cloud placeholders)");
                    if tree.stats.unreadable > 0 {
                        let t = RichText::new(format!("{} unreadable", format::count(tree.stats.unreadable))).color(WARN);
                        if ui.add(egui::Button::new(t).frame(false)).on_hover_text("Folders that could not be read (click for details)").clicked() {
                            actions.push(Action::ShowUnreadable);
                        }
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        breadcrumbs(ui, tree, self.current, actions);
                    });
                });
            });
        });
    }

    pub(super) fn status_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(tree) = &self.tree else { return };
        let _ = actions;
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(10, 6))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for c in Category::ALL {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                    if c == Category::Other {
                        // Uncategorised types each get their own colour.
                        for (i, key) in [60u16, 25, 1].into_iter().enumerate() {
                            let [r, g, b] = category::rgb(Category::Other, key);
                            let x = rect.min.x + i as f32 * rect.width() / 3.0;
                            let stripe = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(rect.width() / 3.0, rect.height()));
                            ui.painter().rect_filled(stripe, 0.0, Color32::from_rgb(r, g, b));
                        }
                    } else {
                        let [r, g, b] = c.rgb();
                        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                    }
                    ui.add_space(-8.0);
                    ui.label(RichText::new(c.label()).size(12.0).color(TEXT_DIM));
                }
            });
            ui.horizontal(|ui| {
                let root = tree.node(Tree::ROOT);
                let s = &tree.stats;
                let text = format!(
                    "{} on disk ({} logical) · {} files · scanned {} folders in {} · {} links not followed · {}",
                    format::bytes(root.allocated),
                    format::bytes(root.logical),
                    format::count(root.files),
                    format::count(s.dirs),
                    format::duration(s.elapsed_secs),
                    format::count(s.links),
                    self.renderer
                );
                ui.label(RichText::new(text).size(12.0).color(TEXT_DIM));
                if let Some(task) = &self.task {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&task.label).size(12.0).color(WARN));
                        ui.spinner();
                    });
                }
            });
        });
    }

    pub(super) fn side_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(tree) = &self.tree else { return };
        let metric = self.metric;
        let busy = self.task.is_some();
        egui::Panel::right("side")
            .resizable(true)
            .default_size(420.0)
            .size_range(360.0..=900.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(10.0))
            .show(ui, |ui| {
                let cur = tree.node(self.current);
                let cur_size = cur.size(metric);
                let title = if self.current == Tree::ROOT {
                    tree.root_path.display().to_string()
                } else {
                    cur.name.to_string()
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .truncate(),
                );
                ui.label(
                    RichText::new(format!(
                        "{} · {} files · {} items",
                        format::bytes(cur_size),
                        format::count(cur.files),
                        format::count(self.list.len() as u64)
                    ))
                    .color(TEXT_DIM),
                );
                ui.add_space(8.0);

                // Selection card
                egui::Frame::new()
                    .fill(CARD)
                    .corner_radius(0.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        // Fixed height: selecting must not shift the list under the
                        // cursor (the second click of a double-click would hit
                        // another row). Do not wrap the action rows. A wrap grows
                        // this card and moves the list.
                        ui.set_width(ui.available_width());
                        ui.set_min_height(SELECTION_CARD_H);
                        ui.set_max_height(SELECTION_CARD_H);
                        match self.selected {
                            None => {
                                ui.label(
                                    RichText::new(
                                        "Click an item in the map or the list to select it.",
                                    )
                                    .color(TEXT_DIM),
                                );
                                ui.label(
                                    RichText::new("Double-click a folder to open it.")
                                        .color(TEXT_DIM),
                                );
                            }
                            Some(sel) => {
                                let n = tree.node(sel);
                                let path = tree.path_of(sel);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&*n.name)
                                            .strong()
                                            .size(15.0)
                                            .color(Color32::WHITE),
                                    )
                                    .truncate(),
                                );
                                let size = n.size(metric);
                                let size_text = format::bytes(size);
                                let (num, unit) =
                                    size_text.rsplit_once(' ').unwrap_or((&size_text, ""));
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(num)
                                            .size(28.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    ui.label(RichText::new(unit).size(14.0).color(TEXT_DIM));
                                });
                                let path_text = path.display().to_string();
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&path_text).size(11.5).color(TEXT_DIM),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&path_text);
                                let kind = match n.kind {
                                    NodeKind::Dir => {
                                        format!("Folder · {} files", format::count(n.files))
                                    }
                                    NodeKind::File => type_label(n),
                                    NodeKind::Link => "Link (not followed)".to_owned(),
                                };
                                ui.add(
                                    egui::Label::new(format!(
                                        "{} on disk · {} logical · {} of this folder · {kind}",
                                        format::bytes(n.allocated),
                                        format::bytes(n.logical),
                                        format::percent(n.size(metric), cur_size)
                                    ))
                                    .truncate(),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if n.is_dir()
                                        && ui
                                            .button("Open")
                                            .on_hover_text("Zoom into this folder (Enter)")
                                            .clicked()
                                    {
                                        actions.push(Action::Open(sel));
                                    }
                                    if ui
                                        .button("Show in Explorer")
                                        .on_hover_text("Ctrl+E")
                                        .clicked()
                                    {
                                        actions.push(Action::Reveal(sel));
                                    }
                                    if ui.button("Copy path").on_hover_text("Ctrl+C").clicked() {
                                        actions.push(Action::CopyPath(sel));
                                    }
                                });
                                ui.horizontal(|ui| {
                                    let can = !busy && n.kind != NodeKind::Link;
                                    if ui
                                        .add_enabled(can, egui::Button::new("Move to Recycle Bin…"))
                                        .on_hover_text("Delete key. Asks for confirmation first.")
                                        .clicked()
                                    {
                                        actions.push(Action::Delete(sel, DeleteMode::RecycleBin));
                                    }
                                    if ui
                                        .add_enabled(
                                            can,
                                            egui::Button::new(
                                                RichText::new("Delete permanently…")
                                                    .color(Color32::from_rgb(255, 170, 170)),
                                            ),
                                        )
                                        .on_hover_text(
                                            "Shift+Delete. Bypasses the Recycle Bin; asks twice.",
                                        )
                                        .clicked()
                                    {
                                        actions.push(Action::Delete(sel, DeleteMode::Permanent));
                                    }
                                });
                            }
                        }
                    });
                ui.add_space(8.0);

                // Column headers
                ui.horizontal(|ui| {
                    let w = ui.available_width();
                    for (key, label, width) in [
                        (SortKey::Name, "Name", w - 190.0),
                        (SortKey::Size, "Size", 90.0),
                        (SortKey::Files, "Files", 80.0),
                    ] {
                        let active = self.sort == key;
                        let text = RichText::new(label).color(if active {
                            Color32::WHITE
                        } else {
                            TEXT_DIM
                        });
                        let resp = ui
                            .add_sized([width, 20.0], egui::Button::new(text).frame(false))
                            .on_hover_text("Sort (click again to reverse)");
                        if active {
                            sort_arrow(ui.painter(), resp.rect, self.sort_desc);
                        }
                        if resp.clicked() {
                            actions.push(Action::Sort(key));
                        }
                    }
                });
                ui.separator();

                if self.list.is_empty() {
                    ui.add_space(20.0);
                    let msg = if cur.is_unreadable() {
                        "This folder could not be read (access denied)."
                    } else {
                        "This folder is empty."
                    };
                    ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(TEXT_DIM)));
                    return;
                }

                let spacing = ui.spacing().item_spacing.y;
                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
                if let Some(idx) = self.list_scroll_to.take() {
                    let row = ROW_H + spacing;
                    let view_h = ui.available_height();
                    let prev = ui
                        .ctx()
                        .data(|d| d.get_temp::<f32>(Id::new("list-offset")))
                        .unwrap_or(0.0);
                    let top = idx as f32 * row;
                    let offset = if top < prev {
                        top
                    } else if top + row > prev + view_h {
                        top + row - view_h
                    } else {
                        prev
                    };
                    area = area.vertical_scroll_offset(offset.max(0.0));
                }
                let list = &self.list;
                let selected = self.selected;
                let output = area.show_rows(ui, ROW_H, list.len(), |ui, range| {
                    for i in range {
                        let id = list[i];
                        let n = tree.node(id);
                        let resp = list_row(ui, n, metric, cur_size, selected == Some(id));
                        if resp.clicked() {
                            actions.push(Action::Select(id));
                        }
                        if resp.double_clicked() && n.is_dir() && selected == Some(id) {
                            actions.push(Action::Open(id));
                        }
                        if resp.secondary_clicked() {
                            actions.push(Action::Select(id));
                        }
                        resp.context_menu(|ui| context_menu(ui, id, n.kind, busy, actions));
                    }
                });
                ui.ctx()
                    .data_mut(|d| d.insert_temp(Id::new("list-offset"), output.state.offset.y));
            });
    }

    pub(super) fn treemap(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(tree) = &self.tree else { return };
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let key = LayoutKey {
            node: self.current,
            w: rect.width().max(0.0) as u32,
            h: rect.height().max(0.0) as u32,
            metric: self.metric,
            generation: self.generation,
        };
        if self.tiles_key != Some(key) {
            let t0 = Instant::now();
            self.tiles = treemap::layout(
                tree,
                self.current,
                treemap::Rect::new(0.0, 0.0, key.w as f32, key.h as f32),
                self.metric,
                &LayoutOptions::default(),
            );
            self.tiles_key = Some(key);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            if ms > 50.0 {
                diag::log(&format!(
                    "treemap layout: {} tiles in {ms:.1} ms",
                    self.tiles.len()
                ));
            }
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BG);
        let origin = rect.min;
        let to_screen = |r: &treemap::Rect| {
            Rect::from_min_size(
                Pos2::new(origin.x + r.x, origin.y + r.y),
                Vec2::new(r.w, r.h),
            )
        };

        if self.tiles.is_empty() {
            let cur = tree.node(self.current);
            let msg = if cur.is_unreadable() {
                "This folder could not be read (access denied)."
            } else if tree.children(self.current).next().is_none() {
                "This folder is empty."
            } else {
                "Everything in this folder is 0 bytes."
            };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                msg,
                FontId::proportional(16.0),
                TEXT_DIM,
            );
            return;
        }

        // Fills go into one mesh (one draw call for tens of thousands of
        // tiles); labels are drawn on top afterwards. Tiles come parents-first,
        // so children paint over their folder's frame.
        let metric = self.metric;
        let mut mesh = egui::Mesh::default();
        mesh.reserve_vertices(self.tiles.len() * 4);
        mesh.reserve_triangles(self.tiles.len() * 2);
        let mut labels: Vec<(Rect, String, u64, Color32)> = Vec::new();
        let mut headers: Vec<(Rect, String)> = Vec::new();
        for tile in &self.tiles {
            let r = to_screen(&tile.rect);
            if r.width() < 0.5 || r.height() < 0.5 {
                continue;
            }
            // A hairline gap between neighbours instead of a dark border.
            let leaf = if r.width() >= 4.0 && r.height() >= 4.0 {
                r.shrink(0.5)
            } else {
                r
            };
            match tile.kind {
                TileKind::Node(id) => {
                    let n = tree.node(id);
                    if n.is_dir() {
                        let frame = dir_color(tile.depth, n.is_unreadable());
                        let tint = if n.is_unreadable() || n.allocated == 0 {
                            frame
                        } else {
                            node_color(n)
                        };
                        if tile.nested {
                            // Frames carry a hint of what's inside.
                            mesh.add_colored_rect(r, mix(tint, frame, 0.8));
                        } else {
                            // Too small to open up: show the type that fills it.
                            let fill = mix(tint, frame, 0.15);
                            shaded_rect(&mut mesh, leaf, fill);
                            labels.push((r, n.name.to_string(), n.size(metric), fill));
                        }
                        if tile.has_header {
                            let mut label = n.name.to_string();
                            let mut cur = id;
                            for _ in 0..tile.chain {
                                match treemap::only_dir_child(tree, cur, metric) {
                                    Some(c) => {
                                        label.push_str(" › ");
                                        label.push_str(&tree.node(c).name);
                                        cur = c;
                                    }
                                    None => break,
                                }
                            }
                            headers
                                .push((r, format!("{label}  {}", format::bytes(n.size(metric)))));
                        }
                    } else {
                        let fill = node_color(n);
                        shaded_rect(&mut mesh, leaf, fill);
                        labels.push((r, n.name.to_string(), n.size(metric), fill));
                    }
                }
                TileKind::Rest {
                    count,
                    size,
                    largest,
                    ..
                } => {
                    let base = if largest != NO_NODE {
                        node_color(tree.node(largest))
                    } else {
                        Color32::from_rgb(90, 96, 110)
                    };
                    let fill = mix(base, Color32::from_rgb(60, 64, 74), 0.3);
                    shaded_rect(&mut mesh, leaf, fill);
                    if r.width() > 60.0 && r.height() > 16.0 {
                        labels.push((
                            r,
                            format!("{} smaller items", format::count(count as u64)),
                            size,
                            fill,
                        ));
                    }
                }
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        for (r, text) in headers {
            painter.with_clip_rect(r.shrink(1.0)).text(
                Pos2::new(r.min.x + 5.0, r.min.y + 2.0),
                Align2::LEFT_TOP,
                text,
                FontId::proportional(11.5),
                Color32::from_rgb(225, 229, 238),
            );
        }
        for (r, name, size, fill) in labels {
            label_tile(&painter, r, &name, size, fill);
        }

        // Selection and hover outlines.
        if let Some(sel) = self.selected {
            if let Some(t) = self.tiles.iter().find(|t| t.kind == TileKind::Node(sel)) {
                painter.rect_stroke(
                    to_screen(&t.rect),
                    0.0,
                    Stroke::new(2.5, SELECT),
                    StrokeKind::Inside,
                );
            }
        }
        let hovered = response
            .hover_pos()
            .and_then(|p| treemap::hit_test(&self.tiles, p.x - origin.x, p.y - origin.y))
            .map(|i| self.tiles[i]);
        if let Some(t) = hovered {
            painter.rect_stroke(
                to_screen(&t.rect),
                0.0,
                Stroke::new(1.5, Color32::WHITE),
                StrokeKind::Inside,
            );
        }

        if response.secondary_clicked() {
            self.context_node = hovered.and_then(|t| match t.kind {
                TileKind::Node(id) => Some(id),
                TileKind::Rest { .. } => None,
            });
            if let Some(id) = self.context_node {
                actions.push(Action::Select(id));
            }
        }
        if response.double_clicked() {
            match hovered.map(|t| t.kind) {
                Some(TileKind::Node(id)) => actions.push(Action::Open(id)),
                Some(TileKind::Rest { parent, .. }) => actions.push(Action::Open(parent)),
                None => {}
            }
        } else if response.clicked() {
            match hovered.map(|t| t.kind) {
                Some(TileKind::Node(id)) => actions.push(Action::Select(id)),
                _ => self.selected = None,
            }
        }
        if response.clicked_by(PointerButton::Middle) {
            actions.push(Action::Up);
        }

        let busy = self.task.is_some();
        if let Some(id) = self.context_node {
            let kind = tree.node(id).kind;
            response.context_menu(|ui| context_menu(ui, id, kind, busy, actions));
        }

        if let Some(t) = hovered {
            let metric = self.metric;
            let current = self.current;
            response.on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(480.0);
                match t.kind {
                    TileKind::Node(id) => {
                        let n = tree.node(id);
                        let parent = tree.node(n.parent);
                        ui.label(RichText::new(&*n.name).strong().color(Color32::WHITE));
                        ui.label(
                            RichText::new(tree.path_of(id).display().to_string())
                                .size(11.5)
                                .color(TEXT_DIM),
                        );
                        ui.label(format!(
                            "{} on disk · {} logical",
                            format::bytes(n.allocated),
                            format::bytes(n.logical)
                        ));
                        let parent_name = if n.parent == Tree::ROOT {
                            tree.root_path.display().to_string()
                        } else {
                            parent.name.to_string()
                        };
                        ui.label(format!(
                            "{} of {parent_name}",
                            format::percent(n.size(metric), parent.size(metric))
                        ));
                        if n.parent != current {
                            ui.label(format!(
                                "{} of the current folder",
                                format::percent(n.size(metric), tree.node(current).size(metric))
                            ));
                        }
                        match n.kind {
                            NodeKind::Dir => {
                                ui.label(format!("Folder with {} files", format::count(n.files)));
                                ui.label(
                                    RichText::new("Double-click to open · right-click for actions")
                                        .size(11.0)
                                        .color(TEXT_DIM),
                                );
                            }
                            NodeKind::File => {
                                ui.label(RichText::new(type_label(n)).size(11.5).color(TEXT_DIM));
                            }
                            NodeKind::Link => {}
                        }
                    }
                    TileKind::Rest {
                        parent,
                        count,
                        size,
                        ..
                    } => {
                        ui.label(
                            RichText::new(format!("{} smaller items", format::count(count as u64)))
                                .strong(),
                        );
                        ui.label(format!(
                            "{} in {}",
                            format::bytes(size),
                            tree.path_of(parent).display()
                        ));
                        ui.label(
                            RichText::new(
                                "Too small to draw individually; open the folder to see them.",
                            )
                            .size(11.0)
                            .color(TEXT_DIM),
                        );
                    }
                }
            });
        }
    }
}
