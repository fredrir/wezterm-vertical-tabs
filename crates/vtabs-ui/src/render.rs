use crate::{input::display_text, *};
use ratatui::{
    layout::Position,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Widget, Wrap},
};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::Model;

impl SidebarUi {
    /// Returns None when a native terminal repaint can reuse the previously committed UI.
    /// Resize publishes one fully composed frame; the previous buffer survives until swap.
    pub fn render(&mut self, model: &Model, area: Rect, now: Duration) -> Option<FrameUpdate> {
        self.now = now;
        let resized = self.buffer.area != area;
        if resized {
            self.reveal_selection = true;
            self.staging.resize(area);
            self.cancel_effects();
            self.dirty = true;
        }
        if self.revision != Some(model.revision) {
            if self
                .pending_form
                .is_some_and(|revision| revision != model.revision)
            {
                self.dismiss();
            }
            self.revision = Some(model.revision);
            self.update_theme(model);
            self.config_owned.clone_from(&model.config_owned);
            if self
                .last_rail
                .is_some_and(|rail| rail != model.settings.rail)
                && model.settings.animations
                && !model.settings.reduced_motion
                && model.settings.animation_ms > 0
                && self.window_focused
                && self.visible
                && !self.is_modal()
                && !area.is_empty()
            {
                let direction = if model.settings.side == vtabs_core::Side::Right {
                    0.15
                } else {
                    -0.15
                };
                self.transition_surface(
                    direction,
                    0.0,
                    now,
                    Duration::from_millis(u64::from(model.settings.animation_ms)),
                );
            }
            self.last_rail = Some(model.settings.rail);
            self.space_activity.clear();
            self.space_activity.extend(
                model
                    .tabs
                    .values()
                    .filter(|tab| tab.unread || tab.bell)
                    .map(|tab| tab.space_id.clone()),
            );
            if self.last_selected_space.as_ref() != Some(&model.selected_space) {
                self.reveal_selection = true;
                self.tab_scroll = 0;
                if let Some(index) = model
                    .spaces
                    .iter()
                    .position(|space| space.id == model.selected_space)
                {
                    let rows = usize::from(self.spaces_rect.height).max(1);
                    if index < self.space_scroll {
                        self.space_scroll = index;
                    } else if index >= self.space_scroll + rows {
                        self.space_scroll = index + 1 - rows;
                    }
                }
                self.last_selected_space = Some(model.selected_space.clone());
            }
            if self.last_selected_tab != model.selected_tab {
                self.reveal_selection = true;
                if let Some(id) = model.selected_tab {
                    self.ensure_tab_visible(model, id);
                }
                self.last_selected_tab = model.selected_tab;
            }
            self.dirty = true;
            if !model.settings.animations || model.settings.reduced_motion {
                self.cancel_effects();
            }
            self.prune_targets(model);
        }
        if !self.visible {
            return None;
        }
        if self.caret_deadline.is_some_and(|deadline| now >= deadline) {
            self.caret_visible = !self.caret_visible;
            self.caret_deadline = Some(now + Duration::from_millis(600));
            self.dirty = true;
        }
        if self
            .tooltip_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.tooltip_deadline = None;
            self.show_tooltip = self.hovered.is_some();
            self.dirty = true;
        }
        if !self.dirty && self.effect.is_none() && self.motion.is_none() {
            return None;
        }
        self.staging.resize(area);
        self.staging.reset();
        self.hits.clear();
        self.rounded_surfaces.clear();
        self.cursor = None;
        self.editor_rect = Rect::default();
        Block::default()
            .style(self.theme.base())
            .render(area, &mut self.staging);
        if area.width > 0 && area.height > 0 {
            let width = self.sidebar_columns.unwrap_or(area.width).min(area.width);
            // A successful form can close during this render before the host contracts
            // its viewport. Keep rail targets within the configured width in that frame.
            let sidebar = if area.width > width {
                Rect::new(
                    if model.settings.side == vtabs_core::Side::Right {
                        area.right() - width
                    } else {
                        area.x
                    },
                    area.y,
                    width,
                    area.height,
                )
            } else {
                area
            };
            self.sidebar_rect = sidebar;
            self.compose_sidebar(model, sidebar);
            if self.settings_page {
                let page = if area.width > sidebar.width {
                    Rect::new(
                        if model.settings.side == vtabs_core::Side::Right {
                            area.x
                        } else {
                            sidebar.right()
                        },
                        area.y,
                        area.width - sidebar.width,
                        area.height,
                    )
                } else {
                    area
                };
                self.page_rect = page;
                if page == area {
                    self.hits.clear();
                }
                self.compose_settings_page(model, page);
            }
            if let Some(mut overlay) = self.overlay.take() {
                // Modal hit regions replace underlying targets; background clicks dismiss.
                self.hits.clear();
                self.compose_overlay(model, area, &mut overlay);
                self.overlay = Some(overlay);
            } else if self.show_tooltip {
                self.compose_tooltip(area);
            }
        }
        let elapsed = now
            .saturating_sub(self.last_frame)
            .min(Duration::from_millis(1000));
        if let Some(effect) = self.effect.as_mut() {
            let effect_area = self.effect_area.unwrap_or(area).intersection(area);
            effect.process(
                tachyonfx::Duration::from_millis(
                    elapsed.as_millis().min(u128::from(u32::MAX)) as u32
                ),
                &mut self.staging,
                effect_area,
            );
            if effect.done() {
                self.effect = None;
                self.effect_area = None;
            }
        }
        let mut transform = SurfaceTransform {
            translate_x: 0.0,
            opacity: 1.0,
        };
        if let Some(motion) = self.motion {
            let t = (now.saturating_sub(motion.start).as_secs_f32()
                / motion.duration.as_secs_f32())
            .clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            transform.translate_x = motion.from + (motion.to - motion.from) * eased;
            if t >= 1.0 {
                self.motion = None;
            }
        }
        let changed_cells = if resized {
            (area.y..area.bottom())
                .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                .collect()
        } else {
            self.buffer
                .diff(&self.staging)
                .into_iter()
                .map(|(x, y, _)| (x, y))
                .collect::<Vec<_>>()
        };
        let mut dirty_rows = Vec::new();
        for &(_, y) in &changed_cells {
            if dirty_rows.last() != Some(&y) {
                dirty_rows.push(y);
            }
        }
        std::mem::swap(&mut self.buffer, &mut self.staging);
        self.last_frame = now;
        self.dirty = false;
        self.frame_revision = self.frame_revision.wrapping_add(1);
        let editor = match &self.overlay {
            Some(Overlay::Form(form)) if self.focused == Some(ElementId::Editor) => {
                Some(&form.editor)
            }
            Some(Overlay::Menu(menu)) => menu.search.as_ref().map(|search| &search.editor),
            _ if self.settings_page && self.settings_search_focused => Some(&self.settings_query),
            _ => None,
        };
        let ime_rect = editor.filter(|_| self.editor_rect.width > 0).map(|editor| {
            Rect::new(
                self.editor_rect.x
                    + editor
                        .cursor_columns()
                        .saturating_sub(editor.scroll_columns)
                        .min(usize::from(self.editor_rect.width - 1)) as u16,
                self.editor_rect.y,
                1,
                1,
            )
        });
        Some(FrameUpdate {
            revision: self.frame_revision,
            resized,
            changed_cells,
            dirty_rows,
            cursor: self.cursor,
            ime_rect,
            transform,
        })
    }

    fn update_theme(&mut self, model: &Model) {
        let settings = &model.settings;
        self.theme.background =
            Theme::parse_color(&settings.background).unwrap_or(self.theme.background);
        self.theme.foreground =
            Theme::parse_color(&settings.foreground).unwrap_or(self.theme.foreground);
        self.theme.muted = Theme::parse_color(&settings.muted).unwrap_or(self.theme.muted);
        self.theme.selected =
            Theme::parse_color(&settings.selected_background).unwrap_or(self.theme.selected);
        self.theme.private =
            Theme::parse_color(&settings.private_accent).unwrap_or(self.theme.private);
        self.theme.accent = if model.private {
            self.theme.private
        } else {
            model
                .spaces
                .iter()
                .find(|space| space.id == model.selected_space)
                .and_then(|space| space.accent.as_deref())
                .and_then(Theme::parse_color)
                .or_else(|| Theme::parse_color(&settings.accent))
                .unwrap_or(self.theme.accent)
        };
        self.theme.sync_surfaces();
    }

    fn prune_targets(&mut self, model: &Model) {
        let valid = |id: &ElementId| match id {
            ElementId::Tab(id) | ElementId::CloseTab(id) => model.tabs.contains_key(id),
            ElementId::Space(id) => model.spaces.iter().any(|space| &space.id == id),
            _ => true,
        };
        if self.focused.as_ref().is_some_and(|id| !valid(id)) {
            self.focused = None;
        }
        if self.hovered.as_ref().is_some_and(|id| !valid(id)) {
            self.hovered = None;
            self.show_tooltip = false;
        }
        if self.drag.as_ref().is_some_and(|id| !valid(id)) {
            self.drag = None;
        }
        let prune = |overlay: &mut Overlay| {
            if let Overlay::Menu(menu) = overlay {
                menu.items.retain(|item| action_exists(model, &item.action));
                if let Some(search) = &mut menu.search {
                    search
                        .all_items
                        .retain(|item| action_exists(model, &item.action));
                }
                menu.selected = menu.selected.min(menu.items.len().saturating_sub(1));
            }
        };
        if let Some(overlay) = &mut self.overlay {
            prune(overlay);
        }
        for overlay in &mut self.overlay_stack {
            prune(overlay);
        }
        if self.overlay.as_ref().is_some_and(|overlay| match overlay {
            Overlay::Form(Form {
                kind: FormKind::RenameTab(id),
                ..
            }) => !model.tabs.contains_key(id),
            Overlay::Form(Form {
                kind:
                    FormKind::RenameSpace(id)
                    | FormKind::SpaceIcon(id)
                    | FormKind::SpaceAccent(id)
                    | FormKind::SpaceRules(id),
                ..
            }) => !model.spaces.iter().any(|space| &space.id == id),
            _ => false,
        }) {
            self.dismiss();
        }
    }

    pub(crate) fn write(&mut self, rect: Rect, text: impl Into<Line<'static>>, style: Style) {
        if rect.width > 0 && rect.height > 0 {
            Paragraph::new(text.into())
                .style(style)
                .render(rect, &mut self.staging);
        }
    }
    pub(crate) fn hit(&mut self, id: ElementId, rect: Rect, tooltip: impl Into<String>) {
        if rect.width > 0 && rect.height > 0 {
            self.hits.push(HitRegion {
                id,
                rect,
                tooltip: tooltip.into(),
            });
        }
    }
    pub(crate) fn item_style(&self, id: &ElementId, selected: bool) -> Style {
        let mut style = self.theme.base();
        if selected {
            style = style
                .bg(self.theme.selected)
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD);
        }
        if self.hovered.as_ref() == Some(id) {
            style = style.bg(self.theme.card).fg(self.theme.accent);
        }
        style
    }

    fn compose_overlay(&mut self, _model: &Model, area: Rect, overlay: &mut Overlay) {
        let searching = matches!(
            overlay,
            Overlay::Menu(Menu {
                search: Some(_),
                ..
            })
        );
        let width = area.width.min(64);
        let desired_height = match overlay {
            Overlay::Menu(menu) => menu
                .search
                .as_ref()
                .map_or(menu.items.len(), |search| search.all_items.len())
                .max(1)
                .saturating_add(if searching { 4 } else { 2 })
                .min(usize::from(u16::MAX)) as u16,
            Overlay::Form(_) => 7,
        };
        let height = area.height.min(desired_height);
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        self.overlay_rect = rect;
        Clear.render(rect, &mut self.staging);
        self.rounded(rect, self.theme.background, self.theme.border);
        let framed = rect.width >= 4 && rect.height >= 3;
        let search_header = searching && rect.height >= 5;
        let inner = if framed {
            Rect::new(
                rect.x + 1,
                rect.y + if search_header { 3 } else { 1 },
                rect.width - 2,
                rect.height - if search_header { 4 } else { 2 },
            )
        } else {
            rect
        };
        match overlay {
            Overlay::Menu(menu) => {
                if framed {
                    let title = Rect::new(
                        rect.x + 1,
                        rect.y + u16::from(search_header),
                        rect.width.saturating_sub(2),
                        1,
                    );
                    if let Some(search) = &mut menu.search {
                        let field = Rect::new(
                            rect.x,
                            rect.y,
                            rect.width,
                            if search_header { 3 } else { 1 },
                        );
                        self.rounded(field, self.theme.card, self.theme.accent);
                        let label = if title.width >= 12 { "⌕ " } else { "" };
                        let label_width = label.width() as u16;
                        let edit =
                            Rect::new(title.x + label_width, title.y, title.width - label_width, 1);
                        self.editor_rect = edit;
                        self.write(title, label, self.theme.muted());
                        self.hit(ElementId::Editor, field, "Search tabs");
                        search.editor.keep_cursor_visible(usize::from(edit.width));
                        let mut col = 0;
                        let text: String = search
                            .editor
                            .display_text()
                            .graphemes(true)
                            .filter(|g| {
                                let start = col;
                                col += g.width();
                                start >= search.editor.scroll_columns
                            })
                            .collect();
                        self.write(edit, text, self.theme.base().bg(self.theme.card));
                        self.compose_editor_marks(&search.editor, edit);
                        if self.window_focused && edit.width > 0 {
                            self.cursor = Some(Position::new(
                                self.editor_rect.x
                                    + search
                                        .editor
                                        .cursor_columns()
                                        .saturating_sub(search.editor.scroll_columns)
                                        .min(usize::from(edit.width - 1))
                                        as u16,
                                edit.y,
                            ));
                        }
                    } else {
                        self.write(title, display_text(&menu.title), self.theme.accent());
                    }
                }
                if menu.items.is_empty() && inner.height > 0 {
                    self.write(
                        Rect::new(inner.x, inner.y, inner.width, 1),
                        "No matching tabs",
                        self.theme.muted(),
                    );
                }
                let rows = usize::from(inner.height);
                if menu.selected < menu.scroll {
                    menu.scroll = menu.selected;
                }
                if menu.selected >= menu.scroll + rows.max(1) {
                    menu.scroll = menu.selected.saturating_sub(rows.saturating_sub(1));
                }
                menu.scroll = menu
                    .scroll
                    .min(menu.items.len().saturating_sub(rows.max(1)));
                for (offset, item) in menu.items.iter().skip(menu.scroll).take(rows).enumerate() {
                    let row = Rect::new(inner.x, inner.y + offset as u16, inner.width, 1);
                    let selected = menu.scroll + offset == menu.selected;
                    self.rounded(
                        row,
                        if selected {
                            self.theme.selected
                        } else {
                            self.theme.background
                        },
                        self.theme.background,
                    );
                    let style = if !item.enabled {
                        self.theme.muted()
                    } else if selected {
                        self.theme.accent().bg(self.theme.selected)
                    } else {
                        self.theme.base()
                    };
                    let hint_width = item.hint.width().min(usize::from(row.width)) as u16;
                    self.write(
                        Rect::new(row.x, row.y, row.width.saturating_sub(hint_width), 1),
                        format!(
                            "{} {}",
                            if selected { "›" } else { " " },
                            display_text(&item.label)
                        ),
                        style,
                    );
                    if hint_width > 0 {
                        self.write(
                            Rect::new(row.right() - hint_width, row.y, hint_width, 1),
                            item.hint.clone(),
                            style.fg(self.theme.muted),
                        );
                    }
                    self.hit(ElementId::Menu(item.id.clone()), row, item.label.clone());
                }
            }
            Overlay::Form(form) => {
                if inner.height == 0 {
                    return;
                }
                self.write(
                    Rect::new(inner.x, inner.y, inner.width, 1),
                    display_text(&form.title),
                    self.theme.accent(),
                );
                let input_y = inner.y + u16::from(inner.height > 1);
                let edit = Rect::new(inner.x, input_y, inner.width, 1);
                let editing = self.focused == Some(ElementId::Editor);
                self.editor_rect = edit;
                form.editor.keep_cursor_visible(usize::from(edit.width));
                let text = form.editor.display_text();
                let mut col = 0;
                let display: String = text
                    .graphemes(true)
                    .filter(|g| {
                        let start = col;
                        col += g.width();
                        start >= form.editor.scroll_columns
                    })
                    .collect();
                self.rounded(
                    edit,
                    self.theme.selected,
                    if editing {
                        self.theme.accent
                    } else {
                        self.theme.border
                    },
                );
                self.write(edit, display, self.theme.base().bg(self.theme.selected));
                self.hit(ElementId::Editor, edit, "Text entry");
                if editing {
                    self.compose_editor_marks(&form.editor, edit);
                }
                if editing && self.caret_visible && self.window_focused && edit.width > 0 {
                    let x = edit.x
                        + form
                            .editor
                            .cursor_columns()
                            .saturating_sub(form.editor.scroll_columns)
                            .min(usize::from(edit.width - 1)) as u16;
                    self.cursor = Some(Position::new(x, edit.y));
                }
                if inner.height > 2 {
                    self.write(
                        Rect::new(inner.x, input_y + 1, inner.width, 1),
                        form.error
                            .clone()
                            .unwrap_or_else(|| "Enter saves   Esc cancels".into()),
                        if form.error.is_some() {
                            self.theme.base().fg(self.theme.danger)
                        } else {
                            self.theme.muted()
                        },
                    );
                }
                if inner.height > 3 {
                    let row = inner.bottom() - 1;
                    let button_width = (inner.width / 2).clamp(1, 8);
                    let save = Rect::new(inner.x, row, inner.width.min(button_width), 1);
                    let gap = u16::from(inner.width > save.width + 1);
                    let cancel = Rect::new(
                        save.right() + gap,
                        row,
                        inner.width.saturating_sub(save.width + gap).min(8),
                        1,
                    );
                    for (id, button, label) in [
                        (ElementId::Submit, save, " Save"),
                        (ElementId::Cancel, cancel, " Cancel"),
                    ] {
                        let focused = self.focused.as_ref() == Some(&id);
                        let fill = if focused || self.hovered.as_ref() == Some(&id) {
                            self.theme.selected
                        } else {
                            self.theme.card
                        };
                        self.rounded(
                            button,
                            fill,
                            if focused {
                                self.theme.accent
                            } else {
                                self.theme.border
                            },
                        );
                        self.write(button, label, self.item_style(&id, false).bg(fill));
                        self.hit(id, button, label.trim());
                    }
                }
            }
        }
    }

    fn compose_editor_marks(&mut self, editor: &TextEditor, rect: Rect) {
        let visible_columns = |range: std::ops::Range<usize>| {
            let start = range
                .start
                .saturating_sub(editor.scroll_columns)
                .min(usize::from(rect.width)) as u16;
            let end = range
                .end
                .saturating_sub(editor.scroll_columns)
                .min(usize::from(rect.width)) as u16;
            rect.x + start..rect.x + end
        };
        if let Some(selection) = editor.selection_columns() {
            let columns = visible_columns(selection);
            if !columns.is_empty() {
                self.rounded_surfaces.push(RoundedSurface {
                    rect: Rect::new(columns.start, rect.y, columns.end - columns.start, 1),
                    fill: self.theme.accent,
                    border: self.theme.accent,
                    radius: 2.0,
                    inset: 0.0,
                });
                for x in columns {
                    self.staging[(x, rect.y)].set_style(
                        Style::default()
                            .fg(self.theme.background)
                            .bg(self.theme.accent),
                    );
                }
            }
        }
        for x in visible_columns(editor.preedit_columns()) {
            self.staging[(x, rect.y)]
                .set_style(Style::default().add_modifier(Modifier::UNDERLINED));
        }
    }

    fn compose_tooltip(&mut self, area: Rect) {
        let Some(hit) = self
            .hovered
            .as_ref()
            .and_then(|id| self.hits.iter().find(|hit| &hit.id == id))
            .cloned()
        else {
            self.show_tooltip = false;
            self.tooltip_deadline = None;
            return;
        };
        if area.width < 8 || area.height < 4 {
            return;
        }
        let text = hit.tooltip.lines().map(display_text).collect::<Vec<_>>();
        let natural_width = text.iter().map(|line| line.width()).max().unwrap_or(0);
        if natural_width == 0 {
            return;
        }
        let width = natural_width
            .min(42)
            .min(usize::from(area.width - 2))
            .max(1);
        let mut lines = 0usize;
        for line in &text {
            lines += 1;
            let mut used = 0usize;
            for word in line.split_whitespace() {
                let word_width = word.width();
                if used > 0 && used + 1 + word_width > width {
                    lines += 1;
                    used = 0;
                }
                used += usize::from(used > 0) + word_width;
                lines += used.saturating_sub(1) / width;
                used = used.saturating_sub(1) % width + 1;
            }
        }
        let height = (lines + 2).min(usize::from(area.height)) as u16;
        let width = width as u16 + 2;
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        Clear.render(rect, &mut self.staging);
        self.rounded(rect, self.theme.card, self.theme.border);
        let content = Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2);
        let text = text
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let style = if index == 0 {
                    self.theme.base()
                } else {
                    self.theme.muted()
                };
                Line::styled(line, style.bg(self.theme.card))
            })
            .collect::<Vec<_>>();
        Paragraph::new(text)
            .style(self.theme.base().bg(self.theme.card))
            .wrap(Wrap { trim: true })
            .render(content, &mut self.staging);
    }
}

fn action_exists(model: &Model, action: &Action) -> bool {
    use vtabs_core::Intent;
    match action {
        Action::Domain(
            Intent::ActivateTab(id)
            | Intent::CloseTab(id)
            | Intent::CloseOthers(id)
            | Intent::ReturnToAuto(id)
            | Intent::MoveTabToNewWindow(id)
            | Intent::RenameTab { id, .. }
            | Intent::PinTab { id, .. }
            | Intent::MoveTab { id, .. },
        )
        | Action::RenameTab(id)
        | Action::MoveTab(id) => model.tabs.contains_key(id),
        Action::Domain(Intent::AssignTab { id, space_id }) => {
            model.tabs.contains_key(id) && model.spaces.iter().any(|space| &space.id == space_id)
        }
        Action::Domain(
            Intent::SelectSpace(id)
            | Intent::RenameSpace { id, .. }
            | Intent::EditSpace { id, .. }
            | Intent::MoveSpace { id, .. },
        )
        | Action::RenameSpace(id)
        | Action::EditSpaceIcon(id)
        | Action::EditSpaceAccent(id)
        | Action::EditSpaceRules(id)
        | Action::DeleteSpace(id) => model.spaces.iter().any(|space| &space.id == id),
        Action::Domain(Intent::DeleteSpace { id, destination }) => {
            model.spaces.len() > 1
                && model.spaces.iter().any(|space| &space.id == id)
                && destination.as_ref().is_none_or(|destination| {
                    model.spaces.iter().any(|space| &space.id == destination)
                })
        }
        Action::Confirm { action, .. } => action_exists(model, action),
        Action::Submenu { items, .. } => {
            items.iter().any(|item| action_exists(model, &item.action))
        }
        _ => true,
    }
}
