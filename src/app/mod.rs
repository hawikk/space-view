//! The egui application: state, background work (scan/delete/refresh),
//! actions and keyboard handling. Drawing lives in the submodules.

mod browser;
mod dialogs;
mod screens;
mod widgets;

use widgets::*;

use crate::diag;
use disktree::category::{self, Category};
use disktree::format;
use disktree::ops::{self, DeleteMode, DeleteOutcome, Drive};
use disktree::safety::{self, Protection, SystemPaths};
use disktree::scan::{self, Progress, ScanError};
use disktree::tree::{Metric, NodeId, NodeKind, ScannedDir, Tree, NO_NODE};
use disktree::treemap::{self, LayoutOptions, Tile, TileKind};
use egui::{
    Align, Align2, Color32, Event, FontId, Id, Key, Layout, Modifiers, PointerButton, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(17, 19, 25);
const PANEL: Color32 = Color32::from_rgb(25, 28, 36);
const CARD: Color32 = Color32::from_rgb(33, 37, 47);
const CARD_HOVER: Color32 = Color32::from_rgb(42, 47, 60);
const TEXT_DIM: Color32 = Color32::from_rgb(150, 156, 170);
const ACCENT: Color32 = Color32::from_rgb(98, 160, 255);
const DANGER: Color32 = Color32::from_rgb(170, 48, 48);
const WARN: Color32 = Color32::from_rgb(240, 180, 70);
const SELECT: Color32 = Color32::from_rgb(255, 214, 90);
const ROW_H: f32 = 22.0;
const SELECTION_CARD_H: f32 = 200.0;

enum Drives {
    Loading(Receiver<Vec<Drive>>),
    Ready(Vec<Drive>),
}

struct ScanJob {
    root: PathBuf,
    progress: Arc<Progress>,
    rx: Receiver<Result<Tree, ScanError>>,
    started: Instant,
    restore: Option<PathBuf>,
}

enum TaskKind {
    Delete {
        node: NodeId,
        name: String,
        size: u64,
        mode: DeleteMode,
    },
    Refresh {
        node: NodeId,
    },
}

enum TaskResult {
    Delete(DeleteOutcome),
    Refresh(Result<ScannedDir, String>),
}

struct Task {
    kind: TaskKind,
    rx: Receiver<TaskResult>,
    label: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortKey {
    Size,
    Name,
    Files,
}

struct ConfirmDelete {
    node: NodeId,
    path: PathBuf,
    name: String,
    allocated: u64,
    logical: u64,
    files: u64,
    is_dir: bool,
    mode: DeleteMode,
    caution: Option<String>,
    typed: String,
    /// Permanent deletes need two confirmations: step 0, then step 1.
    step: u8,
    /// The final button only works after a short pause, so a double-click on
    /// "Continue…" cannot also hit "Delete permanently" in the same spot.
    armed_at: Instant,
}

enum Dialog {
    Confirm(ConfirmDelete),
    Message { title: String, body: String },
    Unreadable,
    Shortcuts,
}

struct Toast {
    text: String,
    until: Instant,
    error: bool,
}

enum Action {
    Select(NodeId),
    Open(NodeId),
    Up,
    GoTo(NodeId),
    Reveal(NodeId),
    CopyPath(NodeId),
    Delete(NodeId, DeleteMode),
    RefreshFolder(NodeId),
    Rescan,
    NewScan,
    Sort(SortKey),
    ShowUnreadable,
    ShowShortcuts,
}

#[derive(Clone, Copy, PartialEq)]
struct LayoutKey {
    node: NodeId,
    w: u32,
    h: u32,
    metric: Metric,
    generation: u64,
}

pub struct DiskTreeApp {
    renderer: &'static str,
    sys: SystemPaths,
    drives: Drives,
    path_input: String,
    start_error: Option<String>,
    scan: Option<ScanJob>,
    tree: Option<Tree>,
    generation: u64,
    current: NodeId,
    selected: Option<NodeId>,
    context_node: Option<NodeId>,
    metric: Metric,
    sort: SortKey,
    sort_desc: bool,
    list: Vec<NodeId>,
    list_key: Option<(NodeId, Metric, SortKey, bool, u64)>,
    list_scroll_to: Option<usize>,
    tiles: Vec<Tile>,
    tiles_key: Option<LayoutKey>,
    dialog: Option<Dialog>,
    task: Option<Task>,
    toast: Option<Toast>,
    owner_hwnd: isize,
}

impl DiskTreeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        renderer: &'static str,
        path: Option<PathBuf>,
    ) -> Self {
        setup_style(&cc.egui_ctx);
        load_system_fonts_in_background(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("disktree-worker-drives".into())
            .spawn(move || {
                let drives = ops::fixed_drives();
                let _ = tx.send(drives);
                ctx.request_repaint();
            });
        let drives = if spawned.is_ok() {
            Drives::Loading(rx)
        } else {
            Drives::Ready(Vec::new())
        };
        let mut app = DiskTreeApp {
            renderer,
            sys: SystemPaths::from_env(),
            drives,
            path_input: String::new(),
            start_error: None,
            scan: None,
            tree: None,
            generation: 0,
            current: Tree::ROOT,
            selected: None,
            context_node: None,
            metric: Metric::Allocated,
            sort: SortKey::Size,
            sort_desc: true,
            list: Vec::new(),
            list_key: None,
            list_scroll_to: None,
            tiles: Vec::new(),
            tiles_key: None,
            dialog: None,
            task: None,
            toast: None,
            owner_hwnd: 0,
        };
        if let Some(p) = path {
            app.path_input = p.display().to_string();
            app.start_scan(&cc.egui_ctx, p, None);
        }
        app
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + Duration::from_secs(6),
            error,
        });
    }

    // ----- scanning -------------------------------------------------------

    fn start_scan(&mut self, ctx: &egui::Context, root: PathBuf, restore: Option<PathBuf>) {
        if self.task.is_some() {
            self.toast("Wait for the current delete to finish first.", true);
            return;
        }
        let root = scan::normalize_root(&root);
        diag::log(&format!("scan started: {}", root.display()));
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let (p, r, c) = (progress.clone(), root.clone(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("disktree-worker-scan".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scan::scan(&r, &p, scan::default_threads())
                }))
                .unwrap_or_else(|panic| Err(ScanError::Internal(scan::panic_message(&panic))));
                let _ = tx.send(result);
                c.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.start_error = None;
                self.dialog = None;
                self.scan = Some(ScanJob {
                    root,
                    progress,
                    rx,
                    started: Instant::now(),
                    restore,
                });
            }
            Err(e) => self.start_error = Some(format!("Could not start the scan: {e}")),
        }
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.scan else { return };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err(ScanError::Internal("the scan thread stopped".into()))
            }
        };
        let job = self.scan.take().expect("job present");
        match result {
            Ok(tree) => {
                diag::log(&format!(
                    "scan finished: {} nodes, {} files, {} unreadable, {:.2}s",
                    tree.len(),
                    tree.stats.files,
                    tree.stats.unreadable,
                    tree.stats.elapsed_secs
                ));
                self.set_tree(ctx, tree, job.restore);
            }
            Err(ScanError::Cancelled) => diag::log("scan cancelled"),
            Err(e) => {
                diag::log(&format!("scan failed: {e}"));
                if self.tree.is_some() {
                    self.toast(e.to_string(), true);
                } else {
                    self.start_error = Some(e.to_string());
                }
            }
        }
    }

    fn set_tree(&mut self, ctx: &egui::Context, tree: Tree, restore: Option<PathBuf>) {
        self.current = restore.and_then(|p| tree.find(&p)).unwrap_or(Tree::ROOT);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "disktree — {}",
            tree.root_path.display()
        )));
        if let Some(old) = self.tree.replace(tree) {
            // Freeing millions of nodes takes a moment; don't stall the UI.
            let _ = std::thread::Builder::new()
                .name("disktree-worker-free".into())
                .spawn(move || drop(old));
        }
        self.selected = None;
        self.generation += 1;
        self.list_scroll_to = Some(0);
    }

    // ----- background tasks (delete / refresh) ----------------------------

    fn spawn_task(
        &mut self,
        ctx: &egui::Context,
        kind: TaskKind,
        label: String,
        job: impl FnOnce() -> TaskResult + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let c = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("disktree-worker-task".into())
            .spawn(move || {
                let r = job();
                let _ = tx.send(r);
                c.request_repaint();
            });
        match spawned {
            Ok(_) => self.task = Some(Task { kind, rx, label }),
            Err(e) => self.toast(format!("Could not start: {e}"), true),
        }
    }

    fn start_delete(&mut self, ctx: &egui::Context, c: ConfirmDelete) {
        let size = if self.metric == Metric::Allocated {
            c.allocated
        } else {
            c.logical
        };
        let verb = match c.mode {
            DeleteMode::RecycleBin => "Moving to Recycle Bin",
            DeleteMode::Permanent => "Deleting",
        };
        let label = format!("{verb}: {}", c.name);
        diag::log(&format!(
            "delete requested ({:?}): {}",
            c.mode,
            c.path.display()
        ));
        let (path, mode, owner) = (c.path.clone(), c.mode, self.owner_hwnd);
        self.spawn_task(
            ctx,
            TaskKind::Delete {
                node: c.node,
                name: c.name,
                size,
                mode,
            },
            label,
            move || TaskResult::Delete(ops::delete(&path, mode, owner)),
        );
    }

    fn start_refresh(&mut self, ctx: &egui::Context, node: NodeId) {
        let Some(tree) = &self.tree else { return };
        if tree.node(node).kind != NodeKind::Dir {
            return;
        }
        let path = tree.path_of(node);
        let label = format!("Refreshing {}", path.display());
        self.spawn_task(ctx, TaskKind::Refresh { node }, label, move || {
            let p = Progress::default();
            TaskResult::Refresh(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scan::scan_subtree(&path, &p, scan::default_threads())
                }))
                .map_err(|panic| scan::panic_message(&panic))
                .and_then(|r| r.map_err(|e| e.to_string())),
            )
        });
    }

    fn poll_task(&mut self, ctx: &egui::Context) {
        let Some(task) = &self.task else { return };
        let result = match task.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => match task.kind {
                TaskKind::Delete { .. } => TaskResult::Delete(DeleteOutcome::Failed(
                    "The delete worker stopped unexpectedly.".into(),
                )),
                TaskKind::Refresh { .. } => {
                    TaskResult::Refresh(Err("The refresh worker stopped unexpectedly.".into()))
                }
            },
        };
        let task = self.task.take().expect("task present");
        match (task.kind, result) {
            (
                TaskKind::Delete {
                    node,
                    name,
                    size,
                    mode,
                },
                TaskResult::Delete(outcome),
            ) => {
                diag::log(&format!("delete finished: {outcome:?}"));
                match outcome {
                    DeleteOutcome::Deleted | DeleteOutcome::AlreadyGone => {
                        if let Some(tree) = &mut self.tree {
                            tree.remove(node);
                            if let Some(sel) = self.selected {
                                if tree.is_ancestor_or_self(node, sel) {
                                    self.selected = None;
                                }
                            }
                            if tree.is_ancestor_or_self(node, self.current) {
                                self.current = tree.node(node).parent;
                            }
                        }
                        self.generation += 1;
                        let msg = match (outcome, mode) {
                            (DeleteOutcome::AlreadyGone, _) => {
                                format!("{name} was already gone; removed it from the map.")
                            }
                            (_, DeleteMode::RecycleBin) => format!(
                                "Moved {name} to the Recycle Bin ({}).",
                                format::bytes(size)
                            ),
                            (_, DeleteMode::Permanent) => format!(
                                "Permanently deleted {name} ({} freed).",
                                format::bytes(size)
                            ),
                        };
                        self.toast(msg, false);
                    }
                    DeleteOutcome::Cancelled => {
                        self.toast(
                            "Cancelled. Anything already removed is reflected after a refresh.",
                            false,
                        );
                        self.start_refresh(ctx, node);
                    }
                    DeleteOutcome::Partial(why) => {
                        self.dialog = Some(Dialog::Message { title: "Partly deleted".into(), body: format!("{name}: {why}\n\nThe folder is being re-read so the sizes are accurate.") });
                        self.start_refresh(ctx, node);
                    }
                    DeleteOutcome::Failed(why) => {
                        self.dialog = Some(Dialog::Message {
                            title: "Could not delete".into(),
                            body: format!("{name}\n\n{why}"),
                        });
                    }
                }
            }
            (TaskKind::Refresh { node }, TaskResult::Refresh(res)) => match res {
                Ok(fresh) => {
                    if let Some(tree) = &mut self.tree {
                        tree.graft(node, fresh);
                        if let Some(sel) = self.selected {
                            if sel != node && tree.is_ancestor_or_self(node, sel) {
                                self.selected = None;
                            }
                        }
                        if self.current != node && tree.is_ancestor_or_self(node, self.current) {
                            self.current = node;
                        }
                    }
                    self.generation += 1;
                }
                Err(e) => {
                    // The folder itself may have been deleted meanwhile.
                    let gone = self
                        .tree
                        .as_ref()
                        .map(|t| !t.path_of(node).exists())
                        .unwrap_or(false);
                    if gone {
                        if let Some(tree) = &mut self.tree {
                            tree.remove(node);
                            if tree.is_ancestor_or_self(node, self.current) {
                                self.current = tree.node(node).parent;
                            }
                        }
                        self.selected = None;
                        self.generation += 1;
                    } else {
                        self.toast(format!("Refresh failed: {e}"), true);
                    }
                }
            },
            _ => {}
        }
    }

    // ----- actions --------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        let Some(tree) = &self.tree else {
            if let Action::NewScan = action {
                self.scan = None;
            }
            return;
        };
        match action {
            Action::Select(n) => self.selected = Some(n),
            Action::Open(n) => {
                let node = tree.node(n);
                if node.is_dir() {
                    self.current = n;
                    self.selected = None;
                    self.list_scroll_to = Some(0);
                } else if node.parent != NO_NODE && node.parent != self.current {
                    self.current = node.parent;
                    self.selected = Some(n);
                }
            }
            Action::Up => {
                let parent = tree.node(self.current).parent;
                if parent != NO_NODE {
                    self.selected = Some(self.current);
                    self.current = parent;
                    self.list_scroll_to = self.selected.and_then(|s| self.list_position(s));
                }
            }
            Action::GoTo(n) => {
                if n != self.current {
                    self.selected = None;
                    self.current = n;
                    self.list_scroll_to = Some(0);
                }
            }
            Action::Reveal(n) => ops::reveal_in_explorer(&tree.path_of(n)),
            Action::CopyPath(n) => {
                let p = tree.path_of(n).display().to_string();
                ctx.copy_text(p.clone());
                self.toast(format!("Copied {p}"), false);
            }
            Action::Delete(n, mode) => self.request_delete(n, mode),
            Action::RefreshFolder(n) => {
                if self.task.is_some() {
                    self.toast("Wait for the current operation to finish first.", true);
                } else {
                    self.start_refresh(ctx, n);
                }
            }
            Action::Rescan => {
                let root = tree.root_path.clone();
                let restore = Some(tree.path_of(self.current));
                self.start_scan(ctx, root, restore);
            }
            Action::NewScan => {
                if self.task.is_some() {
                    self.toast("Wait for the current operation to finish first.", true);
                    return;
                }
                if let Some(old) = self.tree.take() {
                    let _ = std::thread::Builder::new()
                        .name("disktree-worker-free".into())
                        .spawn(move || drop(old));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Title("disktree".into()));
                self.selected = None;
                self.current = Tree::ROOT;
                self.generation += 1;
            }
            Action::Sort(k) => {
                if self.sort == k {
                    self.sort_desc = !self.sort_desc;
                } else {
                    self.sort = k;
                    self.sort_desc = k != SortKey::Name;
                }
            }
            Action::ShowUnreadable => self.dialog = Some(Dialog::Unreadable),
            Action::ShowShortcuts => self.dialog = Some(Dialog::Shortcuts),
        }
    }

    fn request_delete(&mut self, node: NodeId, mode: DeleteMode) {
        let Some(tree) = &self.tree else { return };
        if self.task.is_some() {
            self.toast("Wait for the current operation to finish first.", true);
            return;
        }
        if node == Tree::ROOT {
            self.dialog = Some(Dialog::Message {
                title: "disktree won't delete this".into(),
                body: "This is the folder you scanned. Go up a level (or scan its parent) to delete it.".into(),
            });
            return;
        }
        let path = tree.path_of(node);
        let n = tree.node(node);
        let caution = match safety::classify(&path.to_string_lossy(), n.kind, &self.sys) {
            Protection::Blocked(reason) => {
                self.dialog = Some(Dialog::Message {
                    title: "disktree won't delete this".into(),
                    body: format!("{}\n\n{reason}", path.display()),
                });
                return;
            }
            Protection::Caution(reason) => Some(reason),
            Protection::Normal => None,
        };
        self.dialog = Some(Dialog::Confirm(ConfirmDelete {
            node,
            name: n.name.to_string(),
            allocated: n.allocated,
            logical: n.logical,
            files: n.files,
            is_dir: n.is_dir(),
            path,
            mode,
            caution,
            typed: String::new(),
            step: 0,
            armed_at: Instant::now(),
        }));
    }

    fn list_position(&self, node: NodeId) -> Option<usize> {
        self.list.iter().position(|&n| n == node)
    }

    fn refresh_list(&mut self) {
        let Some(tree) = &self.tree else { return };
        let key = (
            self.current,
            self.metric,
            self.sort,
            self.sort_desc,
            self.generation,
        );
        if self.list_key == Some(key) {
            return;
        }
        let mut v: Vec<NodeId> = tree.children(self.current).collect();
        let metric = self.metric;
        match self.sort {
            SortKey::Size => v.sort_by(|&a, &b| {
                tree.node(b)
                    .size(metric)
                    .cmp(&tree.node(a).size(metric))
                    .then(a.cmp(&b))
            }),
            SortKey::Files => {
                v.sort_by(|&a, &b| tree.node(b).files.cmp(&tree.node(a).files).then(a.cmp(&b)))
            }
            SortKey::Name => {
                v.sort_by_cached_key(|&a| tree.node(a).name.to_lowercase());
                v.reverse();
            }
        }
        if !self.sort_desc {
            v.reverse();
        }
        self.list = v;
        self.list_key = Some(key);
    }

    // ----- keyboard -------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.dialog.is_some() || ctx.egui_wants_keyboard_input() || self.tree.is_none() {
            return;
        }
        let mut delete: Option<bool> = None;
        let (
            mut up,
            mut enter,
            mut home,
            mut f5,
            mut copy,
            mut reveal,
            mut prev,
            mut next,
            mut esc,
            mut help,
        ) = (
            false, false, false, false, false, false, false, false, false, false,
        );
        ctx.input_mut(|i| {
            // Take the modifiers from the Delete key event itself, so the mode
            // is decided by the key press, not by Shift's state later on.
            // On Windows egui-winit reports Shift+Delete as `Event::Cut` (the
            // legacy cut shortcut) and swallows the key event; Ctrl+X is also
            // `Cut` but has Ctrl held.
            let shift_only = i.modifiers.shift && !i.modifiers.command;
            i.events.retain(|e| match e {
                Event::Key {
                    key: Key::Delete,
                    pressed: true,
                    modifiers,
                    ..
                } if delete.is_none() => {
                    delete = Some(modifiers.shift);
                    false
                }
                Event::Cut if delete.is_none() && shift_only => {
                    delete = Some(true);
                    false
                }
                _ => true,
            });
            copy = i.events.iter().any(|e| matches!(e, Event::Copy))
                || i.consume_key(Modifiers::COMMAND, Key::C);
            reveal = i.consume_key(Modifiers::COMMAND, Key::E);
            up = i.consume_key(Modifiers::ALT, Key::ArrowUp)
                || i.consume_key(Modifiers::NONE, Key::Backspace)
                || i.pointer.button_pressed(PointerButton::Extra1);
            enter = i.consume_key(Modifiers::NONE, Key::Enter);
            home = i.consume_key(Modifiers::NONE, Key::Home);
            f5 = i.consume_key(Modifiers::NONE, Key::F5);
            prev = i.consume_key(Modifiers::NONE, Key::ArrowUp);
            next = i.consume_key(Modifiers::NONE, Key::ArrowDown);
            esc = i.consume_key(Modifiers::NONE, Key::Escape);
            help = i.consume_key(Modifiers::NONE, Key::F1);
        });
        if let Some(permanent) = delete {
            if let Some(sel) = self.selected {
                actions.push(Action::Delete(
                    sel,
                    if permanent {
                        DeleteMode::Permanent
                    } else {
                        DeleteMode::RecycleBin
                    },
                ));
            }
        }
        let target = self.selected.unwrap_or(self.current);
        if copy {
            actions.push(Action::CopyPath(target));
        }
        if reveal {
            actions.push(Action::Reveal(target));
        }
        if up {
            actions.push(Action::Up);
        }
        if enter {
            if let Some(sel) = self.selected {
                actions.push(Action::Open(sel));
            }
        }
        if home {
            actions.push(Action::GoTo(Tree::ROOT));
        }
        if f5 {
            actions.push(Action::Rescan);
        }
        if help {
            actions.push(Action::ShowShortcuts);
        }
        if esc {
            self.selected = None;
        }
        if prev || next {
            self.refresh_list();
            if !self.list.is_empty() {
                let idx = self.selected.and_then(|s| self.list_position(s));
                let new = match (idx, next) {
                    (None, _) => 0,
                    (Some(i), true) => (i + 1).min(self.list.len() - 1),
                    (Some(i), false) => i.saturating_sub(1),
                };
                self.selected = Some(self.list[new]);
                self.list_scroll_to = Some(new);
            }
        }
    }
}

impl eframe::App for DiskTreeApp {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.owner_hwnd == 0 {
            self.owner_hwnd = window_handle(frame);
        }
        if let Drives::Loading(rx) = &self.drives {
            if let Ok(d) = rx.try_recv() {
                diag::log(&format!("found {} fixed drives", d.len()));
                self.drives = Drives::Ready(d);
            }
        }
        self.poll_scan(&ctx);
        self.poll_task(&ctx);

        // Dropping a folder onto the window scans it.
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| p.is_dir())
        });
        if let Some(p) = dropped {
            if self.scan.is_none() {
                self.path_input = p.display().to_string();
                self.start_scan(&ctx, p, None);
            }
        }

        let mut actions = Vec::new();
        if self.scan.is_some() {
            self.scanning_screen(ui);
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.tree.is_none() {
            self.start_screen(ui);
        } else {
            self.handle_keys(&ctx, &mut actions);
            self.browser(ui, &mut actions);
        }
        for a in actions {
            self.apply(&ctx, a);
        }
        self.dialogs(&ctx);
        self.paint_toast(&ctx);
        if self.task.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

fn window_handle(frame: &eframe::Frame) -> isize {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = frame.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                return w.hwnd.get();
            }
        }
    }
    let _ = frame;
    0
}

/// egui's bundled fonts cover Latin, Greek and Cyrillic only. File names can
/// be anything, so add Windows' own fonts as fallbacks (read off-thread).
fn load_system_fonts_in_background(ctx: &egui::Context) {
    if !cfg!(windows) {
        return;
    }
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("disktree-worker-fonts".into())
        .spawn(move || {
            let dir = std::env::var_os("SystemRoot")
                .or_else(|| std::env::var_os("windir"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join("Fonts");
            let mut fonts = egui::FontDefinitions::default();
            let mut added = Vec::new();
            for file in [
                "segoeui.ttf",
                "seguisym.ttf",
                "msyh.ttc",
                "YuGothR.ttc",
                "meiryo.ttc",
                "malgun.ttf",
                "Nirmala.ttc",
                "seguiemj.ttf",
            ] {
                if let Ok(bytes) = std::fs::read(dir.join(file)) {
                    fonts
                        .font_data
                        .insert(file.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                        fonts
                            .families
                            .entry(family)
                            .or_default()
                            .push(file.to_owned());
                    }
                    added.push(file);
                }
            }
            diag::log(&format!("fallback fonts: {added:?}"));
            if !added.is_empty() {
                ctx.set_fonts(fonts);
                ctx.request_repaint();
            }
        });
}

fn setup_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(12, 14, 19);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(58, 64, 80));
    visuals.faint_bg_color = CARD;
    visuals.selection.bg_fill = Color32::from_rgb(45, 90, 160);
    visuals.hyperlink_color = ACCENT;
    let square = egui::CornerRadius::ZERO;
    visuals.widgets.noninteractive.corner_radius = square;
    visuals.widgets.inactive.corner_radius = square;
    visuals.widgets.hovered.corner_radius = square;
    visuals.widgets.active.corner_radius = square;
    visuals.widgets.open.corner_radius = square;
    // The palette is dark; don't follow a light OS theme.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 4.0);
    });
}
