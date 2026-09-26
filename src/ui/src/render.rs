use crate::{
    icons,
    input::display_text,
    sidebar::{Content, ICON_CELLS, ROW_INSET, Row, SURFACE_RADIUS},
    *,
};
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
    /// Returns None when a terminal repaint can reuse the previously committed UI.
    /// Resize publishes one fully composed frame; the previous buffer survives until swap.
    pub fn render(&mut self, model: &Model, area: Rect, now: Duration) -> Option<FrameUpdate> {
        self.frame.now = now;
        let resized = self.frame.buffer.area != area;
        if resized {
            self.sidebar.reveal_selection = true;
            self.frame.staging.resize(area);
            self.cancel_effects();
            self.pointer.press = None;
            self.frame.dirty = true;
        }
        if self.frame.revision != Some(model.revision) {
            if self
                .overlays
                .pending_form
                .is_some_and(|revision| revision != model.revision)
            {
                self.dismiss();
            }
            self.frame.revision = Some(model.revision);
            self.update_theme(model);
            self.settings.config_owned.clone_from(&model.config_owned);
            if self
                .frame
                .last_rail
                .is_some_and(|rail| rail != model.settings.rail)
                && motion::enabled(&model.settings)
                && self.host.focused
                && self.host.visible
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
            self.frame.last_rail = Some(model.settings.rail);
            self.sidebar.space_activity.clear();
            self.sidebar.space_activity.extend(
                model
                    .tabs
                    .values()
                    .filter(|tab| tab.unread || tab.bell)
                    .map(|tab| tab.space_id.clone()),
            );
            if self.sidebar.last_space.as_ref() != Some(&model.selected_space) {
                self.sidebar.reveal_selection = true;
                self.sidebar.scroll = 0;
                self.sidebar.settings_place = None;
                if let Some(index) = model
                    .spaces
                    .iter()
                    .position(|space| space.id == model.selected_space)
                {
                    let rows = usize::from(self.sidebar.spaces.height).max(1);
                    self.sidebar.space_scroll =
                        list::reveal_rows(self.sidebar.space_scroll, index, rows);
                }
                self.sidebar.last_space = Some(model.selected_space.clone());
            }
            if self.sidebar.last_tab != model.selected_tab {
                // Composition reveals the row once the frame's row capacity is known.
                self.sidebar.reveal_selection = true;
                self.sidebar.last_tab = model.selected_tab;
            }
            self.frame.dirty = true;
            if !motion::enabled(&model.settings) {
                self.cancel_effects();
            }
            self.prune_targets(model);
        }
        if !self.host.visible {
            return None;
        }
        if self.caret.tick(now) | self.tooltip.tick(now, self.pointer.hovered.is_some()) {
            self.frame.dirty = true;
        }
        self.advance_press(model, now);
        self.advance_drop(model, now);
        if !self.frame.dirty && self.effects.cell.is_none() {
            return self
                .effects
                .surface
                .is_some()
                .then(|| self.finish_frame(false, Vec::new(), Vec::new(), now));
        }
        self.frame.staging.resize(area);
        self.frame.staging.reset();
        self.paint.hits.clear();
        self.paint.surfaces.clear();
        self.paint.cursor = None;
        self.paint.editor_rect = Rect::default();
        self.paint.editor_shift = 0.0;
        Block::default()
            .style(self.theme.base())
            .render(area, &mut self.frame.staging);
        if area.width > 0 && area.height > 0 {
            let width = self
                .host
                .sidebar_columns
                .unwrap_or(area.width)
                .min(area.width);
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
            self.sidebar.rect = sidebar;
            self.compose_sidebar(model, sidebar);
            if self.settings.open {
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
                self.settings.rect = page;
                if page == area {
                    self.paint.hits.clear();
                }
                self.compose_settings_page(model, page);
            }
            if let Some(mut overlay) = self.overlays.current.take() {
                // Modal hit regions replace underlying targets; background clicks dismiss.
                self.paint.hits.clear();
                self.compose_overlay(model, area, &mut overlay);
                self.overlays.current = Some(overlay);
            } else if self.tooltip.shown {
                self.compose_tooltip(area);
            }
        }
        let elapsed = now
            .saturating_sub(self.frame.last)
            .min(Duration::from_millis(1000));
        if let Some(effect) = self.effects.cell.as_mut() {
            let effect_area = self.effects.cell_area.unwrap_or(area).intersection(area);
            effect.process(
                tachyonfx::Duration::from_millis(
                    elapsed.as_millis().min(u128::from(u32::MAX)) as u32
                ),
                &mut self.frame.staging,
                effect_area,
            );
            if effect.done() {
                self.effects.cell = None;
                self.effects.cell_area = None;
            }
        }
        let changed_cells = if resized {
            (area.y..area.bottom())
                .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                .collect()
        } else {
            self.frame
                .buffer
                .diff_iter(&self.frame.staging)
                .map(|(x, y, _)| (x, y))
                .collect::<Vec<_>>()
        };
        let mut dirty_rows = Vec::new();
        for &(_, y) in &changed_cells {
            if dirty_rows.last() != Some(&y) {
                dirty_rows.push(y);
            }
        }
        std::mem::swap(&mut self.frame.buffer, &mut self.frame.staging);
        self.frame.dirty = false;
        Some(self.finish_frame(resized, changed_cells, dirty_rows, now))
    }

    /// Drop previews move fast enough to keep up with the pointer, never slower than a frame or two.
    fn advance_drop(&mut self, model: &Model, now: Duration) {
        let Some(motion) = &mut self.pointer.drop_motion else {
            return;
        };
        let span = motion::span(&model.settings, 2, 3);
        let start = *motion.start.get_or_insert(now);
        let progress = motion::eased(now, start, span);
        if motion.progress != progress {
            motion.progress = progress;
            self.frame.dirty = true;
        }
    }

    /// A press shrinks its surface and a release grows it back, even for a quick click.
    fn advance_press(&mut self, model: &Model, now: Duration) {
        let Some(press) = &mut self.pointer.press else {
            return;
        };
        let span = motion::span(&model.settings, 1, 2);
        let eased = |from: Duration| motion::eased(now, from, span);
        let down = *press.down.get_or_insert(now);
        let release = press
            .up
            .as_mut()
            .map(|up| (*up.get_or_insert(now)).max(down + span));
        let level = match release {
            Some(up) if now >= up => 1.0 - eased(up),
            _ => eased(down),
        };
        let finished = release.is_some_and(|up| now >= up + span);
        press.animating = !finished && (release.is_some() || level < 1.0);
        if press.level != level || finished {
            press.level = level;
            self.frame.dirty = true;
        }
        if finished {
            self.pointer.press = None;
        }
    }

    fn finish_frame(
        &mut self,
        resized: bool,
        changed_cells: Vec<(u16, u16)>,
        dirty_rows: Vec<u16>,
        now: Duration,
    ) -> FrameUpdate {
        let mut transform = SurfaceTransform {
            translate_x: 0.0,
            opacity: 1.0,
        };
        if let Some(surface) = self.effects.surface {
            let t = motion::progress(now.saturating_sub(surface.start), surface.duration);
            transform.translate_x = motion::lerp(surface.from, surface.to, motion::ease_out(t));
            if t >= 1.0 {
                self.effects.surface = None;
            }
        }
        self.frame.last = now;
        self.frame.number = self.frame.number.wrapping_add(1);
        let editor = self.active_editor().and_then(|slot| self.editor(slot));
        let ime_rect = editor
            .filter(|_| self.paint.editor_rect.width > 0)
            .map(|editor| {
                Rect::new(
                    self.paint.editor_rect.x
                        + editor
                            .cursor_columns()
                            .saturating_sub(editor.scroll_columns)
                            .min(usize::from(self.paint.editor_rect.width - 1))
                            as u16,
                    self.paint.editor_rect.y,
                    1,
                    1,
                )
            });
        FrameUpdate {
            revision: self.frame.number,
            resized,
            changed_cells,
            dirty_rows,
            cursor: self.paint.cursor,
            cursor_shift: self.paint.editor_shift,
            ime_rect,
            transform,
        }
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
        self.theme.machines = settings
            .distro_colors
            .iter()
            .filter_map(|(os, color)| Some((os.clone(), Theme::parse_color(color)?)))
            .collect();
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
            ElementId::Pane(id, pane) | ElementId::ClosePane(id, pane) => model
                .tabs
                .get(id)
                .is_some_and(|tab| tab.panes.iter().any(|entry| entry.id == *pane)),
            ElementId::Space(id) => model.spaces.iter().any(|space| &space.id == id),
            _ => true,
        };
        if self
            .sidebar
            .rename
            .as_ref()
            .is_some_and(|rename| !model.tabs.contains_key(&rename.id))
        {
            self.sidebar.rename = None;
            self.caret.stop();
            if self.focused == Some(ElementId::Editor) {
                self.focused = None;
            }
        }
        if self
            .pointer
            .press
            .as_ref()
            .is_some_and(|press| !valid(&press.id))
        {
            self.pointer.press = None;
        }
        if self.focused.as_ref().is_some_and(|id| !valid(id)) {
            self.focused = None;
        }
        if self.pointer.hovered.as_ref().is_some_and(|id| !valid(id)) {
            self.pointer.hovered = None;
            self.tooltip.shown = false;
        }
        if self.pointer.drag.as_ref().is_some_and(|id| !valid(id)) {
            self.pointer.drag = None;
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
        if let Some(overlay) = &mut self.overlays.current {
            prune(overlay);
        }
        for overlay in &mut self.overlays.stack {
            prune(overlay);
        }
        if self
            .overlays
            .current
            .as_ref()
            .is_some_and(|overlay| match overlay {
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
            })
        {
            self.dismiss();
        }
    }

    pub(crate) fn write(&mut self, rect: Rect, text: impl Into<Line<'static>>, style: Style) {
        if rect.width > 0 && rect.height > 0 {
            Paragraph::new(text.into())
                .style(style)
                .render(rect, &mut self.frame.staging);
        }
    }
    pub(crate) fn hit(&mut self, id: ElementId, rect: Rect, tooltip: impl Into<String>) {
        if rect.width > 0 && rect.height > 0 {
            self.paint.hits.push(HitRegion {
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
        if self.pointer.hovered.as_ref() == Some(id) {
            style = style.bg(self.theme.card).fg(self.theme.accent);
        }
        style
    }

    fn compose_overlay(&mut self, _model: &Model, area: Rect, overlay: &mut Overlay) {
        if let Overlay::Menu(menu) = overlay
            && menu.search.is_some()
        {
            self.compose_palette(area, menu);
            return;
        }
        if let Overlay::Menu(menu) = overlay
            && let Some(message) = &menu.message
            && area.height >= DIALOG_ROWS
            && area.width >= 24
        {
            self.compose_dialog(area, menu, message);
            return;
        }
        let desired_height = match overlay {
            Overlay::Menu(menu) => menu
                .items
                .len()
                .max(1)
                .saturating_add(2)
                .min(usize::from(u16::MAX)) as u16,
            Overlay::Form(_) => 7,
        };
        let rect = match overlay {
            Overlay::Form(_) => centered(area, 64, desired_height),
            Overlay::Menu(menu) => match self.anchor_position() {
                Some(anchor) => anchored(area, anchor, menu_width(menu), desired_height),
                None => centered(area, menu_width(menu), desired_height),
            },
        };
        let rect = self.clear_of_rows(rect, area);
        self.overlays.rect = rect;
        Clear.render(rect, &mut self.frame.staging);
        self.rounded(rect, self.theme.background);
        let framed = rect.width >= 4 && rect.height >= 3;
        let inner = if framed {
            Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 2)
        } else {
            rect
        };
        match overlay {
            Overlay::Menu(menu) => {
                if framed {
                    self.write(
                        Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1),
                        display_text(&menu.title),
                        self.theme.accent(),
                    );
                }
                let rows = usize::from(inner.height);
                menu.scroll = list::scroll_to(menu.scroll, menu.selected, menu.items.len(), rows);
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
                self.rounded(edit, self.theme.selected);
                let editing = self.focused == Some(ElementId::Editor);
                self.compose_editor(&mut form.editor, edit, self.theme.selected, editing);
                self.hit(ElementId::Editor, edit, "Text entry");
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
                        let fill = if focused || self.pointer.hovered.as_ref() == Some(&id) {
                            self.theme.selected
                        } else {
                            self.theme.card
                        };
                        self.rounded(button, fill);
                        self.write(button, label, self.item_style(&id, false).bg(fill));
                        self.hit(id, button, label.trim());
                    }
                }
            }
        }
    }

    /// A question, what it costs, and two buttons: the accepting one preselected and last.
    fn compose_dialog(&mut self, area: Rect, menu: &Menu, message: &str) {
        let explained = !message.is_empty();
        let rect = centered(area, 46, DIALOG_ROWS - u16::from(!explained));
        self.overlays.rect = rect;
        Clear.render(rect, &mut self.frame.staging);
        self.rounded(rect, self.theme.background);
        let inner = Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 2);
        self.write(
            Rect::new(inner.x, inner.y, inner.width, 1),
            format!(
                "{:<width$}{}",
                icons::ALERT,
                display_text(&menu.title),
                width = usize::from(ICON_CELLS)
            ),
            self.theme.base(),
        );
        if explained {
            self.write(
                Rect::new(
                    inner.x + ICON_CELLS,
                    inner.y + 1,
                    inner.width - ICON_CELLS,
                    1,
                ),
                display_text(message),
                self.theme.muted(),
            );
        }
        let mut right = inner.right();
        for (at, item) in menu.items.iter().enumerate().rev() {
            let width = (display_text(&item.label).width() as u16 + 4).min(right - inner.x);
            let button = Rect::new(right - width, inner.bottom() - 2, width, 2);
            right = button.x.saturating_sub(1).max(inner.x);
            let id = ElementId::Menu(item.id.clone());
            let active = at == menu.selected || self.pointer.hovered.as_ref() == Some(&id);
            let accepts = !matches!(item.action, Action::Close);
            let fill = match (accepts, active) {
                (true, true) => self.theme.warn(self.theme.card, 45),
                (true, false) => self.theme.warn(self.theme.card, 22),
                (false, true) => self.theme.lift(self.theme.card, 12),
                (false, false) => self.theme.card,
            };
            self.surface(button, fill, SURFACE_RADIUS, ROW_INSET);
            self.write(
                Rect::new(button.x, button.y, button.width, 1),
                Line::from(display_text(&item.label)).alignment(ratatui::layout::Alignment::Center),
                self.theme.base().bg(fill).fg(if accepts && active {
                    self.theme.foreground
                } else if active {
                    self.theme.accent
                } else {
                    self.theme.muted
                }),
            );
            self.hit(id, button, "");
        }
    }

    /// Launchers share the sidebar row painter and editor.
    fn compose_palette(&mut self, area: Rect, menu: &mut Menu) {
        let Some(search) = &mut menu.search else {
            return;
        };
        let tall = area.height >= 12 && area.width >= 12;
        let line = if tall { 2 } else { 1 };
        let pad = u16::from(tall);
        let gap = u16::from(tall);
        let wanted = search.all_items.len().max(1).min(usize::from(u16::MAX / 2)) as u16;
        let rect = centered(area, 64, pad * 2 + line + gap + wanted * line);
        self.overlays.rect = rect;
        Clear.render(rect, &mut self.frame.staging);
        self.rounded(rect, self.theme.background);
        let inner = Rect::new(
            rect.x + pad,
            rect.y + pad,
            rect.width.saturating_sub(pad * 2),
            rect.height.saturating_sub(pad * 2),
        );
        let field = Rect::new(inner.x, inner.y, inner.width, line.min(inner.height));
        self.surface(field, self.theme.card, SURFACE_RADIUS, ROW_INSET);
        let lead = ICON_CELLS.min(field.width.saturating_sub(1));
        self.write(
            Rect::new(field.x + 1, field.y, lead, 1),
            icons::SEARCH,
            self.theme.muted().bg(self.theme.card),
        );
        let edit = Rect::new(
            field.x + 1 + lead,
            field.y,
            field.width.saturating_sub(lead + 2),
            1,
        );
        // The host centers first-row text of a two-row surface; marks follow it.
        self.paint.editor_shift = if field.height == 2 { 0.5 } else { 0.0 };
        self.compose_editor(&mut search.editor, edit, self.theme.card, true);
        if search.editor.display_text().is_empty() {
            self.write(
                edit,
                menu.title.clone(),
                self.theme.muted().bg(self.theme.card),
            );
        }
        self.hit(ElementId::Editor, field, "");
        let list = Rect::new(
            inner.x,
            field.bottom() + gap,
            inner.width,
            inner.bottom().saturating_sub(field.bottom() + gap),
        );
        let rows = usize::from(list.height / line).max(1);
        menu.scroll = list::scroll_to(menu.scroll, menu.selected, menu.items.len(), rows);
        if menu.items.is_empty() && list.height > 0 {
            self.write(
                Rect::new(list.x + 1, list.y, list.width.saturating_sub(2), 1),
                if search.all_items.is_empty() {
                    search.empty
                } else {
                    "No matches"
                },
                self.theme.muted(),
            );
        }
        for (offset, item) in menu.items.iter().skip(menu.scroll).take(rows).enumerate() {
            let rect = Rect::new(
                list.x,
                list.y + offset as u16 * line,
                list.width,
                line.min(list.bottom().saturating_sub(list.y + offset as u16 * line)),
            );
            if rect.is_empty() {
                break;
            }
            let layout = self.row(Row {
                id: ElementId::Menu(item.id.clone()),
                rect,
                indent: 0,
                icon: item.icon,
                icon_color: item.icon_color,
                index: item.index,
                content: Content::Label(&display_text(&item.label)),
                tooltip: None,
                selected: menu.scroll + offset == menu.selected,
                muted: !item.enabled,
                compact: false,
                trailing: None,
                field: false,
            });
            let hint = display_text(&item.hint);
            let width = (hint.width() as u16).min(layout.content.width / 2);
            self.write(
                Rect::new(layout.content.right() - width, rect.y, width, 1),
                hint,
                self.theme.muted().bg(layout.fill),
            );
        }
    }

    /// One text field painter for forms, search and inline renames.
    pub(crate) fn compose_editor(
        &mut self,
        editor: &mut TextEditor,
        rect: Rect,
        fill: ratatui::style::Color,
        active: bool,
    ) {
        self.paint.editor_rect = rect;
        editor.keep_cursor_visible(usize::from(rect.width));
        let mut column = 0;
        let text: String = editor
            .display_text()
            .graphemes(true)
            .filter(|grapheme| {
                let start = column;
                column += grapheme.width();
                start >= editor.scroll_columns
            })
            .collect();
        self.write(rect, text, self.theme.base().bg(fill));
        if !active {
            return;
        }
        self.compose_editor_marks(editor, rect);
        if self.caret.visible && self.host.focused && rect.width > 0 {
            let x = rect.x
                + editor
                    .cursor_columns()
                    .saturating_sub(editor.scroll_columns)
                    .min(usize::from(rect.width - 1)) as u16;
            self.paint.cursor = Some(Position::new(x, rect.y));
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
                let selection = Rect::new(columns.start, rect.y, columns.end - columns.start, 1);
                self.paint.surfaces.push(RoundedSurface {
                    shift_y: self.paint.editor_shift,
                    ..RoundedSurface::new(selection, self.theme.accent, 2.0, 0.0)
                });
                for x in columns {
                    self.frame.staging[(x, rect.y)].set_style(
                        Style::default()
                            .fg(self.theme.background)
                            .bg(self.theme.accent),
                    );
                }
            }
        }
        for x in visible_columns(editor.preedit_columns()) {
            self.frame.staging[(x, rect.y)]
                .set_style(Style::default().add_modifier(Modifier::UNDERLINED));
        }
    }

    fn compose_tooltip(&mut self, area: Rect) {
        let Some(hit) = self
            .pointer
            .hovered
            .as_ref()
            .and_then(|id| self.paint.hits.iter().find(|hit| &hit.id == id))
            .cloned()
        else {
            self.tooltip.hide();
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
        // A row is about two cells tall, so one row of padding matches two columns.
        let padding = if text.len() == 1 { 1 } else { 2 };
        let width = natural_width
            .min(42)
            .min(usize::from(area.width).saturating_sub(padding * 2))
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
        // The host centers a single line inside a two-row pill; longer text pads by a row.
        let height = if lines == 1 { 2 } else { lines + 2 }.min(usize::from(area.height)) as u16;
        let width = (width + padding * 2) as u16;
        let rect = self.tooltip_rect(area, hit.rect, width, height);
        Clear.render(rect, &mut self.frame.staging);
        self.rounded(rect, self.theme.card);
        let padding = padding as u16;
        let content = if lines == 1 {
            Rect::new(rect.x + padding, rect.y, rect.width - padding * 2, 1)
        } else {
            Rect::new(
                rect.x + padding,
                rect.y + 1,
                rect.width - padding * 2,
                rect.height - 2,
            )
        };
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
            .render(content, &mut self.frame.staging);
    }
}

impl SidebarUi {
    /// Sidebar hints sit beside the rail, level with their control and out of its way.
    fn tooltip_rect(&self, area: Rect, target: Rect, width: u16, height: u16) -> Rect {
        let sidebar = self.sidebar.rect;
        let beside = if sidebar.x > area.x {
            sidebar.x.checked_sub(width + 1).filter(|x| *x >= area.x)
        } else {
            Some(sidebar.right() + 1).filter(|x| x + width <= area.right())
        };
        match beside.filter(|_| sidebar.contains(target.as_position())) {
            Some(x) => Rect::new(
                x,
                target.y.min(area.bottom().saturating_sub(height)),
                width,
                height,
            ),
            None => self.clear_of_rows(
                anchored(
                    area,
                    Position::new(target.x, target.bottom().saturating_sub(1)),
                    width,
                    height,
                ),
                area,
            ),
        }
    }

    /// A row's centered label reaches into its second cell row; overlays start clear of it.
    fn clear_of_rows(&self, mut rect: Rect, area: Rect) -> Rect {
        let splits_a_row = self.paint.surfaces.iter().any(|surface| {
            surface.rect.height == 2
                && surface.rect.y + 1 == rect.y
                && surface.rect.x < rect.right()
                && rect.x < surface.rect.right()
        });
        if splits_a_row && rect.bottom() < area.bottom() {
            rect.y += 1;
        } else if splits_a_row && rect.y > area.y {
            rect.y -= 1;
        }
        rect
    }
}

/// Padding, the question, its explanation, a gap, two-row buttons, padding.
const DIALOG_ROWS: u16 = 7;

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Opens below the anchor row, flips above it when the bottom edge is closer, and stays in the area.
fn anchored(area: Rect, anchor: Position, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = anchor.x.clamp(area.x, area.right() - width);
    let anchor_y = anchor.y.clamp(area.y, area.bottom() - 1);
    let below = anchor_y + 1;
    let y = if below + height <= area.bottom() {
        below
    } else if anchor_y >= area.y + height {
        anchor_y - height
    } else {
        area.bottom() - height
    };
    Rect::new(x, y, width, height)
}

fn menu_width(menu: &Menu) -> u16 {
    let rows = menu
        .items
        .iter()
        .map(|item| display_text(&item.label).width() + item.hint.width() + 5);
    rows.chain(std::iter::once(display_text(&menu.title).width() + 2))
        .max()
        .unwrap_or(0)
        .clamp(20, 56) as u16
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
        Action::KillPane(tab, pane) => model
            .tabs
            .get(tab)
            .is_some_and(|tab| tab.panes.iter().any(|entry| entry.id == *pane)),
        Action::Confirm { action, .. } => action_exists(model, action),
        Action::Submenu { items, .. } => {
            items.iter().any(|item| action_exists(model, &item.action))
        }
        _ => true,
    }
}
