use std::collections::BTreeMap;

use vtabs_core::ui::SettingsUi;
use vtabs_protocol::v2::{ModelMsg, SettingsField, SettingsGroup};
use vtabs_theme::Theme;

use crate::frame::{self, Cell, Style};
use crate::text;

/// spell themselves out.
pub const MIN_COLS: i64 = 48;
pub const WIDE_COLS: i64 = 90;

fn short_source(locked: &str) -> &str {
    match locked {
        "wezterm.lua" => "wezterm.lua",
        "wezterm.lua (host)" => "host",
        "not editable" => "read-only",
        other => other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub cols: i64,
    pub nav_x1: i64,
    pub nav_x2: i64,
    pub divider: i64,
    pub caret_x: i64,
    pub label_x: i64,
    pub label_x2: i64,
    pub form_x2: i64,
    pub marker_x: i64,
    pub value_x2: i64,
}

pub fn grid(cols: i64) -> Option<Grid> {
    if cols < MIN_COLS {
        return None;
    }
    let nav_x2 = 1 + 14;
    let divider = nav_x2 + 1;
    let caret_x = divider + 1;
    let label_x = caret_x + 2;
    let form_x2 = cols - 2;
    Some(Grid {
        cols,
        nav_x1: 2,
        nav_x2,
        divider,
        caret_x,
        label_x,
        label_x2: label_x + 22,
        form_x2,
        marker_x: form_x2,
        value_x2: form_x2 - 2,
    })
}

/// Rows the body has for nav entries and form rows: header, rule, blank … blank, rule, hints.
pub fn body_rows(rows: i64) -> i64 {
    (rows - 6).max(0)
}

fn matches(key: &str, filter: &str) -> bool {
    filter.is_empty() || key.to_lowercase().contains(&filter.to_lowercase())
}

/// One row the form is showing: a field, or one line of the armed recorder's caveat banner.
#[derive(Debug, Clone, Copy)]
pub enum Shown<'a> {
    Field(&'a SettingsField),
    Caveat(&'a str),
}

impl<'a> Shown<'a> {
    fn field(self) -> Option<&'a SettingsField> {
        match self {
            Shown::Field(f) => Some(f),
            Shown::Caveat(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Body<'a> {
    pub nav: Option<&'a SettingsGroup>,
    pub nav_selected: bool,
    pub caveat: Option<&'a str>,
    pub field: Option<&'a SettingsField>,
    pub focused: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Row<'a> {
    TooNarrow,
    Header,
    Rule,
    Space,
    Help(Option<&'a SettingsField>),
    Hints,
    Body(Body<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanId {
    Nav,
    Dec,
    Inc,
    Value,
    Field,
}

#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub id: SpanId,
    pub x1: i64,
    pub x2: i64,
}

/// One row a click can name: the nav entry it carries, the form row it carries, or both.
#[derive(Debug, Clone)]
pub struct Hit<'a> {
    pub nav: Option<&'a str>,
    pub field: Option<&'a SettingsField>,
    pub index: Option<i64>,
    pub spans: Vec<Span>,
}

impl Hit<'_> {
    /// `hit.span`: the first sub-target under `x`, so dec/inc win over the value column they sit in.
    pub fn span(&self, x: i64) -> Option<SpanId> {
        self.spans
            .iter()
            .find(|s| x >= s.x1 && x <= s.x2)
            .map(|s| s.id)
    }
}

/// Everything about a frame of the page that does not depend on colour.
pub struct Plan<'a> {
    pub grid: Option<Grid>,
    pub rows: Vec<Option<Row<'a>>>,
    pub hits: Vec<Option<Hit<'a>>>,
    pub groups: &'a [SettingsGroup],
    pub group: Option<&'a SettingsGroup>,
    pub shown: Vec<Shown<'a>>,
    pub focus: i64,
    pub scroll: i64,
}

impl<'a> Plan<'a> {
    /// `page.focused`. A caveat line is not a form row, so it answers for no key.
    pub fn focused(&self) -> Option<&'a SettingsField> {
        self.shown
            .get(usize::try_from(self.focus - 1).ok()?)
            .and_then(|s| s.field())
    }

    pub fn hit_at(&self, y: i64) -> Option<&Hit<'a>> {
        self.hits.get(usize::try_from(y - 1).ok()?)?.as_ref()
    }

    pub fn count(&self) -> i64 {
        self.shown.len() as i64
    }
}

/// The pane's own facts plus the settings model, as the widget reads them.
pub struct SettingsView<'a> {
    pub cols: i64,
    pub rows: i64,
    pub model: &'a ModelMsg,
    pub ui: &'a SettingsUi,
    pub theme: Theme,
    pub glyphs: BTreeMap<String, String>,
}

impl SettingsView<'_> {
    fn glyph(&self, key: &str) -> &str {
        self.glyphs.get(key).map(String::as_str).unwrap_or("")
    }

    fn ellipsis(&self) -> &str {
        self.glyph("ellipsis")
    }

    /// True while Lua holds an edit buffer for some row: every key crosses raw until it lets go.
    pub fn editing(&self) -> bool {
        self.model.fields.iter().any(|f| f.editing.is_some())
    }

    /// True while a recorder is armed: the next key is the binding, whatever it is.
    pub fn armed(&self) -> bool {
        self.model.fields.iter().any(|f| f.armed)
    }
}

fn trunc(s: &str, max: i64, ellipsis: &str) -> String {
    match usize::try_from(max) {
        Ok(max) => text::truncate(s, max, ellipsis),
        Err(_) => String::new(),
    }
}

fn width(s: &str) -> i64 {
    text::width(s) as i64
}

pub fn plan<'a>(v: &SettingsView<'a>) -> Plan<'a> {
    let n = v.rows.max(0) as usize;
    let g = grid(v.cols);
    let mut out = Plan {
        grid: g,
        rows: vec![None; n],
        hits: vec![None; n],
        groups: &v.model.groups,
        group: None,
        shown: Vec::new(),
        focus: 1,
        scroll: 0,
    };
    let Some(g) = g else {
        let at = (v.rows / 2).max(1);
        out.rows.fill(Some(Row::Space));
        if let Some(row) = out
            .rows
            .get_mut(usize::try_from(at - 1).unwrap_or(usize::MAX))
        {
            *row = Some(Row::TooNarrow);
        }
        return out;
    };

    let groups = &v.model.groups;
    let at = v.ui.group.clamp(1, (groups.len() as i64).max(1));
    let group = groups.get((at - 1) as usize);
    out.group = group;
    for field in &v.model.fields {
        if group.map(|g| g.id.as_str()) != Some(field.group.as_str())
            || !matches(&field.key, &v.ui.filter)
        {
            continue;
        }
        out.shown.push(Shown::Field(field));
        if field.armed {
            for line in v.model.caveat.iter().flatten() {
                out.shown.push(Shown::Caveat(line));
            }
        }
    }

    let body = body_rows(v.rows);
    let count = out.count();
    let focus = v.ui.focus.clamp(1, count.max(1));
    let mut scroll = v.ui.scroll.clamp(0, (count - body).max(0));
    if focus <= scroll {
        scroll = focus - 1;
    } else if focus > scroll + body {
        scroll = focus - body;
    }
    out.focus = focus;
    out.scroll = scroll;

    let set = |rows: &mut Vec<Option<Row<'a>>>, row: i64, spec: Row<'a>| {
        if let Ok(i) = usize::try_from(row - 1)
            && i < n
        {
            rows[i] = Some(spec);
        }
    };
    set(&mut out.rows, 1, Row::Header);
    set(&mut out.rows, 2, Row::Rule);
    set(&mut out.rows, 3, Row::Space);
    for i in 1..=body {
        let nav = groups.get((i - 1) as usize);
        let shown = out.shown.get((i + scroll - 1) as usize).copied();
        let caveat = match shown {
            Some(Shown::Caveat(line)) => Some(line),
            _ => None,
        };
        // a caveat line is not a form row: it has no label, no widget and no target
        let entry = shown.and_then(Shown::field);
        set(
            &mut out.rows,
            3 + i,
            Row::Body(Body {
                nav,
                nav_selected: nav.map(|n| &n.id) == group.map(|g| &g.id),
                caveat,
                field: entry,
                focused: entry.is_some() && (i + scroll) == focus,
            }),
        );
        // one row can carry a nav entry and a form row at once, so the column decides which was clicked
        let mut spans = Vec::new();
        if nav.is_some() {
            spans.push(Span {
                id: SpanId::Nav,
                x1: g.nav_x1,
                x2: g.nav_x2,
            });
        }
        if let Some(e) = entry
            && e.locked.is_none()
            && (e.widget == "picker" || e.widget == "stepper")
        {
            spans.push(Span {
                id: SpanId::Dec,
                x1: g.value_x2 - 11,
                x2: g.value_x2 - 11,
            });
            spans.push(Span {
                id: SpanId::Inc,
                x1: g.value_x2,
                x2: g.value_x2,
            });
        }
        if entry.is_some() {
            spans.push(Span {
                id: SpanId::Value,
                x1: g.value_x2 - 11,
                x2: g.value_x2,
            });
            spans.push(Span {
                id: SpanId::Field,
                x1: g.caret_x,
                x2: g.form_x2,
            });
        }
        if (nav.is_some() || entry.is_some())
            && let Ok(at) = usize::try_from(2 + i)
            && at < n
        {
            out.hits[at] = Some(Hit {
                nav: nav.map(|n| n.id.as_str()),
                field: entry,
                index: entry.map(|_| i + scroll),
                spans,
            });
        }
    }
    if v.rows >= 3 {
        // the rows carry raw keys, which is what makes them greppable; the descriptor's own label
        // and help belong here, where there is room for a sentence
        let current = out.focused();
        set(&mut out.rows, v.rows - 2, Row::Help(current));
        set(&mut out.rows, v.rows - 1, Row::Rule);
        set(&mut out.rows, v.rows, Row::Hints);
    }
    out
}

/// Composed from guarded glyphs rather than written out: the arrows are East Asian Ambiguous and
/// U+23CE is in barely any monospace font, so both go through the same fallback every glyph does.
fn hints(v: &SettingsView, wide: bool) -> String {
    let ud = format!("{}{}", v.glyph("hint_up"), v.glyph("hint_down"));
    let lr = format!("{}{}", v.glyph("hint_left"), v.glyph("hint_right"));
    if wide {
        format!("{ud} field   {lr} change   Enter edit   r reset   c copy as Lua   esc close")
    } else {
        format!("{ud} {lr} Enter r c esc")
    }
}

/// Paints a planned page. The same three cell primitives the sidebar uses, then the same encoder.
pub fn cells(v: &SettingsView) -> (Vec<Option<Vec<Cell>>>, Vec<Option<f64>>) {
    let plan = plan(v);
    let (theme, cols) = (&v.theme, v.cols);
    let dim = theme.meta_fg;
    let ellipsis = v.ellipsis();
    let n = v.rows.max(0) as usize;

    let mut painted: Vec<Option<Vec<Cell>>> = Vec::with_capacity(n);
    for row in 0..n {
        let mut cells = frame::new_line(cols, theme.bg, theme.fg);
        match plan.rows[row] {
            Some(Row::TooNarrow) => {
                let s = format!("Settings needs {MIN_COLS} columns");
                let x = (((cols - width(&s)).div_euclid(2)) + 1).max(1);
                frame::put(&mut cells, x, &s, &Style::fg(theme.fg), cols);
            }
            Some(Row::Header) => {
                let mark = v.glyphs.get("settings").map(String::as_str).unwrap_or("*");
                let head = format!("{mark} Settings");
                let style = Style {
                    fg: theme.fg,
                    bg: None,
                    bold: true,
                };
                frame::put(&mut cells, 2, &head, &style, cols);
                let version = v.model.version.as_deref().unwrap_or("");
                let tag = if cols < WIDE_COLS {
                    version.to_string()
                } else {
                    format!("wez-vtabs {version}")
                };
                frame::put(&mut cells, cols - width(&tag), &tag, &Style::fg(dim), cols);
            }
            Some(Row::Rule) => {
                let rule = v.glyph("rule");
                for x in 2..=(cols - 1) {
                    frame::put(&mut cells, x, rule, &Style::fg(theme.separator), x);
                }
            }
            Some(Row::Help(field)) => {
                if let Some(field) = field {
                    let words = trunc(&field.help, cols - 2, ellipsis);
                    frame::put(&mut cells, 2, &words, &Style::fg(dim), cols);
                }
            }
            Some(Row::Hints) => {
                let text = trunc(&hints(v, cols >= WIDE_COLS), cols - 2, ellipsis);
                frame::put(&mut cells, 2, &text, &Style::fg(dim), cols);
            }
            Some(Row::Body(body)) => {
                let g = plan.grid.expect("a body row implies a grid");
                if let Some(nav) = body.nav {
                    let fg = if body.nav_selected { theme.fg } else { dim };
                    if body.nav_selected {
                        frame::fill(&mut cells, g.nav_x1, g.nav_x2, theme.active_bg);
                        let active = v.glyph("active");
                        frame::put(
                            &mut cells,
                            g.nav_x1,
                            active,
                            &Style::fg(theme.accent),
                            g.nav_x1,
                        );
                    }
                    frame::put(
                        &mut cells,
                        g.nav_x1 + 2,
                        &nav.label,
                        &Style::fg(fg),
                        g.nav_x2,
                    );
                }
                frame::put(
                    &mut cells,
                    g.divider,
                    "│",
                    &Style::fg(theme.separator),
                    g.divider,
                );
                if let Some(line) = body.caveat {
                    let text = trunc(line, g.form_x2 - g.caret_x + 1, ellipsis);
                    frame::put(
                        &mut cells,
                        g.caret_x,
                        &text,
                        &Style::fg(theme.unseen_fg),
                        g.form_x2,
                    );
                }
                if let Some(field) = body.field {
                    paint_field(&mut cells, field, body.focused, &g, v);
                }
            }
            Some(Row::Space) | None => {}
        }
        painted.push(Some(cells));
    }
    (painted, vec![None; n])
}

fn paint_field(cells: &mut [Cell], row: &SettingsField, focused: bool, g: &Grid, v: &SettingsView) {
    let theme = &v.theme;
    let dim = theme.meta_fg;
    let ellipsis = v.ellipsis();
    if focused {
        let caret = v.glyph("focus");
        frame::put(cells, g.caret_x, caret, &Style::fg(theme.accent), g.caret_x);
    }
    let label_fg = if row.locked.is_some() { dim } else { theme.fg };
    let label = trunc(&row.label, g.label_x2 - g.label_x + 1, ellipsis);
    frame::put(cells, g.label_x, &label, &Style::fg(label_fg), g.label_x2);

    let text = &row.value_text;
    if let Some(lock) = &row.locked {
        let room = (g.value_x2 - g.label_x2 - 1).max(1);
        if focused {
            let short = short_source(&lock.text);
            let badge = [
                format!("LOCKED {}", lock.text),
                lock.text.clone(),
                format!("LOCKED {short}"),
            ]
            .into_iter()
            .find(|c| width(c) <= room)
            .unwrap_or_else(|| trunc(short, room, ellipsis));
            let x = g.value_x2 - width(&badge) + 1;
            frame::put(cells, x, &badge, &Style::fg(theme.unseen_fg), g.value_x2);
            return;
        }
        let shown = trunc(text, (room - 9).max(1), ellipsis);
        let x = g.value_x2 - width(&shown) + 1;
        frame::put(cells, x, &shown, &Style::fg(dim), g.value_x2);
        frame::put(cells, x - 8, "LOCKED", &Style::fg(theme.unseen_fg), x - 2);
        return;
    }
    let shown = trunc(text, g.value_x2 - g.label_x2 - 1, ellipsis);
    let value_fg = if focused || row.changed {
        theme.fg
    } else {
        dim
    };
    let x = g.value_x2 - width(&shown) + 1;
    frame::put(cells, x, &shown, &Style::fg(value_fg), g.value_x2);
    if row.changed {
        let marker = v.glyph("unseen");
        frame::put(
            cells,
            g.marker_x,
            marker,
            &Style::fg(theme.accent),
            g.marker_x,
        );
    }
}

/// The two golden serializations, so the parity test compares what the runtime encodes.
pub fn golden_dumps(v: &SettingsView) -> (String, String) {
    let (painted, fades) = cells(v);
    frame::dumps(&painted, &fades, v.theme.bg, v.cols)
}

/// A committed `settings-*` scene: the model body plus the pane facts the wire never carries.
/// `rev` is stamped at send time, so the fixture stores the body without it.
#[derive(serde::Deserialize)]
pub struct Scene {
    pub cols: i64,
    pub rows: i64,
    pub theme: Theme,
    pub glyphs: BTreeMap<String, String>,
    #[serde(default)]
    pub ui: SettingsUi,
    pub model: serde_json::Map<String, serde_json::Value>,
}

/// Renders a scene the way the runtime would, for the parity gate and `dump-frames` alike.
pub fn scene_dumps(scene: &Scene) -> Result<(String, String), serde_json::Error> {
    let mut body = scene.model.clone();
    body.insert("rev".into(), serde_json::json!(1));
    let model: ModelMsg = serde_json::from_value(serde_json::Value::Object(body))?;
    let view = SettingsView {
        cols: scene.cols,
        rows: scene.rows,
        model: &model,
        ui: &scene.ui,
        theme: scene.theme.clone(),
        glyphs: scene.glyphs.clone(),
    };
    Ok(golden_dumps(&view))
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
