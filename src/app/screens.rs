//! Start screen (drives, folder picker) and scan progress screen.

use super::*;

impl DiskTreeApp {
    pub(super) fn start_screen(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut scan_target: Option<PathBuf> = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(24.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let column = 760.0f32.min(ui.available_width());
                let margin = ((ui.available_width() - column) / 2.0).max(0.0);
                ui.horizontal_top(|ui| {
                ui.add_space(margin);
                ui.vertical(|ui| {
                    ui.set_width(column);
                    ui.add_space(28.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("disktree").size(40.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("See what fills your disk, then remove it safely.").size(16.0).color(TEXT_DIM));
                    });
                    ui.add_space(26.0);

                    if let Some(err) = &self.start_error {
                        egui::Frame::new().fill(Color32::from_rgb(70, 30, 30)).corner_radius(0.0).inner_margin(10.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(err).color(Color32::from_rgb(255, 200, 200)));
                        });
                        ui.add_space(16.0);
                    }

                    section_title(ui, "Scan a drive");
                    match &self.drives {
                        Drives::Loading(_) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Looking for drives…").color(TEXT_DIM));
                            });
                        }
                        Drives::Ready(drives) if drives.is_empty() => {
                            ui.label(RichText::new("No fixed drives found. Choose a folder below.").color(TEXT_DIM));
                        }
                        Drives::Ready(drives) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                                for d in drives {
                                    if drive_card(ui, d) {
                                        scan_target = Some(d.root.clone());
                                    }
                                }
                            });
                        }
                    }

                    ui.add_space(26.0);
                    section_title(ui, "Or scan a folder");
                    ui.horizontal(|ui| {
                        let browse_w = if cfg!(windows) { 110.0 } else { 0.0 };
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.path_input)
                                .hint_text(if cfg!(windows) { r"C:\Users\you\Downloads" } else { "/home/you" })
                                .desired_width(ui.available_width() - 90.0 - browse_w)
                                .font(FontId::proportional(15.0)),
                        );
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let go = ui.add_enabled(!self.path_input.trim().is_empty(), egui::Button::new("Scan")).clicked();
                        if (go || enter) && !self.path_input.trim().is_empty() {
                            scan_target = Some(PathBuf::from(self.path_input.trim().trim_matches('"')));
                        }
                        #[cfg(windows)]
                        if ui.button("Browse…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().set_title("Choose a folder to scan").pick_folder() {
                                self.path_input = p.display().to_string();
                                scan_target = Some(p);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("You can also drop a folder onto this window.").color(TEXT_DIM));

                    ui.add_space(30.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Junctions, symlinks and mount points are shown but never followed, so nothing is counted twice. \
                             Some system folders can only be read when disktree runs as administrator; anything unreadable is \
                             listed after the scan.",
                        )
                        .color(TEXT_DIM)
                        .size(12.5),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("v{} · renderer: {}", env!("CARGO_PKG_VERSION"), self.renderer)).color(TEXT_DIM).size(11.0));
                });
                });
            });
        });
        if let Some(t) = scan_target {
            self.start_scan(&ctx, t, None);
        }
    }

    pub(super) fn scanning_screen(&mut self, ui: &mut Ui) {
        let Some(job) = &self.scan else { return };
        let mut cancel = ui.input(|i| i.key_pressed(Key::Escape));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(24.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(640.0);
                    ui.add_space((ui.available_height() * 0.18).max(20.0));
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(22.0));
                        ui.label(
                            RichText::new(format!("Scanning {}", job.root.display()))
                                .size(22.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });
                    ui.add_space(22.0);
                    let p = &job.progress;
                    let files = p.files.load(std::sync::atomic::Ordering::Relaxed);
                    let dirs = p.dirs.load(std::sync::atomic::Ordering::Relaxed);
                    let bytes = p.allocated.load(std::sync::atomic::Ordering::Relaxed);
                    let bad = p.unreadable.load(std::sync::atomic::Ordering::Relaxed);
                    let secs = job.started.elapsed().as_secs_f64();
                    egui::Grid::new("scan-stats")
                        .num_columns(2)
                        .spacing([40.0, 10.0])
                        .show(ui, |ui| {
                            stat(ui, "Files", &format::count(files));
                            stat(ui, "Folders", &format::count(dirs));
                            ui.end_row();
                            stat(ui, "Found so far", &format::bytes(bytes));
                            stat(ui, "Elapsed", &format::duration(secs));
                            ui.end_row();
                            stat(
                                ui,
                                "Speed",
                                &format!(
                                    "{} files/s",
                                    format::count((files as f64 / secs.max(0.001)) as u64)
                                ),
                            );
                            stat(ui, "Unreadable", &format::count(bad));
                            ui.end_row();
                        });
                    ui.add_space(18.0);
                    let cur = p.current();
                    ui.add(
                        egui::Label::new(RichText::new(cur).color(TEXT_DIM).size(12.0)).truncate(),
                    );
                    ui.add_space(22.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Cancel").size(15.0))
                                .min_size(Vec2::new(120.0, 32.0)),
                        )
                        .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if cancel {
            job.progress.cancel();
        }
    }
}
