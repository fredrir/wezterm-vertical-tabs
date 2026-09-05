use crate::{input::display_text, *};
use ratatui::{
    layout::Position,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtabs_core::{Model, RailMode};

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
        self.cursor = None;
        self.editor_rect = Rect::default();
        Block::default()
            .style(self.theme.base())
            .render(area, &mut self.staging);
        if area.width > 0 && area.height > 0 {
            self.compose_sidebar(model, area);
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
            Some(Overlay::Form(form)) => Some(&form.editor),
            Some(Overlay::Menu(menu)) => menu.search.as_ref().map(|search| &search.editor),
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

    fn write(&mut self, rect: Rect, text: impl Into<Line<'static>>, style: Style) {
        if rect.width > 0 && rect.height > 0 {
            Paragraph::new(text.into())
                .style(style)
                .render(rect, &mut self.staging);
        }
    }
    fn hit(&mut self, id: ElementId, rect: Rect, tooltip: impl Into<String>) {
        if rect.width > 0 && rect.height > 0 {
            self.hits.push(HitRegion {
                id,
                rect,
                tooltip: tooltip.into(),
            });
        }
    }
    fn item_style(&self, id: &ElementId, selected: bool) -> Style {
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
        if self.focused.as_ref() == Some(id) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    fn compose_sidebar(&mut self, model: &Model, area: Rect) {
        let compact = model.settings.rail == RailMode::Collapsed || area.width < 12;
        let plus_width = if area.width >= 5 { 3 } else { 1 };
        let plus_rect = Rect::new(area.right() - plus_width, area.y, plus_width, 1);
        let header = Rect::new(area.x, area.y, area.width.saturating_sub(plus_width), 1);
        self.write(
            header,
            if model.private {
                "◈ Private tabs".to_owned()
            } else {
                "Spaces".to_owned()
            },
            self.theme.accent().add_modifier(Modifier::BOLD),
        );
        if model.private {
            self.hit(
                ElementId::PrivateInfo,
                header,
                "Private tabs and history stay in memory. Spaces and settings are shared.",
            );
        }
        self.write(
            plus_rect,
            if plus_width == 3 { "(+)" } else { "+" },
            self.item_style(&ElementId::CreateSpace, false),
        );
        self.hit(ElementId::CreateSpace, plus_rect, "Create space");
        // The create-space control always owns the final header cell, even at 1×1.
        if area.height == 1 {
            return;
        }
        let control_rows = if area.height >= 5 { 2 } else { 1 };
        let footer_text_rows = model
            .footer
            .lines()
            .count()
            .min(usize::from(area.height / 4))
            .min(16) as u16;
        let footer_rows = control_rows + footer_text_rows;
        let available = area.height.saturating_sub(1 + footer_rows);
        let space_rows = (available / 3)
            .max(u16::from(available > 0))
            .min(model.spaces.len().min(usize::from(u16::MAX)) as u16);
        self.spaces_rect = Rect::new(area.x, area.y + 1, area.width, space_rows);
        if self.reveal_selection
            && let Some(index) = model
                .spaces
                .iter()
                .position(|space| space.id == model.selected_space)
        {
            if index < self.space_scroll {
                self.space_scroll = index;
            } else if index >= self.space_scroll + usize::from(space_rows).max(1) {
                self.space_scroll = index + 1 - usize::from(space_rows).max(1);
            }
        }
        self.space_scroll = self
            .space_scroll
            .min(model.spaces.len().saturating_sub(usize::from(space_rows)));
        for (row, space) in model
            .spaces
            .iter()
            .skip(self.space_scroll)
            .take(usize::from(space_rows))
            .enumerate()
        {
            let rect = Rect::new(area.x, self.spaces_rect.y + row as u16, area.width, 1);
            let selected = space.id == model.selected_space;
            let activity = self.space_activity.contains(&space.id);
            let label = if compact {
                format!(
                    "{}{}",
                    if selected { "▏" } else { " " },
                    if space.icon.is_empty() {
                        space.name.graphemes(true).next().unwrap_or("·")
                    } else {
                        &space.icon
                    }
                )
            } else {
                format!(
                    "{} {} {}{}",
                    if selected { "▸" } else { " " },
                    space.icon,
                    display_text(&space.name),
                    if activity { " •" } else { "" }
                )
            };
            let id = ElementId::Space(space.id.clone());
            self.write(rect, label, self.item_style(&id, selected));
            self.hit(
                id,
                rect,
                format!(
                    "{}{}",
                    space.name,
                    if activity { " — activity" } else { "" }
                ),
            );
        }
        let footer_y = area.bottom() - footer_rows;
        let tabs_y = self.spaces_rect.bottom();
        self.tabs_rect = Rect::new(area.x, tabs_y, area.width, footer_y.saturating_sub(tabs_y));
        if self.reveal_selection {
            if let Some(id) = model.selected_tab {
                self.ensure_tab_visible(model, id);
            }
            self.reveal_selection = false;
        }
        let visible = model.visible_ids();
        let card_rows = if model.settings.cards && !compact && self.tabs_rect.height >= 4 {
            if model.settings.show_metadata { 3 } else { 2 }
        } else {
            1
        };
        let count = usize::from(self.tabs_rect.height / card_rows);
        self.tab_scroll = self
            .tab_scroll
            .min(visible.len().saturating_sub(count.max(1)));
        if visible.is_empty() && self.tabs_rect.height > 0 {
            let text = if compact {
                "∅"
            } else {
                "No tabs in this space"
            };
            self.write(
                Rect::new(area.x, tabs_y, area.width, 1),
                text,
                self.theme.muted(),
            );
            if self.tabs_rect.height >= 3 {
                let rect = Rect::new(area.x, tabs_y + 2, area.width, 1);
                self.write(rect, "+ New tab", self.theme.accent());
                self.hit(ElementId::NewTab, rect, "New tab in selected space");
            }
        }
        for (slot, tab) in visible
            .iter()
            .skip(self.tab_scroll)
            .take(count)
            .filter_map(|id| model.tabs.get(id))
            .enumerate()
        {
            let rect = Rect::new(
                area.x,
                tabs_y + slot as u16 * card_rows,
                area.width,
                card_rows,
            );
            let selected = model.selected_tab == Some(tab.id);
            let id = ElementId::Tab(tab.id);
            let style = self.item_style(&id, selected);
            Block::default()
                .style(if selected {
                    style
                } else {
                    style.bg(self.theme.card)
                })
                .render(rect, &mut self.staging);
            let index = self.tab_scroll + slot + 1;
            let marker = match (tab.pinned, tab.bell, tab.unread) {
                (true, true, _) => "◆!",
                (true, false, true) => "◆•",
                (true, false, false) => "◆",
                (false, true, _) => "!",
                (false, false, true) => "•",
                _ => " ",
            };
            let icon = if tab.icon.is_empty() {
                if tab.remote { "↗" } else { "›" }.to_owned()
            } else {
                display_text(&tab.icon)
            };
            let label = if compact {
                format!("{marker}{index}")
            } else {
                format!(
                    "{marker}{}{}{}{}",
                    if model.settings.show_indices {
                        format!(" {index} ")
                    } else {
                        " ".into()
                    },
                    icon,
                    " ",
                    display_text(tab.display_title())
                )
            };
            let close_width = u16::from(model.settings.show_close && area.width >= 12);
            self.write(
                Rect::new(rect.x, rect.y, rect.width.saturating_sub(close_width), 1),
                label,
                style,
            );
            self.hit(
                id,
                rect,
                format!("{}\n{} · {}", tab.display_title(), tab.cwd, tab.domain),
            );
            if close_width > 0 {
                let close = Rect::new(rect.right() - 1, rect.y, 1, 1);
                self.write(close, "×", style.fg(self.theme.muted));
                self.hit(ElementId::CloseTab(tab.id), close, "Close tab");
            }
            if card_rows >= 3 {
                self.write(
                    Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), 1),
                    format!("{} · {}", display_text(&tab.cwd), display_text(&tab.domain)),
                    style.fg(self.theme.muted),
                );
            }
            if card_rows >= 2
                && tab.pinned
                && visible
                    .get(index)
                    .and_then(|id| model.tabs.get(id))
                    .is_some_and(|next| !next.pinned)
            {
                self.write(
                    Rect::new(rect.x, rect.bottom() - 1, rect.width, 1),
                    "─".repeat(usize::from(rect.width)),
                    self.theme.muted(),
                );
            }
        }
        for (row, line) in model
            .footer
            .lines()
            .take(usize::from(footer_text_rows))
            .enumerate()
        {
            self.write(
                Rect::new(area.x, footer_y + row as u16, area.width, 1),
                display_text(line),
                self.theme.muted(),
            );
        }
        let new_tab = Rect::new(area.x, footer_y + footer_text_rows, area.width, 1);
        self.write(
            new_tab,
            if compact { "+" } else { "+ New tab" },
            self.item_style(&ElementId::NewTab, false),
        );
        self.hit(ElementId::NewTab, new_tab, "New tab in selected space");
        if control_rows == 2 {
            let rail = Rect::new(area.right() - 1, area.bottom() - 1, 1, 1);
            let settings = Rect::new(area.x, area.bottom() - 1, area.width.saturating_sub(1), 1);
            self.write(
                settings,
                if compact { "⚙" } else { "⚙ Settings" },
                self.item_style(&ElementId::Settings, false),
            );
            self.hit(ElementId::Settings, settings, "Settings");
            self.write(
                rail,
                if compact { "›" } else { "‹" },
                self.item_style(&ElementId::Rail, false),
            );
            self.hit(ElementId::Rail, rail, "Toggle rail");
        }
    }

    fn compose_overlay(&mut self, model: &Model, area: Rect, overlay: &mut Overlay) {
        let width = area.width.min(64);
        let desired_height = match overlay {
            Overlay::Menu(menu) => menu
                .search
                .as_ref()
                .map_or(menu.items.len(), |search| search.all_items.len())
                .max(1)
                .saturating_add(2)
                .min(usize::from(u16::MAX)) as u16,
            Overlay::Form(_) => 7,
            Overlay::Settings { .. } => area.height,
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
        let framed = rect.width >= 4 && rect.height >= 3;
        let block = Block::default()
            .borders(if framed { Borders::ALL } else { Borders::NONE })
            .style(self.theme.base())
            .border_style(Style::default().fg(self.theme.border));
        let inner = block.inner(rect);
        block.render(rect, &mut self.staging);
        match overlay {
            Overlay::Menu(menu) => {
                if framed {
                    let title = if let Some(search) = &mut menu.search {
                        let width = rect.width.saturating_sub(10);
                        self.editor_rect =
                            Rect::new((rect.x + 9).min(rect.right()), rect.y, width, 1);
                        search.editor.keep_cursor_visible(usize::from(width));
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
                        if self.window_focused && width > 0 {
                            self.cursor = Some(Position::new(
                                self.editor_rect.x
                                    + search
                                        .editor
                                        .cursor_columns()
                                        .saturating_sub(search.editor.scroll_columns)
                                        .min(usize::from(width - 1))
                                        as u16,
                                rect.y,
                            ));
                        }
                        format!("Filter: {text}")
                    } else {
                        menu.title.clone()
                    };
                    self.write(
                        Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
                        display_text(&title),
                        self.theme.accent(),
                    );
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
                self.write(edit, display, self.theme.base().bg(self.theme.selected));
                self.hit(ElementId::Editor, edit, "Text entry");
                if let Some(selection) = form.editor.selection_columns() {
                    let start = selection
                        .start
                        .saturating_sub(form.editor.scroll_columns)
                        .min(usize::from(edit.width)) as u16;
                    let end = selection
                        .end
                        .saturating_sub(form.editor.scroll_columns)
                        .min(usize::from(edit.width)) as u16;
                    for x in edit.x + start..edit.x + end {
                        self.staging[(x, edit.y)].set_style(
                            Style::default()
                                .fg(self.theme.background)
                                .bg(self.theme.accent),
                        );
                    }
                }
                if !form.editor.preedit().is_empty() {
                    // IME preedit is differentiated; native composition still owns candidates.
                    let range = form.editor.preedit_columns();
                    let start = range
                        .start
                        .saturating_sub(form.editor.scroll_columns)
                        .min(usize::from(edit.width)) as u16;
                    let end = range
                        .end
                        .saturating_sub(form.editor.scroll_columns)
                        .min(usize::from(edit.width)) as u16;
                    for x in edit.x + start..edit.x + end {
                        self.staging[(x, edit.y)]
                            .set_style(Style::default().add_modifier(Modifier::UNDERLINED));
                    }
                }
                if self.caret_visible && self.window_focused && edit.width > 0 {
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
                            .unwrap_or_else(|| "Enter saves · Esc cancels".into()),
                        if form.error.is_some() {
                            self.theme.base().fg(self.theme.danger)
                        } else {
                            self.theme.muted()
                        },
                    );
                }
                if inner.height > 3 {
                    let row = inner.bottom() - 1;
                    let save = Rect::new(inner.x, row, inner.width.min(8), 1);
                    let cancel =
                        Rect::new(save.right(), row, inner.width.saturating_sub(save.width), 1);
                    self.write(save, "[ Save ]", self.theme.accent());
                    self.hit(ElementId::Submit, save, "Save");
                    self.write(cancel, " Cancel", self.theme.muted());
                    self.hit(ElementId::Cancel, cancel, "Cancel");
                }
            }
            Overlay::Settings { selected, scroll } => {
                if framed {
                    self.write(
                        Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
                        "Settings",
                        self.theme.accent(),
                    );
                }
                let descriptors = vtabs_core::settings::descriptors();
                let rows = usize::from(inner.height);
                if *selected < *scroll {
                    *scroll = *selected;
                }
                if *selected >= *scroll + rows.max(1) {
                    *scroll = selected.saturating_sub(rows.saturating_sub(1));
                }
                for (offset, field) in descriptors.iter().skip(*scroll).take(rows).enumerate() {
                    let row = Rect::new(inner.x, inner.y + offset as u16, inner.width, 1);
                    let value = model.settings.get(field.key).unwrap_or_default();
                    let label = format!(
                        "{}{}: {}",
                        if self.config_owned.contains(field.key) {
                            "⌑ "
                        } else {
                            ""
                        },
                        field.label,
                        value
                            .as_str()
                            .map_or_else(|| value.to_string(), str::to_owned)
                    );
                    let style = if offset + *scroll == *selected {
                        self.theme.accent().bg(self.theme.selected)
                    } else {
                        self.theme.base()
                    };
                    self.write(row, label, style);
                    self.hit(
                        ElementId::Setting(field.key.into()),
                        row,
                        format!(
                            "{}{}",
                            field.description,
                            if self.config_owned.contains(field.key) {
                                " (controlled by Lua configuration)"
                            } else {
                                ""
                            }
                        ),
                    );
                }
            }
        }
    }

    fn compose_tooltip(&mut self, area: Rect) {
        let Some(hit) = self
            .hovered
            .as_ref()
            .and_then(|id| self.hits.iter().find(|hit| &hit.id == id))
            .cloned()
        else {
            return;
        };
        if area.width < 8 || area.height < 4 {
            return;
        }
        let text = display_text(&hit.tooltip);
        let width = usize::from(area.width.saturating_sub(2)).max(1);
        let mut lines = 1usize;
        let mut used = 0usize;
        for word in text.split_whitespace() {
            let word_width = word.width();
            if used > 0 && used + 1 + word_width > width {
                lines += 1;
                used = 0;
            }
            used += usize::from(used > 0) + word_width;
            lines += used.saturating_sub(1) / width;
            used = used.saturating_sub(1) % width + 1;
        }
        let height = (lines + 2).min(usize::from(area.height)) as u16;
        let y = (hit.rect.bottom() + 1).min(area.bottom() - height);
        let rect = Rect::new(area.x, y, area.width, height);
        Clear.render(rect, &mut self.staging);
        Paragraph::new(text)
            .style(self.theme.base())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.theme.border)),
            )
            .wrap(Wrap { trim: true })
            .render(rect, &mut self.staging);
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
