//! The Cache Deck window.

use crate::commands;
use crate::config::{Config, ConnCfg, Env, ServerKind, now_secs};
use crate::theme::{self, Palette, palette, type_color, type_label};
use crate::worker::{
    After, Bytes, ITEM_LIMIT, InfoSnapshot, Job, KeyDetail, KeyRow, Msg, ProbeStats, ServerMeta, ValueData, Worker,
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, RichText, Sense, Stroke,
    StrokeKind, TextEdit, UiBuilder, vec2,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Options {
    pub screenshot: Option<std::path::PathBuf>,
    pub connect: Option<(String, u16)>,
    pub select: Option<String>,
    pub tab: Option<String>,
    pub light: bool,
    pub run: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Dashboard,
    Keys,
    Console,
    Info,
}

enum ConnState {
    Idle,
    Connecting(u64),
    Connected { id: u64, meta: ServerMeta, db: i64 },
}

#[derive(Default)]
struct KeysView {
    pattern: String,
    kind: Option<&'static str>,
    rows: Vec<KeyRow>,
    cursor: u64,
    done: bool,
    loading: bool,
    selected: Option<Bytes>,
    detail: Option<KeyDetail>,
    loading_key: bool,
    str_buf: String,
    str_orig: String,
    ttl_buf: String,
    renaming: Option<String>,
    add_a: String,
    add_b: String,
    edit: Option<ItemEdit>,
    filter: String,
    pending_select: Option<String>,
}

struct ItemEdit {
    idx: usize,
    b: String,
}

struct ConsoleEntry {
    cmd: String,
    out: String,
    err: bool,
}

#[derive(Default)]
struct ConsoleView {
    input: String,
    log: Vec<ConsoleEntry>,
    history: Vec<String>,
    hist_pos: Option<usize>,
    server_cmds: Vec<String>,
    busy: bool,
}

#[derive(Default)]
struct InfoView {
    snap: Option<InfoSnapshot>,
    prev: Option<InfoSnapshot>,
    cpu: Option<f64>,
    last_req: Option<Instant>,
    pending: bool,
}

enum TestState {
    None,
    Running,
    Ok(String),
    Err(String),
}

struct ConnEditor {
    cfg: ConnCfg,
    is_new: bool,
    test: TestState,
    show_pw: bool,
    port_text: String,
    timeout_text: String,
}

struct Confirm {
    title: String,
    body: String,
    action: String,
    job: Option<Job>,
    forget_conn: Option<u64>,
}

struct NewKey {
    name: String,
    kind: &'static str,
    a: String,
    b: String,
    ttl: String,
}

enum ProbeState {
    Pending,
    Ok(ProbeStats),
    Err(String),
}

#[derive(Clone, Copy, PartialEq)]
enum SortCol {
    Name,
    Memory,
    Keys,
    Ops,
    LastUsed,
}

struct DashView {
    search: String,
    /// None = all; Some(None) = untagged; Some(Some(env)) = that environment.
    env: Option<Option<Env>>,
    sort: SortCol,
    asc: bool,
    last_refresh: Option<Instant>,
    inflight: std::collections::HashSet<u64>,
}

struct Toast {
    text: String,
    err: bool,
    at: Instant,
}

struct Shot {
    path: std::path::PathBuf,
    frames: u32,
    requested: bool,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

pub struct CacheDeck {
    cfg: Config,
    persist: bool,
    worker: Worker,
    logo: egui::TextureHandle,
    conn: ConnState,
    tab: Tab,
    keys: KeysView,
    console: ConsoleView,
    info: InfoView,
    editor: Option<ConnEditor>,
    confirm: Option<Confirm>,
    new_key: Option<NewKey>,
    toasts: Vec<Toast>,
    shot: Option<Shot>,
    startup_cmds: Vec<String>,
    about: bool,
    native_about_ready: bool,
    probes: std::collections::HashMap<u64, ProbeState>,
    dash: DashView,
}

pub const DEVELOPER: &str = "Shaon Dey";
pub const GITHUB_URL: &str = "https://github.com/ShaonDey";

const TLS_SKIP_HINT: &str = "For tunnels and port forwards, where the host is 127.0.0.1 but the certificate names the real server. The certificate must still come from a trusted authority.";

const KIND_FILTERS: [(Option<&str>, &str); 8] = [
    (None, "All types"),
    (Some("string"), "String"),
    (Some("hash"), "Hash"),
    (Some("list"), "List"),
    (Some("set"), "Set"),
    (Some("zset"), "Sorted set"),
    (Some("stream"), "Stream"),
    (Some("ReJSON-RL"), "JSON"),
];

const NEW_KINDS: [(&str, &str); 5] = [
    ("string", "String"),
    ("hash", "Hash"),
    ("list", "List"),
    ("set", "Set"),
    ("zset", "Sorted set"),
];

fn b(s: &str) -> Bytes {
    s.as_bytes().to_vec()
}

fn human_ttl(ms: i64) -> String {
    if ms == -1 {
        return "No expiry".into();
    }
    if ms < 0 {
        return "—".into();
    }
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else if s < 86400 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {}h", s / 86400, (s % 86400) / 3600)
    }
}

fn short_ttl(ms: i64) -> String {
    if ms < 0 {
        return "∞".into();
    }
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86400)
    }
}

fn human_bytes(n: i64) -> String {
    let n = n as f64;
    if n < 1024.0 {
        format!("{n} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else if n < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", n / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", n / 1024.0 / 1024.0 / 1024.0)
    }
}

fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let bytes = include_bytes!("../assets/app-icon/png/icon-256.png");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("app icon")
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture("app-logo", color, egui::TextureOptions::LINEAR)
}

// ---------- small drawing helpers ----------

fn primary_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.on_brand).strong())
            .fill(p.brand)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(7)),
    )
}

fn danger_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.error))
            .fill(p.error.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, p.error.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(7)),
    )
}

fn ghost_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.text))
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.border)),
    )
}

fn small_icon_button(ui: &mut egui::Ui, _p: &Palette, text: &str, color: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(color).size(13.0))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .min_size(vec2(26.0, 24.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn type_tag(ui: &mut egui::Ui, p: &Palette, kind: &str, width: f32) -> egui::Response {
    let color = type_color(kind, p);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(5),
        color.gamma_multiply(if p.dark { 0.18 } else { 0.16 }),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        type_label(kind),
        FontId::proportional(10.5),
        color,
    );
    resp
}

/// A table cell of fixed width; long text is cut with "…" and shown in full on hover.
fn cell(ui: &mut egui::Ui, w: f32, text: &str, rich: RichText) {
    ui.allocate_ui_with_layout(vec2(w, 22.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(w);
        let r = ui.add(egui::Label::new(rich).truncate());
        if text.chars().count() > 20 {
            r.on_hover_text(text);
        }
    });
}

/// Keep only a positive whole number from 1 to 65535 (empty while typing is allowed).
const PORT_MAX: u32 = 65535;
const TIMEOUT_MAX: u32 = crate::config::TIMEOUT_MAX_SECS;

/// Keep only digits, drop leading zeros and cap the value at `max`.
fn sanitize_number(text: &mut String, max: u32) {
    text.retain(|c| c.is_ascii_digit());
    let trimmed = text.trim_start_matches('0').to_string();
    if trimmed != *text {
        *text = trimmed;
    }
    let max_text = max.to_string();
    if text.len() > max_text.len() || text.parse::<u32>().map(|n| n > max).unwrap_or(false) {
        *text = max_text;
    }
}

/// The number typed into a number box, if it is between 1 and `max`.
fn parse_number(text: &str, max: u32) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|n| (1..=max).contains(n))
}

#[cfg(test)]
mod port_tests {
    use super::{PORT_MAX, TIMEOUT_MAX, parse_number, sanitize_number};

    fn sanitize_port(text: &mut String) {
        sanitize_number(text, PORT_MAX);
    }

    #[test]
    fn timeout_follows_the_same_rules() {
        for (input, want) in [
            ("10", "10"),
            ("999", "120"),
            ("1200", "120"),
            ("0", ""),
            ("007", "7"),
            ("3s", "3"),
            ("-5", "5"),
            ("", ""),
        ] {
            let mut s = input.to_string();
            sanitize_number(&mut s, TIMEOUT_MAX);
            assert_eq!(s, want, "input {input:?}");
        }
        assert_eq!(parse_number("10", TIMEOUT_MAX), Some(10));
        assert_eq!(parse_number("120", TIMEOUT_MAX), Some(120));
        assert_eq!(parse_number("121", TIMEOUT_MAX), None);
        assert_eq!(parse_number("0", TIMEOUT_MAX), None);
        assert_eq!(parse_number("", TIMEOUT_MAX), None);
    }

    #[test]
    fn only_positive_numbers() {
        for (input, want) in [
            ("abc12x3", "123"),
            ("0080", "80"),
            ("99999", "65535"),
            ("-5", "5"),
            ("6379", "6379"),
            ("", ""),
            ("0", ""),
            ("1.5e3", "153"),
            (" 42 ", "42"),
            ("１２", ""),
        ] {
            let mut s = input.to_string();
            sanitize_port(&mut s);
            assert_eq!(s, want, "input {input:?}");
        }
    }
}

/// A number box (port, timeout): digits only, 1 to `max`, with ▲/▼ buttons and Up/Down
/// keys to step it. `name` must differ between boxes shown at the same time.
fn number_input(ui: &mut egui::Ui, p: &Palette, text: &mut String, name: &'static str, max: u32) {
    let id = Id::unique(name);
    let step = |text: &mut String, delta: i64| {
        let cur = text.parse::<i64>().unwrap_or(0);
        let next = (cur + delta).clamp(1, max as i64);
        *text = next.to_string();
    };
    let focused = ui.memory(|m| m.has_focus(id));
    let hovered_before = ui.ctx().read_response(id).map(|r| r.hovered()).unwrap_or(false);
    Frame::new()
        .fill(p.bg)
        .stroke(Stroke::new(1.0, if focused { p.brand } else { p.border }))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin {
            left: 10,
            right: 3,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(vec2(94.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().interact_size.y = 20.0;
                ui.spacing_mut().item_spacing.x = 4.0;
                if focused {
                    let (up, down) = ui.input_mut(|i| {
                        (
                            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                        )
                    });
                    if up {
                        step(text, 1);
                    }
                    if down {
                        step(text, -1);
                    }
                }
                ui.add(
                    TextEdit::singleline(text)
                        .id(id)
                        .frame(Frame::NONE)
                        .char_limit(max.to_string().len())
                        .margin(Margin::symmetric(0, 0))
                        .align(Align2::LEFT_CENTER)
                        .min_size(vec2(64.0, 20.0))
                        .desired_width(64.0),
                );
                sanitize_number(text, max);
                // ▲/▼ stepper
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y));
                let bottom = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.center().y), rect.max);
                let up = ui
                    .interact(top, id.with("up"), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let down = ui
                    .interact(bottom, id.with("down"), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let painter = ui.painter();
                painter.rect_filled(rect, CornerRadius::same(4), p.card_hover);
                for (r, resp, pointing_up) in [(top, &up, true), (bottom, &down, false)] {
                    let color = if resp.hovered() {
                        p.brand
                    } else {
                        p.text.gamma_multiply(if hovered_before || focused { 0.9 } else { 0.7 })
                    };
                    let c = r.center();
                    let (w, h) = (4.0, 3.0);
                    let pts = if pointing_up {
                        vec![
                            egui::pos2(c.x, c.y - h),
                            egui::pos2(c.x + w, c.y + h),
                            egui::pos2(c.x - w, c.y + h),
                        ]
                    } else {
                        vec![
                            egui::pos2(c.x - w, c.y - h),
                            egui::pos2(c.x + w, c.y - h),
                            egui::pos2(c.x, c.y + h),
                        ]
                    };
                    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
                }
                if up.clicked() {
                    step(text, 1);
                }
                if down.clicked() {
                    step(text, -1);
                }
            });
        });
}

fn env_color(e: Env, p: &Palette) -> Color32 {
    match e {
        Env::Production => p.error,
        Env::Staging => p.accent,
        Env::Development => p.info,
        Env::Local => p.brand,
    }
}

fn env_pill(ui: &mut egui::Ui, p: &Palette, e: Env) {
    let color = env_color(e, p);
    Frame::new()
        .fill(color.gamma_multiply(if p.dark { 0.18 } else { 0.14 }))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(e.tag()).size(10.5).color(color).strong())
        });
}

fn ago(secs: Option<u64>) -> String {
    let Some(t) = secs else { return "never".into() };
    let d = now_secs().saturating_sub(t);
    if d < 60 {
        "just now".into()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86400 {
        format!("{} h ago", d / 3600)
    } else {
        let days = d / 86400;
        format!("{days} day{} ago", if days == 1 { "" } else { "s" })
    }
}

fn section_label(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(p.muted).strong());
}

fn modal_frame(ctx: &egui::Context, p: &Palette) -> Frame {
    Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(24))
        .shadow(ctx.global_style().visuals.window_shadow)
}

fn card_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
    ui.painter()
        .circle_filled(rect.center(), 7.0, color.gamma_multiply(0.18));
}

impl CacheDeck {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut cfg = Config::load();
        let mut persist = opts.screenshot.is_none();
        if opts.light {
            cfg.dark = false;
        }
        theme::install(&ctx, cfg.dark);
        let worker = Worker::spawn(ctx.clone());
        let logo = load_logo(&ctx);
        let mut app = Self {
            cfg,
            persist,
            worker,
            logo,
            conn: ConnState::Idle,
            tab: match opts.tab.as_deref() {
                Some("commands") | Some("console") => Tab::Console,
                Some("info") => Tab::Info,
                Some("keys") => Tab::Keys,
                _ => Tab::Dashboard,
            },
            keys: KeysView {
                pattern: String::new(),
                done: true,
                ..Default::default()
            },
            console: ConsoleView::default(),
            info: InfoView::default(),
            editor: None,
            confirm: None,
            new_key: None,
            toasts: Vec::new(),
            startup_cmds: opts.run.clone(),
            about: opts.tab.as_deref() == Some("about"),
            native_about_ready: false,
            probes: Default::default(),
            dash: DashView {
                search: String::new(),
                env: None,
                sort: SortCol::Name,
                asc: true,
                last_refresh: None,
                inflight: Default::default(),
            },
            shot: opts.screenshot.map(|path| Shot {
                path,
                frames: 0,
                requested: false,
                result: Arc::new(Mutex::new(None)),
            }),
        };
        if opts.tab.as_deref() == Some("new") {
            app.open_new_conn();
        }
        if let Some((host, port)) = opts.connect {
            // A one-off connection from the command line: shown, never saved.
            persist = false;
            app.persist = persist;
            let id = app.cfg.alloc_id();
            let c = ConnCfg {
                id,
                name: format!("{host}:{port}"),
                host,
                port,
                ..Default::default()
            };
            app.cfg.connections.insert(0, c.clone());
            app.keys.pending_select = opts.select;
            app.connect(c);
        }
        app
    }

    fn save_cfg(&mut self) {
        if !self.persist {
            return;
        }
        if let Err(e) = self.cfg.save() {
            self.toast(format!("Couldn't save settings: {e}"), true);
        }
    }

    fn toast(&mut self, text: impl Into<String>, err: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            err,
            at: Instant::now(),
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn connected_id(&self) -> Option<u64> {
        match &self.conn {
            ConnState::Connected { id, .. } => Some(*id),
            _ => None,
        }
    }

    fn conn_cfg(&self, id: u64) -> Option<&ConnCfg> {
        self.cfg.connections.iter().find(|c| c.id == id)
    }

    fn connect(&mut self, c: ConnCfg) {
        if self.tab == Tab::Dashboard {
            self.tab = Tab::Keys;
        }
        self.conn = ConnState::Connecting(c.id);
        self.keys = KeysView {
            pending_select: self.keys.pending_select.take(),
            done: true,
            ..Default::default()
        };
        self.info = InfoView::default();
        self.worker.send(Job::Connect(c));
    }

    fn switch_db(&mut self, db: i64) {
        let Some(id) = self.connected_id() else { return };
        if let Some(mut c) = self.conn_cfg(id).cloned() {
            c.db = db;
            self.conn = ConnState::Connecting(id);
            self.keys = KeysView {
                done: true,
                ..Default::default()
            };
            self.worker.send(Job::Connect(c));
            // Remember the chosen db on the connection itself.
            if let Some(saved) = self.cfg.connections.iter_mut().find(|x| x.id == id) {
                saved.db = db;
            }
            self.save_cfg();
        }
    }

    fn scan(&mut self, reset: bool) {
        if !matches!(self.conn, ConnState::Connected { .. }) {
            return;
        }
        let raw = self.keys.pattern.trim().to_string();
        let pattern = if raw.is_empty() {
            "*".to_string()
        } else if raw.contains(['*', '?', '[']) {
            raw
        } else {
            format!("*{raw}*")
        };
        let cursor = if reset { 0 } else { self.keys.cursor };
        if !reset && self.keys.done {
            return;
        }
        self.keys.loading = true;
        self.worker.send(Job::Scan {
            pattern,
            kind: self.keys.kind.map(str::to_string),
            cursor,
            reset,
        });
    }

    fn select_key(&mut self, key: Bytes) {
        self.keys.selected = Some(key.clone());
        self.keys.loading_key = true;
        self.keys.edit = None;
        self.keys.renaming = None;
        self.keys.add_a.clear();
        self.keys.add_b.clear();
        self.keys.filter.clear();
        self.worker.send(Job::Load(key));
    }

    fn ops(&mut self, cmds: Vec<Vec<Bytes>>, after: After, ok: &str) {
        self.worker.send(Job::Ops {
            cmds,
            must_not_exist: None,
            after,
            ok: ok.to_string(),
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.worker.rx.try_recv() {
            match msg {
                Msg::Connected(meta, cmds) => {
                    let id = match self.conn {
                        ConnState::Connecting(id) => id,
                        ConnState::Connected { id, .. } => id,
                        ConnState::Idle => continue,
                    };
                    let db = self.conn_cfg(id).map(|c| c.db).unwrap_or(0);
                    if let Some(w) = &meta.mismatch {
                        self.toast(w.clone(), true);
                    }
                    self.console.server_cmds = cmds;
                    if let Some(c) = self.cfg.connections.iter_mut().find(|c| c.id == id) {
                        c.last_used = Some(now_secs());
                    }
                    self.save_cfg();
                    self.conn = ConnState::Connected { id, meta, db };
                    for line in std::mem::take(&mut self.startup_cmds) {
                        if let Ok(args) = crate::worker::split_args(&line) {
                            self.console.history.push(line);
                            self.worker.send(Job::Console(args));
                        }
                    }
                    self.scan(true);
                    self.request_info();
                }
                Msg::ConnectFailed(e) => {
                    self.conn = ConnState::Idle;
                    self.toast(e, true);
                }
                Msg::Disconnected(e) => {
                    self.conn = ConnState::Idle;
                    self.keys.loading = false;
                    self.console.busy = false;
                    self.toast(e, true);
                }
                Msg::Scanned { reset, keys, cursor } => {
                    if reset {
                        self.keys.rows = keys;
                    } else {
                        let mut seen: std::collections::HashSet<Bytes> =
                            self.keys.rows.iter().map(|r| r.key.clone()).collect();
                        self.keys
                            .rows
                            .extend(keys.into_iter().filter(|r| seen.insert(r.key.clone())));
                    }
                    self.keys.cursor = cursor;
                    self.keys.done = cursor == 0;
                    self.keys.loading = false;
                    if let Some(want) = self.keys.pending_select.take()
                        && let Some(r) = self.keys.rows.iter().find(|r| r.name == want)
                    {
                        let k = r.key.clone();
                        self.select_key(k);
                    }
                }
                Msg::Key(d) => {
                    if self.keys.selected.as_ref() != Some(&d.key) {
                        continue;
                    }
                    self.keys.loading_key = false;
                    if d.kind == "none" {
                        self.toast(format!("\"{}\" no longer exists", d.name), true);
                        self.keys.rows.retain(|r| r.key != d.key);
                        self.keys.selected = None;
                        self.keys.detail = None;
                        continue;
                    }
                    if let Some(r) = self.keys.rows.iter_mut().find(|r| r.key == d.key) {
                        r.kind = d.kind.clone();
                        r.ttl_ms = d.ttl_ms;
                    }
                    if let ValueData::Str(s) = &d.value {
                        self.keys.str_buf = s.clone();
                        self.keys.str_orig = s.clone();
                    }
                    self.keys.ttl_buf = if d.ttl_ms > 0 {
                        (d.ttl_ms / 1000).max(1).to_string()
                    } else {
                        String::new()
                    };
                    self.keys.edit = None;
                    self.keys.detail = Some(d);
                }
                Msg::OpsDone { after, ok } => {
                    self.toast(ok, false);
                    match after {
                        After::ReloadKey(k) => {
                            if self.keys.selected.as_ref() == Some(&k) {
                                self.worker.send(Job::Load(k));
                            }
                        }
                        After::KeyDeleted(k) => {
                            self.keys.rows.retain(|r| r.key != k);
                            if self.keys.selected.as_ref() == Some(&k) {
                                self.keys.selected = None;
                                self.keys.detail = None;
                            }
                        }
                        After::KeyRenamed(k) | After::KeyCreated(k) => {
                            self.scan(true);
                            self.select_key(k);
                        }
                    }
                }
                Msg::Failed(e) => {
                    self.keys.loading = false;
                    self.keys.loading_key = false;
                    self.info.pending = false;
                    self.toast(e, true);
                }
                Msg::Info(s) => {
                    let cpu_of = |x: &InfoSnapshot| {
                        let p = |k| x.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        p("used_cpu_sys") + p("used_cpu_user")
                    };
                    if let Some(prev) = &self.info.snap {
                        let dt = s.at.duration_since(prev.at).as_secs_f64();
                        if dt > 0.2 {
                            self.info.cpu = Some(((cpu_of(&s) - cpu_of(prev)) / dt * 100.0).max(0.0));
                        }
                    }
                    self.info.prev = self.info.snap.take();
                    self.info.snap = Some(s);
                    self.info.pending = false;
                }
                Msg::Console { cmd, out, err } => {
                    self.console.busy = false;
                    self.console.log.push(ConsoleEntry { cmd, out, err });
                    if self.console.log.len() > 300 {
                        self.console.log.remove(0);
                    }
                }
                Msg::Probe(id, r) => {
                    self.dash.inflight.remove(&id);
                    self.probes.insert(
                        id,
                        match r {
                            Ok(st) => ProbeState::Ok(st),
                            Err(e) => ProbeState::Err(e),
                        },
                    );
                }
                Msg::Test(r) => {
                    if let Some(ed) = &mut self.editor {
                        ed.test = match r {
                            Ok(s) => TestState::Ok(s),
                            Err(e) => TestState::Err(e),
                        };
                    }
                }
            }
        }
    }

    fn request_info(&mut self) {
        if self.info.pending || !matches!(self.conn, ConnState::Connected { .. }) {
            return;
        }
        self.info.pending = true;
        self.info.last_req = Some(Instant::now());
        self.worker.send(Job::Info);
    }

    // ---------- top bar ----------

    fn top_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            let (label, color) = match &self.conn {
                ConnState::Idle => ("Not connected".to_string(), p.muted),
                ConnState::Connecting(id) => (
                    format!(
                        "Connecting to {}…",
                        self.conn_cfg(*id).map(|c| c.display_name()).unwrap_or_default()
                    ),
                    p.accent,
                ),
                ConnState::Connected { id, .. } => (
                    self.conn_cfg(*id).map(|c| c.display_name()).unwrap_or_default(),
                    p.success,
                ),
            };
            dot(ui, color);
            ui.label(RichText::new(label).size(15.0).strong());
            if let ConnState::Connected { meta, db, .. } = &self.conn {
                let (meta, db) = (meta.clone(), *db);
                let badge = format!("{} {}", meta.kind, meta.version);
                Frame::new()
                    .fill(p.brand.gamma_multiply(0.14))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| ui.label(RichText::new(badge).size(12.0).color(p.brand)));
                ui.add_space(6.0);
                let mut chosen = db;
                egui::ComboBox::from_id_salt("db-pick")
                    .width(84.0)
                    .selected_text(format!("Database {db}"))
                    .show_ui(ui, |ui| {
                        for i in 0..meta.databases.clamp(1, 256) {
                            ui.selectable_value(&mut chosen, i, format!("Database {i}"));
                        }
                    });
                if chosen != db {
                    self.switch_db(chosen);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Icon only: the sun switches to the light theme, the moon to the dark one.
                let icon = if self.cfg.dark { "☀" } else { "🌙" };
                let tip = if self.cfg.dark { "Light theme" } else { "Dark theme" };
                if ghost_button(ui, p, icon).on_hover_text(tip).clicked() {
                    self.cfg.dark = !self.cfg.dark;
                    theme::set_dark(ui.ctx(), self.cfg.dark);
                    self.save_cfg();
                }
                if matches!(self.conn, ConnState::Connected { .. } | ConnState::Connecting(_))
                    && ghost_button(ui, p, "Disconnect").clicked()
                {
                    self.worker.send(Job::Disconnect);
                    self.conn = ConnState::Idle;
                    self.keys = KeysView {
                        done: true,
                        ..Default::default()
                    };
                }
            });
        });
    }

    // ---------- sidebar ----------

    fn sidebar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            ui.painter().image(
                self.logo.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.vertical(|ui| {
                ui.add_space(1.0);
                ui.label(RichText::new("Cache Deck").size(17.0).strong());
                ui.label(RichText::new("for Redis & Valkey").size(11.0).color(p.muted));
            });
        });
        ui.add_space(14.0);
        {
            let on = self.tab == Tab::Dashboard || !matches!(self.conn, ConnState::Connected { .. });
            let resp = ui
                .scope_builder(UiBuilder::new().id_salt("nav-dashboard").sense(Sense::click()), |ui| {
                    let hovered = ui.response().hovered();
                    Frame::new()
                        .fill(if on {
                            p.brand.gamma_multiply(0.14)
                        } else if hovered {
                            p.card
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("🏠").color(if on { p.brand } else { p.muted }));
                                ui.label(
                                    RichText::new("Dashboard")
                                        .color(if on { p.brand } else { p.text })
                                        .strong(),
                                );
                            });
                        });
                })
                .response;
            if resp.clicked() {
                self.tab = Tab::Dashboard;
                self.dash.last_refresh = None;
            }
        }
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            section_label(ui, p, "CONNECTIONS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_icon_button(ui, p, "➕", p.brand)
                    .on_hover_text("New connection")
                    .clicked()
                {
                    self.open_new_conn();
                }
            });
        });
        ui.add_space(2.0);

        let mut connect_to: Option<ConnCfg> = None;
        let mut edit: Option<ConnCfg> = None;
        let mut forget: Option<ConnCfg> = None;
        let conns = self.cfg.connections.clone();
        let active = match &self.conn {
            ConnState::Connected { id, .. } => Some((*id, p.success)),
            ConnState::Connecting(id) => Some((*id, p.accent)),
            ConnState::Idle => None,
        };
        egui::ScrollArea::vertical()
            .id_salt("conn-list")
            .max_height(ui.available_height() - 150.0)
            .show(ui, |ui| {
                if conns.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("No saved connections yet.").color(p.muted).size(12.5));
                }
                for c in &conns {
                    let is_active = active.map(|(id, _)| id == c.id).unwrap_or(false);
                    let resp = ui
                        .scope_builder(UiBuilder::new().id_salt(("conn", c.id)).sense(Sense::click()), |ui| {
                            let hovered = ui.response().hovered();
                            let fill = if is_active {
                                p.brand.gamma_multiply(0.12)
                            } else if hovered {
                                p.card
                            } else {
                                Color32::TRANSPARENT
                            };
                            let stroke = if is_active {
                                Stroke::new(1.0, p.brand.gamma_multiply(0.6))
                            } else {
                                Stroke::NONE
                            };
                            Frame::new()
                                .fill(fill)
                                .stroke(stroke)
                                .corner_radius(CornerRadius::same(9))
                                .inner_margin(Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let idle = match self.probes.get(&c.id) {
                                            Some(ProbeState::Ok(_)) => p.success.gamma_multiply(0.55),
                                            Some(ProbeState::Err(_)) => p.error.gamma_multiply(0.75),
                                            _ => p.muted.gamma_multiply(0.6),
                                        };
                                        dot(
                                            ui,
                                            active.filter(|(id, _)| *id == c.id).map(|(_, col)| col).unwrap_or(idle),
                                        );
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 1.0;
                                            ui.label(RichText::new(c.display_name()).strong().size(13.5));
                                            let kind = if c.kind == ServerKind::Auto {
                                                String::new()
                                            } else {
                                                format!(" · {}", c.kind.label())
                                            };
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}:{} · db{}{}{}",
                                                    c.host,
                                                    c.port,
                                                    c.db,
                                                    if c.tls { " · TLS" } else { "" },
                                                    kind
                                                ))
                                                .size(11.5)
                                                .color(p.muted),
                                            );
                                        });
                                        if hovered || is_active {
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.spacing_mut().item_spacing.x = 0.0;
                                                if small_icon_button(ui, p, "🗑", p.muted)
                                                    .on_hover_text("Delete")
                                                    .clicked()
                                                {
                                                    forget = Some(c.clone());
                                                }
                                                if small_icon_button(ui, p, "✏", p.muted)
                                                    .on_hover_text("Edit")
                                                    .clicked()
                                                {
                                                    edit = Some(c.clone());
                                                }
                                            });
                                        }
                                    });
                                });
                        })
                        .response;
                    if resp.clicked() && edit.is_none() && forget.is_none() {
                        connect_to = Some(c.clone());
                    }
                    ui.add_space(2.0);
                }
            });
        if let Some(c) = connect_to {
            if self.connected_id() != Some(c.id) {
                self.connect(c);
            } else if self.tab == Tab::Dashboard {
                self.tab = Tab::Keys;
            }
        }
        if let Some(c) = edit {
            self.editor = Some(ConnEditor {
                port_text: c.port.to_string(),
                timeout_text: c.timeout_secs.to_string(),
                cfg: c,
                is_new: false,
                test: TestState::None,
                show_pw: false,
            });
        }
        if let Some(c) = forget {
            self.confirm = Some(Confirm {
                title: "Delete connection?".into(),
                body: format!(
                    "\"{}\" will be removed from your saved connections. Nothing on the server changes.",
                    c.display_name()
                ),
                action: "Delete".into(),
                job: None,
                forget_conn: Some(c.id),
            });
        }

        if matches!(self.conn, ConnState::Connected { .. }) {
            ui.add_space(12.0);
            section_label(ui, p, "WORKSPACE");
            ui.add_space(2.0);
            for (tab, icon, label) in [
                (Tab::Keys, "🔑", "Keys"),
                (Tab::Console, "⌨", "Commands"),
                (Tab::Info, "📊", "Server info"),
            ] {
                let on = self.tab == tab;
                let resp = ui
                    .scope_builder(UiBuilder::new().id_salt(("tab", label)).sense(Sense::click()), |ui| {
                        let hovered = ui.response().hovered();
                        Frame::new()
                            .fill(if on {
                                p.brand.gamma_multiply(0.14)
                            } else if hovered {
                                p.card
                            } else {
                                Color32::TRANSPARENT
                            })
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(icon).color(if on { p.brand } else { p.muted }));
                                    ui.label(RichText::new(label).color(if on { p.brand } else { p.text }).strong());
                                });
                            });
                    })
                    .response;
                if resp.clicked() {
                    self.tab = tab;
                    if tab == Tab::Info {
                        self.request_info();
                    }
                }
            }
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            if primary_button(ui, p, "➕  New connection").clicked() {
                self.open_new_conn();
            }
        });
    }

    fn open_new_conn(&mut self) {
        let cfg = ConnCfg {
            name: String::new(),
            ..Default::default()
        };
        self.editor = Some(ConnEditor {
            port_text: cfg.port.to_string(),
            timeout_text: cfg.timeout_secs.to_string(),
            cfg,
            is_new: true,
            test: TestState::None,
            show_pw: false,
        });
    }

    // ---------- status bar ----------

    fn status_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            match &self.conn {
                ConnState::Connected { id, meta, db } => {
                    dot(ui, p.success);
                    let c = self.conn_cfg(*id);
                    let addr = c.map(|c| format!("{}:{}", c.host, c.port)).unwrap_or_default();
                    let keys = self.info.snap.as_ref().map(keyspace_total).unwrap_or(meta.dbsize);
                    ui.label(
                        RichText::new(format!(
                            "Connected · {addr} · {} {} · {} · db{db} · {} key{}",
                            meta.kind,
                            meta.version,
                            meta.mode,
                            thousands(keys),
                            if keys == 1 { "" } else { "s" }
                        ))
                        .size(12.0)
                        .color(p.muted),
                    );
                }
                ConnState::Connecting(_) => {
                    dot(ui, p.accent);
                    ui.label(RichText::new("Connecting…").size(12.0).color(p.muted));
                }
                ConnState::Idle => {
                    dot(ui, p.muted.gamma_multiply(0.6));
                    let total = self.cfg.connections.len();
                    let up = self
                        .cfg
                        .connections
                        .iter()
                        .filter(|c| matches!(self.probes.get(&c.id), Some(ProbeState::Ok(_))))
                        .count();
                    let text = if total > 0 && self.probes.len() >= total.min(1) {
                        format!("Not connected · {up} of {total} servers reachable")
                    } else {
                        "Not connected".to_string()
                    };
                    ui.label(RichText::new(text).size(12.0).color(p.muted));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let r = ui
                    .add(
                        egui::Label::new(
                            RichText::new(concat!("Cache Deck ", env!("CARGO_PKG_VERSION")))
                                .size(11.5)
                                .color(p.muted),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("About Cache Deck");
                if r.clicked() {
                    self.about = true;
                }
            });
        });
    }

    // ---------- dashboard ----------

    fn probe_one(&mut self, c: ConnCfg, reset: bool) {
        if !self.dash.inflight.insert(c.id) {
            return;
        }
        if reset || !self.probes.contains_key(&c.id) {
            self.probes.insert(c.id, ProbeState::Pending);
        }
        self.worker.probe(c);
    }

    fn probe_all(&mut self) {
        for c in self.cfg.connections.clone() {
            self.probe_one(c, false);
        }
        self.dash.last_refresh = Some(Instant::now());
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, p: &Palette) {
        if self.cfg.connections.is_empty() {
            self.welcome(ui, p);
            return;
        }
        if self
            .dash
            .last_refresh
            .map(|t| t.elapsed() > Duration::from_secs(20))
            .unwrap_or(true)
        {
            self.probe_all();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));

        let conns = self.cfg.connections.clone();
        let snap: std::collections::HashMap<u64, ProbeStats> = self
            .probes
            .iter()
            .filter_map(|(id, st)| match st {
                ProbeState::Ok(x) => Some((*id, x.clone())),
                _ => None,
            })
            .collect();
        let stats = |id: u64| snap.get(&id).cloned();
        let online: Vec<ProbeStats> = conns.iter().filter_map(|c| stats(c.id)).collect();
        let offline = conns
            .iter()
            .filter(|c| matches!(self.probes.get(&c.id), Some(ProbeState::Err(_))))
            .count();
        let hot = online
            .iter()
            .filter(|s| s.max_mem > 0 && s.used_mem as f64 / s.max_mem as f64 > 0.8)
            .count();

        Frame::new().inner_margin(Margin::symmetric(24, 18)).show(ui, |ui| {
            // header
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Dashboard").size(24.0).strong());
                    let n = conns.len();
                    ui.label(
                        RichText::new(format!(
                            "{n} saved server{} · {} online · {offline} offline",
                            if n == 1 { "" } else { "s" },
                            online.len()
                        ))
                        .size(12.5)
                        .color(p.muted),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, p, "➕  New connection").clicked() {
                        self.open_new_conn();
                    }
                    let busy = !self.dash.inflight.is_empty();
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(RichText::new("🔄  Refresh all").color(p.text))
                                .fill(p.card)
                                .stroke(Stroke::new(1.0, p.border)),
                        )
                        .clicked()
                    {
                        self.probe_all();
                    }
                    if busy {
                        ui.spinner();
                    }
                    ui.add(
                        TextEdit::singleline(&mut self.dash.search)
                            .hint_text("🔍  Search servers")
                            .desired_width(210.0),
                    );
                });
            });
            ui.add_space(14.0);

            // summary tiles
            let keys: i64 = online.iter().map(|s| s.keys).sum();
            let memv: i64 = online.iter().map(|s| s.used_mem).sum();
            let ops: i64 = online.iter().map(|s| s.ops).sum();
            let attention = offline + hot;
            let tiles = [
                (
                    "🖥",
                    "SERVERS ONLINE",
                    format!("{} / {}", online.len(), conns.len()),
                    if attention == 0 {
                        "all healthy".to_string()
                    } else {
                        format!("{attention} need{} attention", if attention == 1 { "s" } else { "" })
                    },
                    if offline > 0 { p.accent } else { p.success },
                ),
                (
                    "🔑",
                    "TOTAL KEYS",
                    thousands(keys),
                    "across online servers".to_string(),
                    type_color("zset", p),
                ),
                (
                    "💾",
                    "MEMORY IN USE",
                    human_bytes(memv),
                    if hot == 0 {
                        "no server above 80%".to_string()
                    } else {
                        format!("{hot} server{} above 80%", if hot == 1 { "" } else { "s" })
                    },
                    p.info,
                ),
                (
                    "⚡",
                    "OPERATIONS / SEC",
                    thousands(ops),
                    "combined, right now".to_string(),
                    p.accent,
                ),
            ];
            let tw = ((ui.available_width() - 36.0) / 4.0).floor();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for (icon, title, big, sub, color) in tiles {
                    card_frame(p).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.set_width(tw - 30.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon).size(13.0).color(color));
                                ui.label(RichText::new(title).size(11.0).color(p.muted).strong());
                            });
                            ui.add(egui::Label::new(RichText::new(big).size(20.0).strong()).truncate());
                            ui.add(egui::Label::new(RichText::new(sub).size(11.5).color(p.muted)).truncate());
                        });
                    });
                }
            });
            ui.add_space(14.0);

            // environment filters
            ui.horizontal_wrapped(|ui| {
                let mut opts: Vec<(Option<Option<Env>>, String)> = vec![(None, format!("All  {}", conns.len()))];
                for e in Env::ALL {
                    let n = conns.iter().filter(|c| c.env == Some(e)).count();
                    if n > 0 {
                        opts.push((Some(Some(e)), format!("{}  {n}", e.title())));
                    }
                }
                let untagged = conns.iter().filter(|c| c.env.is_none()).count();
                if untagged > 0 && untagged < conns.len() {
                    opts.push((Some(None), format!("No tag  {untagged}")));
                }
                for (val, label) in opts {
                    let on = self.dash.env == val;
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(12.5).color(if on {
                                p.brand
                            } else {
                                p.muted
                            }))
                            .fill(if on {
                                p.brand.gamma_multiply(0.14)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::new(
                                1.0,
                                if on { p.brand.gamma_multiply(0.6) } else { p.border },
                            ))
                            .corner_radius(CornerRadius::same(14)),
                        )
                        .clicked()
                    {
                        self.dash.env = val;
                    }
                }
            });
            ui.add_space(10.0);

            // rows to show
            let q = self.dash.search.trim().to_lowercase();
            let mut rows: Vec<ConnCfg> = conns
                .iter()
                .filter(|c| match self.dash.env {
                    None => true,
                    Some(e) => c.env == e,
                })
                .filter(|c| {
                    q.is_empty()
                        || c.display_name().to_lowercase().contains(&q)
                        || format!("{}:{}", c.host, c.port).contains(&q)
                })
                .cloned()
                .collect();
            let mem_of = |c: &ConnCfg| stats(c.id).map(|s| s.used_mem).unwrap_or(-1);
            let keys_of = |c: &ConnCfg| stats(c.id).map(|s| s.keys).unwrap_or(-1);
            let ops_of = |c: &ConnCfg| stats(c.id).map(|s| s.ops).unwrap_or(-1);
            match self.dash.sort {
                SortCol::Name => rows.sort_by_key(|c| c.display_name().to_lowercase()),
                SortCol::Memory => rows.sort_by_key(mem_of),
                SortCol::Keys => rows.sort_by_key(keys_of),
                SortCol::Ops => rows.sort_by_key(ops_of),
                SortCol::LastUsed => rows.sort_by_key(|c| c.last_used.unwrap_or(0)),
            }
            if !self.dash.asc {
                rows.reverse();
            }

            // table
            let total = ui.available_width();
            let act = 120.0;
            let spacing = 8.0;
            let flex = (total - 32.0 - act - spacing * 7.0 - 4.0).max(500.0);
            let weights = [0.25, 0.15, 0.13, 0.17, 0.10, 0.09, 0.11];
            let cols: Vec<f32> = weights.iter().map(|w| (flex * w).floor()).collect();
            let mut open: Option<ConnCfg> = None;
            let mut retry: Option<ConnCfg> = None;
            let mut edit: Option<ConnCfg> = None;
            let mut forget: Option<ConnCfg> = None;
            let mut sort_click: Option<SortCol> = None;
            let active = self.connected_id();
            Frame::new()
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(12))
                .show(ui, |ui| {
                    ui.set_width(total);
                    Frame::new().inner_margin(Margin::symmetric(16, 10)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let heads = [
                                ("SERVER", Some(SortCol::Name)),
                                ("HOST : PORT", None),
                                ("TYPE", None),
                                ("MEMORY", Some(SortCol::Memory)),
                                ("KEYS", Some(SortCol::Keys)),
                                ("OPS / SEC", Some(SortCol::Ops)),
                                ("LAST USED", Some(SortCol::LastUsed)),
                            ];
                            for (i, (h, col)) in heads.iter().enumerate() {
                                ui.allocate_ui_with_layout(
                                    vec2(cols[i], 18.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_width(cols[i]);
                                        let arrow = match col {
                                            Some(c) if *c == self.dash.sort => {
                                                if self.dash.asc {
                                                    " ↑"
                                                } else {
                                                    " ↓"
                                                }
                                            }
                                            _ => "",
                                        };
                                        let on = arrow.is_empty();
                                        let text = RichText::new(format!("{h}{arrow}"))
                                            .size(11.0)
                                            .strong()
                                            .color(if on { p.muted } else { p.brand });
                                        if let Some(c) = col {
                                            let r = ui
                                                .add(egui::Label::new(text).sense(Sense::click()))
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if r.clicked() {
                                                sort_click = Some(*c);
                                            }
                                        } else {
                                            ui.label(text);
                                        }
                                    },
                                );
                            }
                        });
                    });
                    if rows.is_empty() {
                        let (r, _) = ui.allocate_exact_size(vec2(total, 0.0), Sense::hover());
                        ui.painter()
                            .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, p.border));
                        ui.vertical_centered(|ui| {
                            ui.add_space(18.0);
                            ui.label(RichText::new("No servers match").strong());
                            ui.label(RichText::new("Try another search or filter.").size(12.0).color(p.muted));
                            ui.add_space(18.0);
                        });
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("dash-rows")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for c in &rows {
                                let (r, _) = ui.allocate_exact_size(vec2(total, 0.0), Sense::hover());
                                ui.painter()
                                    .line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, p.border));
                                let state = self.probes.get(&c.id);
                                let is_active = active == Some(c.id);
                                let fill = if is_active {
                                    p.brand.gamma_multiply(0.07)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                Frame::new()
                                    .fill(fill)
                                    .inner_margin(Margin::symmetric(16, 9))
                                    .show(ui, |ui| {
                                        ui.set_width(total - 32.0);
                                        ui.horizontal(|ui| {
                                            // server
                                            ui.allocate_ui_with_layout(
                                                vec2(cols[0], 38.0),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.set_width(cols[0]);
                                                    let (dc, tip) = match state {
                                                        Some(ProbeState::Ok(_)) => (p.success, "Online".to_string()),
                                                        Some(ProbeState::Err(e)) => (p.error, format!("Offline: {e}")),
                                                        _ => (p.accent, "Checking…".to_string()),
                                                    };
                                                    let (dr, _) =
                                                        ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                                    ui.painter().circle_filled(dr.center(), 4.5, dc);
                                                    ui.painter().circle_filled(
                                                        dr.center(),
                                                        7.0,
                                                        dc.gamma_multiply(0.18),
                                                    );
                                                    ui.interact(
                                                        dr.expand(4.0),
                                                        Id::unique(("dash-dot", c.id)),
                                                        Sense::hover(),
                                                    )
                                                    .on_hover_text(tip);
                                                    ui.vertical(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 2.0;
                                                        ui.horizontal(|ui| {
                                                            if let Some(e) = c.env {
                                                                env_pill(ui, p, e);
                                                            }
                                                            ui.add(
                                                                egui::Label::new(
                                                                    RichText::new(c.display_name()).strong(),
                                                                )
                                                                .truncate(),
                                                            )
                                                            .on_hover_text(c.display_name());
                                                        });
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "Database {}{}",
                                                                c.db,
                                                                if c.tls { " · TLS" } else { "" }
                                                            ))
                                                            .size(11.0)
                                                            .color(p.muted),
                                                        );
                                                    });
                                                },
                                            );
                                            // host
                                            let addr = format!("{}:{}", c.host, c.port);
                                            ui.allocate_ui_with_layout(
                                                vec2(cols[1], 38.0),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.set_width(cols[1]);
                                                    ui.add(
                                                        egui::Label::new(RichText::new(&addr).monospace().size(12.5))
                                                            .truncate(),
                                                    )
                                                    .on_hover_text(&addr);
                                                },
                                            );
                                            // type
                                            ui.allocate_ui_with_layout(
                                                vec2(cols[2], 38.0),
                                                Layout::top_down(Align::Min),
                                                |ui| {
                                                    ui.set_width(cols[2]);
                                                    ui.spacing_mut().item_spacing.y = 1.0;
                                                    ui.add_space(2.0);
                                                    match state {
                                                        Some(ProbeState::Ok(st)) => {
                                                            ui.add(
                                                                egui::Label::new(format!("{} {}", st.kind, st.version))
                                                                    .truncate(),
                                                            );
                                                            let mut m = st.mode.clone();
                                                            if let Some(f) = m.get_mut(0..1) {
                                                                f.make_ascii_uppercase();
                                                            }
                                                            ui.label(RichText::new(m).size(11.0).color(p.muted));
                                                        }
                                                        _ => {
                                                            let k = if c.kind == ServerKind::Auto {
                                                                "—".to_string()
                                                            } else {
                                                                c.kind.label().to_string()
                                                            };
                                                            ui.label(RichText::new(k).color(p.muted));
                                                        }
                                                    }
                                                },
                                            );
                                            // memory
                                            ui.allocate_ui_with_layout(
                                                vec2(cols[3], 38.0),
                                                Layout::top_down(Align::Min),
                                                |ui| {
                                                    ui.set_width(cols[3]);
                                                    match state {
                                                        Some(ProbeState::Ok(st)) => {
                                                            ui.add_space(3.0);
                                                            let text = if st.max_mem > 0 {
                                                                format!(
                                                                    "{} of {}",
                                                                    human_bytes(st.used_mem),
                                                                    human_bytes(st.max_mem)
                                                                )
                                                            } else {
                                                                format!("{} · no limit", human_bytes(st.used_mem))
                                                            };
                                                            ui.add(
                                                                egui::Label::new(RichText::new(text).size(12.5))
                                                                    .truncate(),
                                                            );
                                                            let bw = (cols[3] - 16.0).max(20.0);
                                                            let (rect, _) =
                                                                ui.allocate_exact_size(vec2(bw, 6.0), Sense::hover());
                                                            ui.painter().rect_filled(
                                                                rect,
                                                                CornerRadius::same(3),
                                                                p.card_hover,
                                                            );
                                                            if st.max_mem > 0 {
                                                                let f = (st.used_mem as f32 / st.max_mem as f32)
                                                                    .clamp(0.0, 1.0);
                                                                let mut fr = rect;
                                                                fr.set_width((rect.width() * f).max(2.0));
                                                                ui.painter().rect_filled(
                                                                    fr,
                                                                    CornerRadius::same(3),
                                                                    if f > 0.8 { p.accent } else { p.brand },
                                                                );
                                                            }
                                                        }
                                                        Some(ProbeState::Pending) | None => {
                                                            ui.add_space(10.0);
                                                            ui.label(
                                                                RichText::new("Checking…").size(12.0).color(p.muted),
                                                            );
                                                        }
                                                        Some(ProbeState::Err(_)) => {
                                                            ui.add_space(10.0);
                                                            ui.label(
                                                                RichText::new("Offline").size(12.5).color(p.error),
                                                            );
                                                        }
                                                    }
                                                },
                                            );
                                            // keys, ops
                                            for (i, v) in [
                                                (4, stats(c.id).map(|s| thousands(s.keys))),
                                                (5, stats(c.id).map(|s| thousands(s.ops))),
                                            ] {
                                                ui.allocate_ui_with_layout(
                                                    vec2(cols[i], 38.0),
                                                    Layout::left_to_right(Align::Center),
                                                    |ui| {
                                                        ui.set_width(cols[i]);
                                                        match v {
                                                            Some(t) => ui.add(
                                                                egui::Label::new(RichText::new(t).strong()).truncate(),
                                                            ),
                                                            None => ui.label(RichText::new("—").color(p.muted)),
                                                        };
                                                    },
                                                );
                                            }
                                            // last used
                                            ui.allocate_ui_with_layout(
                                                vec2(cols[6], 38.0),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.set_width(cols[6]);
                                                    let t = if is_active {
                                                        "open now".to_string()
                                                    } else {
                                                        ago(c.last_used)
                                                    };
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(t).size(12.5).color(if is_active {
                                                                p.brand
                                                            } else {
                                                                p.muted
                                                            }),
                                                        )
                                                        .truncate(),
                                                    );
                                                },
                                            );
                                            // actions
                                            ui.allocate_ui_with_layout(
                                                vec2(act, 38.0),
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.menu_button(
                                                        RichText::new("⋮").size(16.0).color(p.muted),
                                                        |ui| {
                                                            if ui.button("✏  Edit").clicked() {
                                                                edit = Some(c.clone());
                                                                ui.close();
                                                            }
                                                            if ui.button("🔄  Check again").clicked() {
                                                                retry = Some(c.clone());
                                                                ui.close();
                                                            }
                                                            if ui
                                                                .button(RichText::new("🗑  Delete").color(p.error))
                                                                .clicked()
                                                            {
                                                                forget = Some(c.clone());
                                                                ui.close();
                                                            }
                                                        },
                                                    );
                                                    match state {
                                                        Some(ProbeState::Err(_)) => {
                                                            if ui
                                                                .add(
                                                                    egui::Button::new(
                                                                        RichText::new("Retry").color(p.error),
                                                                    )
                                                                    .fill(p.error.gamma_multiply(0.12))
                                                                    .stroke(Stroke::new(
                                                                        1.0,
                                                                        p.error.gamma_multiply(0.5),
                                                                    )),
                                                                )
                                                                .on_hover_text("Check this server again")
                                                                .clicked()
                                                            {
                                                                retry = Some(c.clone());
                                                            }
                                                        }
                                                        _ => {
                                                            let label = if is_active { "Go to keys" } else { "Open" };
                                                            if ghost_button(ui, p, label).clicked() {
                                                                open = Some(c.clone());
                                                            }
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    });
                            }
                        });
                });

            if let Some(col) = sort_click {
                if self.dash.sort == col {
                    self.dash.asc = !self.dash.asc;
                } else {
                    self.dash.sort = col;
                    self.dash.asc = col == SortCol::Name;
                }
            }
            if let Some(c) = open {
                if self.connected_id() == Some(c.id) {
                    self.tab = Tab::Keys;
                } else {
                    self.connect(c);
                }
            }
            if let Some(c) = retry {
                self.probe_one(c, true);
            }
            if let Some(c) = edit {
                self.editor = Some(ConnEditor {
                    port_text: c.port.to_string(),
                    timeout_text: c.timeout_secs.to_string(),
                    cfg: c,
                    is_new: false,
                    test: TestState::None,
                    show_pw: false,
                });
            }
            if let Some(c) = forget {
                self.confirm = Some(Confirm {
                    title: "Delete connection?".into(),
                    body: format!(
                        "\"{}\" will be removed from your saved connections. Nothing on the server changes.",
                        c.display_name()
                    ),
                    action: "Delete".into(),
                    job: None,
                    forget_conn: Some(c.id),
                });
            }
        });
    }

    // ---------- welcome ----------

    fn welcome(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let connecting = matches!(self.conn, ConnState::Connecting(_));
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.5 - 200.0).max(30.0));
            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 120.0), Sense::hover());
            ui.painter().image(
                self.logo.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.add_space(14.0);
            ui.label(RichText::new("Welcome to Cache Deck").size(26.0).strong());
            ui.add_space(4.0);
            ui.label(
                RichText::new("Connect to a Redis or Valkey server to browse keys, edit values and run commands.")
                    .size(14.5)
                    .color(p.muted),
            );
            ui.add_space(20.0);
            if connecting {
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 60.0);
                    ui.spinner();
                    ui.label(RichText::new("Connecting…").color(p.muted));
                });
            } else {
                if primary_button(ui, p, "➕  New connection").clicked() {
                    self.open_new_conn();
                }
                ui.add_space(10.0);
                let hint = if self.cfg.connections.is_empty() {
                    "Tip: a server on this computer usually runs at 127.0.0.1, port 6379."
                } else {
                    "Or pick one of your saved connections on the left."
                };
                ui.label(RichText::new(hint).size(12.5).color(p.muted));
            }
            ui.add_space(36.0);
            ui.horizontal(|ui| {
                let w = 190.0;
                let total = w * 3.0 + 24.0;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                for (icon, title, body) in [
                    ("🔑", "Browse keys", "Search, filter by type and see expiry times."),
                    ("✏", "Edit values", "Strings, lists, sets, sorted sets and hashes."),
                    ("⌨", "Run commands", "Type commands with history and suggestions."),
                ] {
                    card_frame(p).show(ui, |ui| {
                        ui.set_width(w - 34.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(icon).size(18.0).color(p.brand));
                            ui.label(RichText::new(title).strong());
                            ui.add(egui::Label::new(RichText::new(body).size(12.0).color(p.muted)).wrap());
                        });
                    });
                }
            });
        });
    }

    // ---------- keys ----------

    fn key_list(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(10.0);
        let mut do_scan = false;
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 30.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                if ghost_button(ui, p, "🔄").on_hover_text("Refresh the list").clicked() {
                    do_scan = true;
                }
                let resp = ui.add(
                    TextEdit::singleline(&mut self.keys.pattern)
                        .hint_text("🔍  Search keys (e.g. user:*)")
                        .desired_width(ui.available_width()),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    do_scan = true;
                }
            },
        );
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 30.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                if primary_button(ui, p, "➕ Key").on_hover_text("Add a new key").clicked() {
                    self.new_key = Some(NewKey {
                        name: String::new(),
                        kind: "string",
                        a: String::new(),
                        b: String::new(),
                        ttl: String::new(),
                    });
                }
                let before = self.keys.kind;
                let current = KIND_FILTERS
                    .iter()
                    .find(|(k, _)| *k == self.keys.kind)
                    .map(|(_, l)| *l)
                    .unwrap_or("All types");
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("kind-filter")
                        .selected_text(current)
                        .width(ui.available_width() - 8.0)
                        .show_ui(ui, |ui| {
                            for (k, label) in KIND_FILTERS {
                                ui.selectable_value(&mut self.keys.kind, k, label);
                            }
                        });
                });
                if before != self.keys.kind {
                    do_scan = true;
                }
            },
        );
        if do_scan {
            self.scan(true);
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let n = self.keys.rows.len();
            let text = if self.keys.loading && n == 0 {
                "Loading keys…".to_string()
            } else if self.keys.done {
                format!("{} key{}", thousands(n as i64), if n == 1 { "" } else { "s" })
            } else {
                format!("{} keys loaded · more available", thousands(n as i64))
            };
            ui.label(RichText::new(text).size(11.5).color(p.muted));
            if self.keys.loading {
                ui.spinner();
            }
        });
        ui.add_space(2.0);

        let row_h = 32.0;
        let rows = self.keys.rows.clone();
        let mut clicked: Option<Bytes> = None;
        let load_more = !self.keys.done;
        let avail = ui.available_height() - if load_more { 40.0 } else { 6.0 };
        egui::ScrollArea::vertical()
            .id_salt("key-rows")
            .max_height(avail)
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for r in &rows[range] {
                    let selected = self.keys.selected.as_ref() == Some(&r.key);
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                    let painter = ui.painter_at(rect);
                    if selected {
                        painter.rect_filled(
                            rect.shrink2(vec2(0.0, 1.0)),
                            CornerRadius::same(7),
                            p.brand.gamma_multiply(0.15),
                        );
                        painter.rect_stroke(
                            rect.shrink2(vec2(0.0, 1.0)),
                            CornerRadius::same(7),
                            Stroke::new(1.0, p.brand.gamma_multiply(0.55)),
                            StrokeKind::Inside,
                        );
                    } else if resp.hovered() {
                        painter.rect_filled(rect.shrink2(vec2(0.0, 1.0)), CornerRadius::same(7), p.card);
                    }
                    let color = type_color(&r.kind, p);
                    let tag = egui::Rect::from_min_size(rect.left_center() + vec2(6.0, -9.0), vec2(52.0, 18.0));
                    painter.rect_filled(
                        tag,
                        CornerRadius::same(5),
                        color.gamma_multiply(if p.dark { 0.18 } else { 0.16 }),
                    );
                    painter.text(
                        tag.center(),
                        Align2::CENTER_CENTER,
                        type_label(&r.kind),
                        FontId::proportional(10.0),
                        color,
                    );
                    let ttl = short_ttl(r.ttl_ms);
                    let ttl_color = if r.ttl_ms >= 0 {
                        p.accent
                    } else {
                        p.muted.gamma_multiply(0.7)
                    };
                    painter.text(
                        rect.right_center() - vec2(8.0, 0.0),
                        Align2::RIGHT_CENTER,
                        &ttl,
                        FontId::proportional(11.5),
                        ttl_color,
                    );
                    let name_left = tag.right() + 9.0;
                    let max_w = rect.right() - 44.0 - name_left;
                    let galley = ui.fonts_mut(|f| f.layout_no_wrap(r.name.clone(), FontId::proportional(13.5), p.text));
                    let name_rect = egui::Rect::from_min_max(
                        egui::pos2(name_left, rect.top()),
                        egui::pos2(name_left + max_w, rect.bottom()),
                    );
                    let painter2 = ui.painter_at(name_rect);
                    painter2.galley(
                        egui::pos2(name_left, rect.center().y - galley.size().y / 2.0),
                        galley.clone(),
                        p.text,
                    );
                    if galley.size().x > max_w {
                        let fade = egui::Rect::from_min_max(
                            egui::pos2(name_rect.right() - 18.0, rect.top() + 2.0),
                            egui::pos2(name_rect.right(), rect.bottom() - 2.0),
                        );
                        let bg = if selected {
                            p.panel
                        } else if resp.hovered() {
                            p.card
                        } else {
                            p.panel
                        };
                        painter2.rect_filled(fade, CornerRadius::ZERO, bg.gamma_multiply(0.85));
                        painter2.text(
                            fade.right_center(),
                            Align2::RIGHT_CENTER,
                            "…",
                            FontId::proportional(13.0),
                            p.muted,
                        );
                    }
                    let resp = resp.on_hover_text(&r.name);
                    if resp.clicked() {
                        clicked = Some(r.key.clone());
                    }
                }
            });
        if rows.is_empty() && !self.keys.loading {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No keys found").strong());
                ui.label(
                    RichText::new("Try another search, or add a key.")
                        .size(12.0)
                        .color(p.muted),
                );
            });
        }
        if load_more {
            ui.add_space(4.0);
            let w = ui.available_width();
            if ui
                .add_enabled(
                    !self.keys.loading,
                    egui::Button::new("Load more").min_size(vec2(w, 30.0)),
                )
                .clicked()
            {
                self.scan(false);
            }
        }
        if let Some(k) = clicked {
            self.select_key(k);
        }
    }

    fn key_detail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let Some(d) = self.keys.detail.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                if self.keys.loading_key {
                    ui.spinner();
                } else {
                    ui.label(RichText::new("🔑").size(36.0).color(p.muted.gamma_multiply(0.6)));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Pick a key to see its value").size(16.0).strong());
                    ui.label(RichText::new("Values open here, ready to edit.").color(p.muted));
                }
            });
            return;
        };

        // Header
        ui.horizontal(|ui| {
            type_tag(ui, p, &d.kind, 62.0);
            if let Some(new_name) = &mut self.keys.renaming {
                let r = ui.add(
                    TextEdit::singleline(new_name)
                        .desired_width(ui.available_width() - 180.0)
                        .font(FontId::proportional(16.0)),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let new_name = new_name.clone();
                if primary_button(ui, p, "Rename").clicked() || enter {
                    if !new_name.is_empty() && new_name != d.name {
                        self.worker.send(Job::Ops {
                            cmds: vec![vec![b("RENAME"), d.key.clone(), b(&new_name)]],
                            must_not_exist: Some(b(&new_name)),
                            after: After::KeyRenamed(b(&new_name)),
                            ok: format!("Renamed to \"{new_name}\""),
                        });
                    }
                    self.keys.renaming = None;
                }
                if ghost_button(ui, p, "Cancel").clicked() {
                    self.keys.renaming = None;
                }
            } else {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if danger_button(ui, p, "🗑  Delete").clicked() {
                        self.confirm = Some(Confirm {
                            title: "Delete key?".into(),
                            body: format!(
                                "\"{}\" and its value will be removed from the server. This can't be undone.",
                                d.name
                            ),
                            action: "Delete key".into(),
                            job: Some(Job::Ops {
                                cmds: vec![vec![b("DEL"), d.key.clone()]],
                                must_not_exist: None,
                                after: After::KeyDeleted(d.key.clone()),
                                ok: format!("Deleted \"{}\"", d.name),
                            }),
                            forget_conn: None,
                        });
                    }
                    if ghost_button(ui, p, "Rename").clicked() {
                        self.keys.renaming = Some(d.name.clone());
                    }
                    if ghost_button(ui, p, "🔄").on_hover_text("Reload").clicked() {
                        self.select_key(d.key.clone());
                    }
                    if ghost_button(ui, p, "📋").on_hover_text("Copy key name").clicked() {
                        ui.ctx().copy_text(d.name.clone());
                        self.toast("Key name copied", false);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&d.name).size(18.0).strong()).truncate())
                            .on_hover_text(&d.name);
                    });
                });
            }
        });
        ui.add_space(8.0);

        // Facts + expiry
        card_frame(p).inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let fact = |ui: &mut egui::Ui, label: &str, value: String, color: Color32| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(label).size(11.0).color(p.muted));
                        ui.label(RichText::new(value).strong().color(color));
                    });
                    ui.add_space(18.0);
                };
                let ttl_color = if d.ttl_ms >= 0 { p.accent } else { p.text };
                fact(ui, "EXPIRES IN", human_ttl(d.ttl_ms), ttl_color);
                let size = match &d.value {
                    ValueData::Str(_) => format!("{} chars", thousands(d.len)),
                    ValueData::Text(_) | ValueData::Unsupported => {
                        if d.len > 0 {
                            format!("{} entr{}", thousands(d.len), if d.len == 1 { "y" } else { "ies" })
                        } else {
                            "—".into()
                        }
                    }
                    _ => format!("{} item{}", thousands(d.len), if d.len == 1 { "" } else { "s" }),
                };
                fact(ui, "SIZE", size, p.text);
                fact(
                    ui,
                    "MEMORY",
                    d.memory.map(human_bytes).unwrap_or_else(|| "—".into()),
                    p.text,
                );
                ui.separator();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new("SET EXPIRY (SECONDS)").size(11.0).color(p.muted));
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut self.keys.ttl_buf)
                                .hint_text("e.g. 3600")
                                .desired_width(90.0),
                        );
                        let secs = self.keys.ttl_buf.trim().parse::<i64>().ok().filter(|s| *s > 0);
                        if ui.add_enabled(secs.is_some(), egui::Button::new("Set")).clicked()
                            && let Some(s) = secs
                        {
                            self.ops(
                                vec![vec![b("EXPIRE"), d.key.clone(), b(&s.to_string())]],
                                After::ReloadKey(d.key.clone()),
                                "Expiry updated",
                            );
                        }
                        if ui
                            .add_enabled(d.ttl_ms >= 0, egui::Button::new("Remove"))
                            .on_hover_text("Keep this key forever")
                            .clicked()
                        {
                            self.ops(
                                vec![vec![b("PERSIST"), d.key.clone()]],
                                After::ReloadKey(d.key.clone()),
                                "Expiry removed",
                            );
                        }
                    });
                });
            });
        });
        ui.add_space(10.0);

        if d.len > ITEM_LIMIT as i64 && !matches!(d.value, ValueData::Str(_) | ValueData::Text(_)) {
            ui.label(
                RichText::new(format!("Showing the first {ITEM_LIMIT} of {} items.", thousands(d.len)))
                    .size(12.0)
                    .color(p.accent),
            );
        }

        match &d.value {
            ValueData::Str(_) => self.string_editor(ui, p, &d),
            ValueData::Hash(items) => self.hash_editor(ui, p, &d, items),
            ValueData::List(items) => self.list_editor(ui, p, &d, items),
            ValueData::Set(items) => self.set_editor(ui, p, &d, items),
            ValueData::ZSet(items) => self.zset_editor(ui, p, &d, items),
            ValueData::Text(t) => {
                ui.label(RichText::new("Read-only preview").size(12.0).color(p.muted));
                egui::ScrollArea::both()
                    .id_salt("text-view")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut t = t.clone();
                        ui.add(
                            TextEdit::multiline(&mut t)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .interactive(true),
                        );
                    });
            }
            ValueData::Unsupported => {
                ui.label(
                    RichText::new(format!(
                        "Cache Deck can't show \"{}\" values yet. Use the Commands tab to work with this key.",
                        d.kind
                    ))
                    .color(p.muted),
                );
            }
        }
    }

    fn string_editor(&mut self, ui: &mut egui::Ui, p: &Palette, d: &KeyDetail) {
        let changed = self.keys.str_buf != self.keys.str_orig;
        let json = serde_json::from_str::<serde_json::Value>(&self.keys.str_buf)
            .ok()
            .filter(|v| v.is_object() || v.is_array());
        ui.horizontal(|ui| {
            section_label(ui, p, "VALUE");
            if json.is_some() {
                Frame::new()
                    .fill(type_color("json", p).gamma_multiply(0.16))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(RichText::new("JSON").size(10.5).color(type_color("json", p)))
                    });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        changed,
                        egui::Button::new(
                            RichText::new("Save")
                                .color(if changed { p.on_brand } else { p.muted })
                                .strong(),
                        )
                        .fill(if changed { p.brand } else { p.card }),
                    )
                    .clicked()
                {
                    // KEEPTTL keeps the key's expiry as it was.
                    self.ops(
                        vec![vec![
                            b("SET"),
                            d.key.clone(),
                            self.keys.str_buf.clone().into_bytes(),
                            b("KEEPTTL"),
                        ]],
                        After::ReloadKey(d.key.clone()),
                        "Value saved",
                    );
                }
                if ui.add_enabled(changed, egui::Button::new("Undo changes")).clicked() {
                    self.keys.str_buf = self.keys.str_orig.clone();
                }
                if let Some(j) = &json
                    && ui.button("Format JSON").clicked()
                    && let Ok(s) = serde_json::to_string_pretty(j)
                {
                    self.keys.str_buf = s;
                }
                if changed {
                    ui.label(RichText::new("Unsaved changes").size(12.0).color(p.accent));
                }
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("str-edit")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut self.keys.str_buf)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(14)
                        .min_size(vec2(0.0, ui.available_height())),
                );
            });
    }

    fn filter_box(&mut self, ui: &mut egui::Ui, hint: &str) {
        ui.add(
            TextEdit::singleline(&mut self.keys.filter)
                .hint_text(format!("🔍  {hint}"))
                .desired_width(220.0),
        );
    }

    fn add_row_ui(&mut self, ui: &mut egui::Ui, p: &Palette, a_hint: &str, b_hint: Option<&str>, button: &str) -> bool {
        let mut go = false;
        card_frame(p).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let w = if b_hint.is_some() {
                    (ui.available_width() - 130.0) / 2.0
                } else {
                    ui.available_width() - 122.0
                };
                let r1 = ui.add(
                    TextEdit::singleline(&mut self.keys.add_a)
                        .hint_text(a_hint)
                        .desired_width(w),
                );
                let mut enter = r1.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if let Some(h) = b_hint {
                    let r2 = ui.add(TextEdit::singleline(&mut self.keys.add_b).hint_text(h).desired_width(w));
                    enter |= r2.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                }
                if primary_button(ui, p, button).clicked() || enter {
                    go = true;
                }
            });
        });
        go
    }

    fn table_header(ui: &mut egui::Ui, p: &Palette, cols: &[&str]) {
        for c in cols {
            ui.label(RichText::new(*c).size(11.0).color(p.muted).strong());
        }
        ui.end_row();
    }

    fn hash_editor(&mut self, ui: &mut egui::Ui, p: &Palette, d: &KeyDetail, items: &[(String, String)]) {
        ui.horizontal(|ui| {
            section_label(ui, p, "FIELDS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.filter_box(ui, "Filter fields")
            });
        });
        let f = self.keys.filter.to_lowercase();
        let mut action: Option<(Vec<Vec<Bytes>>, &str)> = None;
        let avail = ui.available_height() - 60.0;
        let total = ui.available_width() - 20.0;
        let fw = (total * 0.32).clamp(110.0, 280.0);
        let vw = (total - fw - 64.0 - 28.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("hash-rows")
            .max_height(avail)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("hash-grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing(vec2(14.0, 6.0))
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        Self::table_header(ui, p, &["FIELD", "VALUE", ""]);
                        for (i, (field, value)) in items.iter().enumerate() {
                            if !f.is_empty() && !field.to_lowercase().contains(&f) && !value.to_lowercase().contains(&f)
                            {
                                continue;
                            }
                            cell(ui, fw, field, RichText::new(field).monospace().color(p.brand));
                            let editing = self.keys.edit.as_ref().map(|e| e.idx == i).unwrap_or(false);
                            if editing {
                                let e = self.keys.edit.as_mut().unwrap();
                                ui.add(TextEdit::singleline(&mut e.b).desired_width(vw));
                                ui.horizontal(|ui| {
                                    if primary_button(ui, p, "Save").clicked() {
                                        let e = self.keys.edit.take().unwrap();
                                        action = Some((
                                            vec![vec![b("HSET"), d.key.clone(), b(field), e.b.into_bytes()]],
                                            "Field saved",
                                        ));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        self.keys.edit = None;
                                    }
                                });
                            } else {
                                cell(ui, vw, value, RichText::new(value).monospace());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    if small_icon_button(ui, p, "✏", p.muted)
                                        .on_hover_text("Edit value")
                                        .clicked()
                                    {
                                        self.keys.edit = Some(ItemEdit {
                                            idx: i,
                                            b: value.clone(),
                                        });
                                    }
                                    if small_icon_button(ui, p, "🗑", p.error)
                                        .on_hover_text("Remove field")
                                        .clicked()
                                    {
                                        action =
                                            Some((vec![vec![b("HDEL"), d.key.clone(), b(field)]], "Field removed"));
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        if self.add_row_ui(ui, p, "New field", Some("Value"), "Add field") {
            if self.keys.add_a.is_empty() {
                self.toast("Enter a field name", true);
            } else {
                let cmd = vec![b("HSET"), d.key.clone(), b(&self.keys.add_a), b(&self.keys.add_b)];
                self.keys.add_a.clear();
                self.keys.add_b.clear();
                action = Some((vec![cmd], "Field added"));
            }
        }
        if let Some((cmds, ok)) = action {
            self.ops(cmds, After::ReloadKey(d.key.clone()), ok);
        }
    }

    fn list_editor(&mut self, ui: &mut egui::Ui, p: &Palette, d: &KeyDetail, items: &[String]) {
        ui.horizontal(|ui| {
            section_label(ui, p, "ITEMS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.filter_box(ui, "Filter items")
            });
        });
        let f = self.keys.filter.to_lowercase();
        let mut action: Option<(Vec<Vec<Bytes>>, &str)> = None;
        let avail = ui.available_height() - 60.0;
        let vw = (ui.available_width() - 20.0 - 44.0 - 64.0 - 28.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("list-rows")
            .max_height(avail)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("list-grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing(vec2(14.0, 6.0))
                    .show(ui, |ui| {
                        Self::table_header(ui, p, &["#", "VALUE", ""]);
                        for (i, value) in items.iter().enumerate() {
                            if !f.is_empty() && !value.to_lowercase().contains(&f) {
                                continue;
                            }
                            ui.label(RichText::new(i.to_string()).monospace().color(p.muted));
                            let editing = self.keys.edit.as_ref().map(|e| e.idx == i).unwrap_or(false);
                            if editing {
                                let e = self.keys.edit.as_mut().unwrap();
                                ui.add(TextEdit::singleline(&mut e.b).desired_width(vw));
                                ui.horizontal(|ui| {
                                    if primary_button(ui, p, "Save").clicked() {
                                        let e = self.keys.edit.take().unwrap();
                                        action = Some((
                                            vec![vec![b("LSET"), d.key.clone(), b(&i.to_string()), e.b.into_bytes()]],
                                            "Item saved",
                                        ));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        self.keys.edit = None;
                                    }
                                });
                            } else {
                                cell(ui, vw, value, RichText::new(value).monospace());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    if small_icon_button(ui, p, "✏", p.muted)
                                        .on_hover_text("Edit item")
                                        .clicked()
                                    {
                                        self.keys.edit = Some(ItemEdit {
                                            idx: i,
                                            b: value.clone(),
                                        });
                                    }
                                    if small_icon_button(ui, p, "🗑", p.error)
                                        .on_hover_text("Remove item")
                                        .clicked()
                                    {
                                        // Mark the exact position, then remove that mark.
                                        let tomb = b("__cache_deck_removed__");
                                        action = Some((
                                            vec![
                                                vec![b("LSET"), d.key.clone(), b(&i.to_string()), tomb.clone()],
                                                vec![b("LREM"), d.key.clone(), b("1"), tomb],
                                            ],
                                            "Item removed",
                                        ));
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        let mut to_start = false;
        let go = card_frame(p)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut go = false;
                ui.horizontal(|ui| {
                    let r = ui.add(
                        TextEdit::singleline(&mut self.keys.add_a)
                            .hint_text("New item")
                            .desired_width(ui.available_width() - 230.0),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = true;
                    }
                    if primary_button(ui, p, "Add to end").clicked() {
                        go = true;
                    }
                    if ghost_button(ui, p, "Add to start").clicked() {
                        go = true;
                        to_start = true;
                    }
                });
                go
            })
            .inner;
        if go {
            let cmd = vec![
                b(if to_start { "LPUSH" } else { "RPUSH" }),
                d.key.clone(),
                b(&self.keys.add_a),
            ];
            self.keys.add_a.clear();
            action = Some((vec![cmd], "Item added"));
        }
        if let Some((cmds, ok)) = action {
            self.ops(cmds, After::ReloadKey(d.key.clone()), ok);
        }
    }

    fn set_editor(&mut self, ui: &mut egui::Ui, p: &Palette, d: &KeyDetail, items: &[String]) {
        ui.horizontal(|ui| {
            section_label(ui, p, "MEMBERS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.filter_box(ui, "Filter members")
            });
        });
        let f = self.keys.filter.to_lowercase();
        let mut action: Option<(Vec<Vec<Bytes>>, &str)> = None;
        let avail = ui.available_height() - 60.0;
        let mw = (ui.available_width() - 20.0 - 32.0 - 14.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("set-rows")
            .max_height(avail)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("set-grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing(vec2(14.0, 6.0))
                    .show(ui, |ui| {
                        Self::table_header(ui, p, &["MEMBER", ""]);
                        for m in items {
                            if !f.is_empty() && !m.to_lowercase().contains(&f) {
                                continue;
                            }
                            cell(ui, mw, m, RichText::new(m).monospace());
                            if small_icon_button(ui, p, "🗑", p.error)
                                .on_hover_text("Remove member")
                                .clicked()
                            {
                                action = Some((vec![vec![b("SREM"), d.key.clone(), b(m)]], "Member removed"));
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        if self.add_row_ui(ui, p, "New member", None, "Add member") {
            let cmd = vec![b("SADD"), d.key.clone(), b(&self.keys.add_a)];
            self.keys.add_a.clear();
            action = Some((vec![cmd], "Member added"));
        }
        if let Some((cmds, ok)) = action {
            self.ops(cmds, After::ReloadKey(d.key.clone()), ok);
        }
    }

    fn zset_editor(&mut self, ui: &mut egui::Ui, p: &Palette, d: &KeyDetail, items: &[(String, f64)]) {
        ui.horizontal(|ui| {
            section_label(ui, p, "MEMBERS BY SCORE");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.filter_box(ui, "Filter members")
            });
        });
        let f = self.keys.filter.to_lowercase();
        let mut action: Option<(Vec<Vec<Bytes>>, &str)> = None;
        let avail = ui.available_height() - 60.0;
        let total = ui.available_width() - 20.0;
        let sw = 140.0;
        let mw = (total - sw - 64.0 - 28.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("zset-rows")
            .max_height(avail)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("zset-grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing(vec2(14.0, 6.0))
                    .show(ui, |ui| {
                        Self::table_header(ui, p, &["MEMBER", "SCORE", ""]);
                        for (i, (m, score)) in items.iter().enumerate() {
                            if !f.is_empty() && !m.to_lowercase().contains(&f) {
                                continue;
                            }
                            cell(ui, mw, m, RichText::new(m).monospace());
                            let editing = self.keys.edit.as_ref().map(|e| e.idx == i).unwrap_or(false);
                            if editing {
                                let e = self.keys.edit.as_mut().unwrap();
                                ui.add(TextEdit::singleline(&mut e.b).desired_width(sw));
                                ui.horizontal(|ui| {
                                    if primary_button(ui, p, "Save").clicked() {
                                        let e = self.keys.edit.take().unwrap();
                                        if e.b.trim().parse::<f64>().is_ok() {
                                            action = Some((
                                                vec![vec![b("ZADD"), d.key.clone(), b(e.b.trim()), b(m)]],
                                                "Score saved",
                                            ));
                                        } else {
                                            self.toast("Score must be a number", true);
                                        }
                                    }
                                    if ui.button("Cancel").clicked() {
                                        self.keys.edit = None;
                                    }
                                });
                            } else {
                                let sc = format_score(*score);
                                cell(ui, sw, &sc, RichText::new(&sc).monospace().color(type_color("zset", p)));
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    if small_icon_button(ui, p, "✏", p.muted)
                                        .on_hover_text("Change score")
                                        .clicked()
                                    {
                                        self.keys.edit = Some(ItemEdit {
                                            idx: i,
                                            b: format_score(*score),
                                        });
                                    }
                                    if small_icon_button(ui, p, "🗑", p.error)
                                        .on_hover_text("Remove member")
                                        .clicked()
                                    {
                                        action = Some((vec![vec![b("ZREM"), d.key.clone(), b(m)]], "Member removed"));
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        if self.add_row_ui(ui, p, "New member", Some("Score, e.g. 10"), "Add member") {
            match self.keys.add_b.trim().parse::<f64>() {
                Ok(_) => {
                    let cmd = vec![b("ZADD"), d.key.clone(), b(self.keys.add_b.trim()), b(&self.keys.add_a)];
                    self.keys.add_a.clear();
                    self.keys.add_b.clear();
                    action = Some((vec![cmd], "Member added"));
                }
                Err(_) => self.toast("Score must be a number", true),
            }
        }
        if let Some((cmds, ok)) = action {
            self.ops(cmds, After::ReloadKey(d.key.clone()), ok);
        }
    }

    // ---------- console ----------

    fn console_view(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Commands").size(20.0).strong());
            ui.label(
                RichText::new("Type a command and press Enter. ↑ / ↓ for history, Tab to complete.")
                    .size(12.5)
                    .color(p.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ghost_button(ui, p, "Clear").clicked() {
                    self.console.log.clear();
                }
            });
        });
        ui.add_space(8.0);

        let suggestions = self.suggestions();
        let bottom_h = 58.0 + if suggestions.is_empty() { 0.0 } else { 34.0 };
        let out_h = (ui.available_height() - bottom_h).max(80.0);
        Frame::new()
            .fill(p.bg)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("console-out")
                    .max_height(out_h - 26.0)
                    .min_scrolled_height(out_h - 26.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.console.log.is_empty() {
                            ui.label(
                                RichText::new("Results show up here. Try PING, INFO memory or SCAN 0.")
                                    .monospace()
                                    .color(p.muted),
                            );
                        }
                        for e in &self.console.log {
                            ui.label(
                                RichText::new(format!("› {}", e.cmd))
                                    .monospace()
                                    .color(p.brand)
                                    .strong(),
                            );
                            let color = if e.err { p.error } else { p.text };
                            ui.add(egui::Label::new(RichText::new(&e.out).monospace().color(color)).selectable(true));
                            ui.add_space(8.0);
                        }
                        if self.console.busy {
                            ui.spinner();
                        }
                    });
            });
        ui.add_space(8.0);

        let mut pick: Option<String> = None;
        if !suggestions.is_empty() {
            ui.horizontal(|ui| {
                for (i, (name, hint)) in suggestions.iter().enumerate() {
                    let text = if i == 0 && !hint.is_empty() {
                        format!("{name}  {hint}")
                    } else {
                        name.clone()
                    };
                    let r = ui.add(
                        egui::Button::new(RichText::new(text).monospace().size(12.0).color(if i == 0 {
                            p.brand
                        } else {
                            p.text
                        }))
                        .fill(if i == 0 { p.brand.gamma_multiply(0.12) } else { p.card })
                        .stroke(Stroke::new(1.0, p.border)),
                    );
                    if r.clicked() {
                        pick = Some(name.clone());
                    }
                }
            });
        }

        let mut run = false;
        Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.brand.gamma_multiply(0.6)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("›").monospace().size(18.0).color(p.brand).strong());
                    let id = Id::unique("console-input");
                    // Tab and arrows belong to this box, not focus moves.
                    let has_focus = ui.memory(|m| m.has_focus(id));
                    if has_focus {
                        let (up, down, tab) = ui.input_mut(|i| {
                            (
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                                i.consume_key(egui::Modifiers::NONE, Key::Tab),
                            )
                        });
                        if up && !self.console.history.is_empty() {
                            let pos = match self.console.hist_pos {
                                None => self.console.history.len() - 1,
                                Some(0) => 0,
                                Some(n) => n - 1,
                            };
                            self.console.hist_pos = Some(pos);
                            self.console.input = self.console.history[pos].clone();
                        }
                        if down {
                            match self.console.hist_pos {
                                Some(n) if n + 1 < self.console.history.len() => {
                                    self.console.hist_pos = Some(n + 1);
                                    self.console.input = self.console.history[n + 1].clone();
                                }
                                _ => {
                                    self.console.hist_pos = None;
                                    self.console.input.clear();
                                }
                            }
                        }
                        if tab && let Some((name, _)) = suggestions.first() {
                            pick = Some(name.clone());
                        }
                    }
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.console.input)
                            .id(id)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("e.g. SET greeting \"hello world\"")
                            .frame(Frame::NONE)
                            .desired_width(ui.available_width() - 70.0),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        run = true;
                        resp.request_focus();
                    }
                    if primary_button(ui, p, "Run").clicked() {
                        run = true;
                    }
                    if let Some(name) = &pick {
                        let rest = self
                            .console
                            .input
                            .trim_start()
                            .split_once(' ')
                            .map(|(_, r)| r.to_string())
                            .unwrap_or_default();
                        self.console.input = format!("{name} {rest}");
                        resp.request_focus();
                        if let Some(mut st) = TextEdit::load_state(ui.ctx(), id) {
                            let end = egui::text::CCursor::new(self.console.input.chars().count());
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            st.store(ui.ctx(), id);
                        }
                    }
                });
            });

        if run && !self.console.busy {
            let line = self.console.input.trim().to_string();
            if !line.is_empty() {
                match crate::worker::split_args(&line) {
                    Ok(args) => {
                        self.console.busy = true;
                        if self.console.history.last() != Some(&line) {
                            self.console.history.push(line.clone());
                        }
                        self.console.hist_pos = None;
                        self.console.input.clear();
                        self.worker.send(Job::Console(args));
                    }
                    Err(e) => self.console.log.push(ConsoleEntry {
                        cmd: line,
                        out: e,
                        err: true,
                    }),
                }
            }
        }
    }

    /// Commands matching the first word typed, with their expected arguments.
    fn suggestions(&self) -> Vec<(String, String)> {
        let input = self.console.input.trim_start();
        if input.is_empty() || input.contains(' ') {
            return Vec::new();
        }
        let up = input.to_uppercase();
        let mut out: Vec<(String, String)> = Vec::new();
        for (name, hint) in commands::BUILTIN {
            if name.starts_with(&up) && *name != up {
                out.push((name.to_string(), hint.to_string()));
            }
        }
        for name in &self.console.server_cmds {
            if name.starts_with(&up) && *name != up && !out.iter().any(|(n, _)| n == name) {
                out.push((name.clone(), commands::hint(name).unwrap_or("").to_string()));
            }
        }
        out.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(a.0.cmp(&b.0)));
        out.truncate(7);
        out
    }

    // ---------- server info ----------

    fn info_view(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let due = self
            .info
            .last_req
            .map(|t| t.elapsed() > Duration::from_secs(3))
            .unwrap_or(true);
        if due {
            self.request_info();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Server info").size(20.0).strong());
            ui.label(RichText::new("Refreshes every few seconds").size(12.5).color(p.muted));
            if self.info.pending {
                ui.spinner();
            }
        });
        ui.add_space(10.0);
        let Some(s) = self.info.snap.clone() else {
            ui.spinner();
            return;
        };
        let g = |k: &str| s.get(k).unwrap_or("—").to_string();
        let num = |k: &str| s.get(k).and_then(|v| v.parse::<f64>().ok());
        let (kind, version, mode) = match &self.conn {
            ConnState::Connected { meta, .. } => (meta.kind, meta.version.clone(), meta.mode.clone()),
            _ => ("", String::new(), String::new()),
        };
        let uptime = num("uptime_in_seconds")
            .map(|x| human_ttl((x * 1000.0) as i64))
            .unwrap_or("—".into());
        let hits = num("keyspace_hits").unwrap_or(0.0);
        let misses = num("keyspace_misses").unwrap_or(0.0);
        let ratio = if hits + misses > 0.0 {
            format!("{:.1}%", hits / (hits + misses) * 100.0)
        } else {
            "—".into()
        };
        let cpu = self.info.cpu.map(|c| format!("{c:.1}%")).unwrap_or_else(|| "…".into());
        let cards: Vec<(&str, &str, String, String, Color32)> = vec![
            (
                "🖥",
                "SERVER",
                format!("{kind} {version}"),
                format!("{mode} · up {uptime}"),
                p.brand,
            ),
            (
                "💾",
                "MEMORY USED",
                g("used_memory_human"),
                format!(
                    "peak {} · max {}",
                    g("used_memory_peak_human"),
                    if g("maxmemory_human") == "0B" {
                        "no limit".into()
                    } else {
                        g("maxmemory_human")
                    }
                ),
                p.info,
            ),
            (
                "⚙",
                "CPU",
                cpu,
                format!(
                    "{:.1}s system · {:.1}s user",
                    num("used_cpu_sys").unwrap_or(0.0),
                    num("used_cpu_user").unwrap_or(0.0)
                ),
                p.accent,
            ),
            (
                "⚡",
                "OPERATIONS / SEC",
                g("instantaneous_ops_per_sec"),
                format!(
                    "{} commands in total",
                    thousands(num("total_commands_processed").unwrap_or(0.0) as i64)
                ),
                type_color("stream", p),
            ),
            (
                "🎯",
                "HIT RATIO",
                ratio,
                format!("{} hits · {} misses", thousands(hits as i64), thousands(misses as i64)),
                p.success,
            ),
            (
                "👥",
                "CLIENTS",
                g("connected_clients"),
                format!("{} blocked", g("blocked_clients")),
                type_color("hash", p),
            ),
            (
                "🔑",
                "KEYS",
                thousands(keyspace_total(&s)),
                keyspace_summary(&s),
                type_color("zset", p),
            ),
        ];
        egui::ScrollArea::vertical()
            .id_salt("info-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let cols = ((ui.available_width() + 12.0) / 260.0).floor().clamp(1.0, 4.0) as usize;
                let w = (ui.available_width() - 12.0 * (cols as f32 - 1.0)) / cols as f32;
                for chunk in cards.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (icon, title, big, sub, color) in chunk {
                            card_frame(p).show(ui, |ui| {
                                ui.set_width(w - 34.0);
                                ui.set_height(78.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(*icon).size(14.0).color(*color));
                                        ui.label(RichText::new(*title).size(11.0).color(p.muted).strong());
                                    });
                                    ui.add(
                                        egui::Label::new(RichText::new(big).size(24.0).strong().color(p.text))
                                            .truncate(),
                                    );
                                    ui.add(egui::Label::new(RichText::new(sub).size(11.5).color(p.muted)).truncate());
                                });
                            });
                        }
                    });
                    ui.add_space(12.0);
                }
                ui.add_space(4.0);
                section_label(ui, p, "ALL DETAILS");
                ui.add_space(4.0);
                for (name, kv) in &s.sections {
                    egui::CollapsingHeader::new(RichText::new(name).strong())
                        .id_salt(("info-sec", name))
                        .show(ui, |ui| {
                            egui::Grid::new(("info-grid", name))
                                .num_columns(2)
                                .striped(true)
                                .spacing(vec2(24.0, 4.0))
                                .show(ui, |ui| {
                                    for (k, v) in kv {
                                        ui.label(RichText::new(k).monospace().color(p.muted));
                                        ui.label(RichText::new(v).monospace());
                                        ui.end_row();
                                    }
                                });
                        });
                }
            });
    }

    // ---------- dialogs ----------

    fn about_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.about {
            return;
        }
        let mut close = false;
        let resp = egui::Modal::new(Id::unique("about"))
            .frame(modal_frame(ctx, p))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
                    ui.painter().image(
                        self.logo.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    ui.add_space(10.0);
                    ui.label(RichText::new("Cache Deck").size(24.0).strong());
                    ui.label(RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION"))).color(p.muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("A desktop client for Redis and Valkey").color(p.text));
                });
                ui.add_space(16.0);
                card_frame(p).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section_label(ui, p, "DEVELOPER");
                    ui.add_space(2.0);
                    egui::Grid::new("about-dev")
                        .num_columns(2)
                        .spacing(vec2(16.0, 8.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").color(p.muted));
                            ui.label(RichText::new(DEVELOPER).strong());
                            ui.end_row();
                            ui.label(RichText::new("GitHub").color(p.muted));
                            ui.hyperlink_to(RichText::new("github.com/ShaonDey").color(p.brand), GITHUB_URL)
                                .on_hover_text(GITHUB_URL);
                            ui.end_row();
                        });
                });
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("© 2026 Shaon Dey").size(12.0).color(p.muted));
                    ui.add_space(10.0);
                    if primary_button(ui, p, "Close").clicked() {
                        close = true;
                    }
                });
            });
        if resp.should_close() || close {
            self.about = false;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, p: &Palette) {
        self.about_dialog(ctx, p);
        // Connection editor
        let mut close_editor = false;
        let mut save: Option<(ConnCfg, bool)> = None;
        let mut test: Option<ConnCfg> = None;
        if let Some(ed) = &mut self.editor {
            let resp = egui::Modal::new(Id::unique("conn-editor"))
                .frame(modal_frame(ctx, p))
                .show(ctx, |ui| {
                    ui.set_width(500.0);
                    ui.label(
                        RichText::new(if ed.is_new { "New connection" } else { "Edit connection" })
                            .size(20.0)
                            .strong(),
                    );
                    ui.label(RichText::new("Where is your Redis or Valkey server?").color(p.muted));
                    ui.add_space(12.0);
                    egui::Grid::new("conn-form")
                        .num_columns(2)
                        .spacing(vec2(14.0, 10.0))
                        .show(ui, |ui| {
                            let w = 300.0;
                            ui.label("Name");
                            ui.add(
                                TextEdit::singleline(&mut ed.cfg.name)
                                    .hint_text("e.g. Local cache")
                                    .desired_width(w),
                            );
                            ui.end_row();
                            ui.label("Host");
                            ui.add(
                                TextEdit::singleline(&mut ed.cfg.host)
                                    .hint_text("127.0.0.1")
                                    .desired_width(w),
                            );
                            ui.end_row();
                            ui.label("Port");
                            number_input(ui, p, &mut ed.port_text, "port-input", PORT_MAX);
                            ui.end_row();
                            ui.label("Username");
                            ui.add(
                                TextEdit::singleline(&mut ed.cfg.username)
                                    .hint_text("optional")
                                    .desired_width(w),
                            );
                            ui.end_row();
                            ui.label("Password");
                            ui.horizontal(|ui| {
                                ui.add(
                                    TextEdit::singleline(&mut ed.cfg.password)
                                        .password(!ed.show_pw)
                                        .hint_text("optional")
                                        .desired_width(w - 60.0),
                                );
                                if ui.button(if ed.show_pw { "Hide" } else { "Show" }).clicked() {
                                    ed.show_pw = !ed.show_pw;
                                }
                            });
                            ui.end_row();
                            ui.label("Database");
                            ui.add(egui::DragValue::new(&mut ed.cfg.db).range(0..=255));
                            ui.end_row();
                            ui.label("Timeout (s)")
                                .on_hover_text("How long to wait for the server when connecting");
                            number_input(ui, p, &mut ed.timeout_text, "timeout-input", TIMEOUT_MAX);
                            ui.end_row();
                            ui.label("Server type");
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("kind-pick")
                                    .selected_text(ed.cfg.kind.label())
                                    .width(130.0)
                                    .show_ui(ui, |ui| {
                                        for k in [ServerKind::Auto, ServerKind::Redis, ServerKind::Valkey] {
                                            ui.selectable_value(&mut ed.cfg.kind, k, k.label());
                                        }
                                    });
                                ui.add_enabled(
                                    ed.cfg.kind != ServerKind::Auto,
                                    TextEdit::singleline(&mut ed.cfg.version)
                                        .hint_text("version (optional)")
                                        .desired_width(150.0),
                                );
                            });
                            ui.end_row();
                            ui.label("Environment");
                            ui.horizontal(|ui| {
                                let mut choice = ed.cfg.env;
                                for (val, label) in [
                                    (None, "None"),
                                    (Some(Env::Production), "Production"),
                                    (Some(Env::Staging), "Staging"),
                                    (Some(Env::Development), "Dev"),
                                    (Some(Env::Local), "Local"),
                                ] {
                                    let on = choice == val;
                                    let color = val.map(|e| env_color(e, p)).unwrap_or(p.muted);
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new(label).size(12.0).color(if on {
                                                color
                                            } else {
                                                p.muted
                                            }))
                                            .fill(if on {
                                                color.gamma_multiply(0.16)
                                            } else {
                                                Color32::TRANSPARENT
                                            })
                                            .stroke(Stroke::new(1.0, if on { color } else { p.border })),
                                        )
                                        .clicked()
                                    {
                                        choice = val;
                                    }
                                }
                                ed.cfg.env = choice;
                            });
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut ed.cfg.tls, "Use a secure connection (TLS)");
                            ui.end_row();
                            if ed.cfg.tls {
                                ui.label("");
                                ui.checkbox(&mut ed.cfg.tls_skip_host_check, "Skip the certificate host name check")
                                    .on_hover_text(TLS_SKIP_HINT);
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                    match &ed.test {
                        TestState::None => {}
                        TestState::Running => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Trying to connect…").color(p.muted));
                            });
                        }
                        TestState::Ok(s) => {
                            ui.label(RichText::new(format!("✔  {s}")).color(p.success));
                        }
                        TestState::Err(e) => {
                            ui.label(RichText::new(format!("✖  {e}")).color(p.error));
                        }
                    }
                    ui.add_space(10.0);
                    let port_ok = parse_number(&ed.port_text, PORT_MAX).map(|n| n as u16);
                    let timeout_ok = parse_number(&ed.timeout_text, TIMEOUT_MAX);
                    // The connection as typed, once both number boxes hold valid values.
                    let mut ready = None;
                    if let (Some(port), Some(timeout_secs)) = (port_ok, timeout_ok) {
                        let mut c = ed.cfg.clone();
                        c.port = port;
                        c.timeout_secs = timeout_secs;
                        ready = Some(c);
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(ready.is_some(), egui::Button::new("Test connection"))
                            .clicked()
                        {
                            ed.test = TestState::Running;
                            test = ready.clone();
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    ready.is_some(),
                                    egui::Button::new(RichText::new("Save & connect").color(p.on_brand).strong())
                                        .fill(p.brand),
                                )
                                .clicked()
                            {
                                save = ready.clone().map(|c| (c, true));
                            }
                            if ui.add_enabled(ready.is_some(), egui::Button::new("Save")).clicked() {
                                save = ready.clone().map(|c| (c, false));
                            }
                            if ui.button("Cancel").clicked() {
                                close_editor = true;
                            }
                        });
                    });
                    if port_ok.is_none() {
                        ui.label(
                            RichText::new("Port must be a number from 1 to 65535")
                                .size(12.0)
                                .color(p.error),
                        );
                    }
                    if timeout_ok.is_none() {
                        ui.label(
                            RichText::new(format!("Timeout must be a number from 1 to {TIMEOUT_MAX}"))
                                .size(12.0)
                                .color(p.error),
                        );
                    }
                });
            if resp.should_close() {
                close_editor = true;
            }
        }
        if let Some(c) = test {
            self.worker.test(c);
        }
        if let Some((mut c, then_connect)) = save {
            if c.id == 0 {
                c.id = self.cfg.alloc_id();
                self.cfg.connections.push(c.clone());
            } else if let Some(x) = self.cfg.connections.iter_mut().find(|x| x.id == c.id) {
                *x = c.clone();
            }
            self.save_cfg();
            self.editor = None;
            self.probe_one(c.clone(), true);
            if then_connect {
                self.connect(c);
            } else {
                self.toast("Connection saved", false);
            }
        } else if close_editor {
            self.editor = None;
        }

        // Confirm
        let mut confirmed = false;
        let mut cancel = false;
        if let Some(cf) = &self.confirm {
            let resp = egui::Modal::new(Id::unique("confirm"))
                .frame(modal_frame(ctx, p))
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(RichText::new(&cf.title).size(18.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new(&cf.body).color(p.muted));
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(&cf.action).color(Color32::WHITE).strong())
                                    .fill(p.error),
                            )
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if resp.should_close() {
                cancel = true;
            }
        }
        if confirmed {
            if let Some(mut cf) = self.confirm.take() {
                if let Some(job) = cf.job.take() {
                    self.worker.send(job);
                }
                if let Some(id) = cf.forget_conn {
                    if self.connected_id() == Some(id) || matches!(self.conn, ConnState::Connecting(x) if x == id) {
                        self.worker.send(Job::Disconnect);
                        self.conn = ConnState::Idle;
                    }
                    self.cfg.connections.retain(|c| c.id != id);
                    self.save_cfg();
                    self.toast("Connection deleted", false);
                }
            }
        } else if cancel {
            self.confirm = None;
        }

        // New key
        let mut create: Option<Job> = None;
        let mut close_new = false;
        if let Some(nk) = &mut self.new_key {
            let mut problem: Option<String> = None;
            let resp = egui::Modal::new(Id::unique("new-key"))
                .frame(modal_frame(ctx, p))
                .show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.label(RichText::new("Add a key").size(20.0).strong());
                    ui.add_space(10.0);
                    egui::Grid::new("new-key-form")
                        .num_columns(2)
                        .spacing(vec2(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.label("Type");
                            ui.horizontal(|ui| {
                                for (k, label) in NEW_KINDS {
                                    let on = nk.kind == k;
                                    let color = type_color(k, p);
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new(label).size(12.5).color(if on {
                                                color
                                            } else {
                                                p.muted
                                            }))
                                            .fill(if on { color.gamma_multiply(0.18) } else { p.card })
                                            .stroke(Stroke::new(1.0, if on { color } else { p.border })),
                                        )
                                        .clicked()
                                    {
                                        nk.kind = k;
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label("Key name");
                            ui.add(
                                TextEdit::singleline(&mut nk.name)
                                    .hint_text("e.g. user:42")
                                    .desired_width(300.0),
                            );
                            ui.end_row();
                            let (a, bh) = match nk.kind {
                                "string" => ("Value", None),
                                "hash" => ("First field", Some("Its value")),
                                "list" => ("First item", None),
                                "set" => ("First member", None),
                                _ => ("First member", Some("Its score")),
                            };
                            ui.label(a);
                            if nk.kind == "string" {
                                ui.add(TextEdit::multiline(&mut nk.a).desired_rows(3).desired_width(300.0));
                            } else {
                                ui.add(TextEdit::singleline(&mut nk.a).desired_width(300.0));
                            }
                            ui.end_row();
                            if let Some(h) = bh {
                                ui.label(h);
                                ui.add(TextEdit::singleline(&mut nk.b).desired_width(300.0));
                                ui.end_row();
                            }
                            ui.label("Expires after");
                            ui.horizontal(|ui| {
                                ui.add(TextEdit::singleline(&mut nk.ttl).hint_text("never").desired_width(90.0));
                                ui.label(RichText::new("seconds").color(p.muted));
                            });
                            ui.end_row();
                        });
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, p, "Add key").clicked() {
                            let key = b(&nk.name);
                            let ttl = nk.ttl.trim();
                            let ttl_val = if ttl.is_empty() {
                                None
                            } else {
                                ttl.parse::<i64>().ok().filter(|t| *t > 0)
                            };
                            if nk.name.is_empty() {
                                problem = Some("Enter a key name".into());
                            } else if !ttl.is_empty() && ttl_val.is_none() {
                                problem = Some("Expiry must be a whole number of seconds".into());
                            } else if nk.kind == "zset" && nk.b.trim().parse::<f64>().is_err() {
                                problem = Some("Score must be a number".into());
                            } else if nk.kind == "hash" && nk.a.is_empty() {
                                problem = Some("Enter a field name".into());
                            } else {
                                let first = match nk.kind {
                                    "string" => vec![b("SET"), key.clone(), b(&nk.a)],
                                    "hash" => vec![b("HSET"), key.clone(), b(&nk.a), b(&nk.b)],
                                    "list" => vec![b("RPUSH"), key.clone(), b(&nk.a)],
                                    "set" => vec![b("SADD"), key.clone(), b(&nk.a)],
                                    _ => vec![b("ZADD"), key.clone(), b(nk.b.trim()), b(&nk.a)],
                                };
                                let mut cmds = vec![first];
                                if let Some(t) = ttl_val {
                                    cmds.push(vec![b("EXPIRE"), key.clone(), b(&t.to_string())]);
                                }
                                create = Some(Job::Ops {
                                    cmds,
                                    must_not_exist: Some(key.clone()),
                                    after: After::KeyCreated(key),
                                    ok: format!("Added \"{}\"", nk.name),
                                });
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close_new = true;
                        }
                    });
                });
            if resp.should_close() {
                close_new = true;
            }
            if let Some(e) = problem {
                self.toast(e, true);
            }
        }
        if let Some(job) = create {
            self.worker.send(job);
            self.new_key = None;
        } else if close_new {
            self.new_key = None;
        }
    }

    fn toasts(&mut self, ctx: &egui::Context, p: &Palette) {
        self.toasts
            .retain(|t| t.at.elapsed() < Duration::from_secs(if t.err { 7 } else { 3 }));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        egui::Area::new(Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for t in &self.toasts {
                    let color = if t.err { p.error } else { p.success };
                    Frame::new()
                        .fill(p.panel)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.7)))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(14, 10))
                        .shadow(ctx.global_style().visuals.popup_shadow)
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if t.err { "✖" } else { "✔" }).color(color));
                                ui.add(egui::Label::new(RichText::new(&t.text)).wrap());
                            });
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        ctx.request_repaint();
        if let Some(res) = shot.result.lock().unwrap().take() {
            match res {
                Ok(()) => {
                    eprintln!("Saved screenshot to {}", shot.path.display());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => {
                    eprintln!("Screenshot failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        if shot.frames >= 120 && !shot.requested {
            shot.requested = true;
            let path = shot.path.clone();
            let slot = shot.result.clone();
            let ctx2 = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(
                move |img| {
                    let r = save_png(&path, &img);
                    *slot.lock().unwrap() = Some(r);
                    ctx2.request_repaint();
                },
            )));
        }
        if shot.frames > 1200 {
            eprintln!("Screenshot never arrived");
            std::process::exit(1);
        }
    }
}

fn save_png(path: &std::path::Path, img: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = img.size;
    let mut raw = Vec::with_capacity(w * h * 4);
    for c in &img.pixels {
        raw.extend_from_slice(&c.to_srgba_unmultiplied());
    }
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, raw).ok_or("bad image size")?;
    buf.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

fn format_score(s: f64) -> String {
    if s.fract() == 0.0 && s.abs() < 1e15 {
        format!("{}", s as i64)
    } else {
        format!("{s}")
    }
}

fn keyspace_total(s: &InfoSnapshot) -> i64 {
    s.sections
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("keyspace"))
        .map(|(_, kv)| {
            kv.iter()
                .filter_map(|(_, v)| {
                    v.split(',')
                        .find_map(|p| p.strip_prefix("keys="))
                        .and_then(|n| n.parse::<i64>().ok())
                })
                .sum()
        })
        .unwrap_or(0)
}

fn keyspace_summary(s: &InfoSnapshot) -> String {
    let dbs = s
        .sections
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("keyspace"))
        .map(|(_, kv)| kv.len())
        .unwrap_or(0);
    let expires: i64 = s
        .sections
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("keyspace"))
        .map(|(_, kv)| {
            kv.iter()
                .filter_map(|(_, v)| {
                    v.split(',')
                        .find_map(|p| p.strip_prefix("expires="))
                        .and_then(|n| n.parse::<i64>().ok())
                })
                .sum()
        })
        .unwrap_or(0);
    format!(
        "{} with expiry · {dbs} database{} in use",
        thousands(expires),
        if dbs == 1 { "" } else { "s" }
    )
}

impl eframe::App for CacheDeck {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(target_os = "macos")]
        if !self.native_about_ready {
            self.native_about_ready = crate::platform::macos::install();
        }
        self.poll();
        self.screenshot_step(&ctx);
        let p = palette(ui);

        egui::Panel::top("top-bar")
            .exact_size(54.0)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(16, 8)))
            .show(ui, |ui| self.top_bar(ui, &p));
        egui::Panel::bottom("status-bar")
            .exact_size(30.0)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(14, 4)))
            .show(ui, |ui| self.status_bar(ui, &p));
        egui::Panel::left("sidebar")
            .default_size(260.0)
            .size_range(220.0..=360.0)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::same(14)))
            .show(ui, |ui| self.sidebar(ui, &p));

        let connected = matches!(self.conn, ConnState::Connected { .. });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                if !connected || self.tab == Tab::Dashboard {
                    self.dashboard(ui, &p);
                    return;
                }
                match self.tab {
                    Tab::Dashboard => {}
                    Tab::Keys => {
                        egui::Panel::left("key-list")
                            .default_size(360.0)
                            .size_range(260.0..=620.0)
                            .frame(
                                Frame::new()
                                    .fill(p.panel.lerp_to_gamma(p.bg, 0.5))
                                    .inner_margin(Margin::symmetric(12, 0)),
                            )
                            .show(ui, |ui| self.key_list(ui, &p));
                        egui::CentralPanel::default()
                            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(20, 14)))
                            .show(ui, |ui| self.key_detail(ui, &p));
                    }
                    Tab::Console => {
                        Frame::new()
                            .inner_margin(Margin::symmetric(20, 0))
                            .show(ui, |ui| self.console_view(ui, &p));
                    }
                    Tab::Info => {
                        Frame::new()
                            .inner_margin(Margin::symmetric(20, 0))
                            .show(ui, |ui| self.info_view(ui, &p));
                    }
                }
            });

        self.dialogs(&ctx, &p);
        self.toasts(&ctx, &p);
    }
}
