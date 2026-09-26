use crate::components::{PRESS_INSET, SURFACE_RADIUS};
use crate::{DropTarget, ElementId, Pointer, Theme};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Clear, Paragraph, Widget},
};

#[derive(Clone, Debug)]
pub struct HitRegion {
    pub id: ElementId,
    pub rect: Rect,
    pub tooltip: String,
}

#[derive(Clone, Debug)]
pub struct RoundedSurface {
    pub rect: Rect,
    pub fill: Color,
    pub radius: f32,
    pub inset: f32,
    /// Icon buttons center a square within their cells; rows keep their full extent.
    pub square: bool,
    /// Fraction of the rect's height to draw, about its center; thin bars need less than a cell.
    pub scale_y: f32,
    /// Holds one text line per cell row, so the host centers nothing beneath it.
    pub stacked: bool,
    /// Rows to move down; marks inside host-centered text follow it by half a row.
    pub shift_y: f32,
}

impl RoundedSurface {
    pub(crate) fn new(rect: Rect, fill: Color, radius: f32, inset: f32) -> Self {
        Self {
            rect,
            fill,
            radius,
            inset,
            square: false,
            scale_y: 1.0,
            stacked: false,
            shift_y: 0.0,
        }
    }
}

/// Where a text field was drawn, for pointer columns and the IME.
#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub id: ElementId,
    pub rect: Rect,
    pub shift: f32,
}

/// Everything a composition leaves beside the buffer.
#[derive(Default)]
pub(crate) struct Paint {
    pub surfaces: Vec<RoundedSurface>,
    pub hits: Vec<HitRegion>,
    pub fields: Vec<Field>,
    pub cursor: Option<Position>,
}

impl Paint {
    pub fn reset(&mut self) {
        self.surfaces.clear();
        self.hits.clear();
        self.fields.clear();
        self.cursor = None;
    }
    /// Modal surfaces replace every target beneath them.
    pub fn clear_targets(&mut self) {
        self.hits.clear();
        self.fields.clear();
    }
    pub fn field(&self, id: &ElementId) -> Option<&Field> {
        self.fields.iter().rev().find(|field| &field.id == id)
    }
}

/// What every component draws into, and the interaction state it reads.
pub(crate) struct Canvas<'a> {
    pub buf: &'a mut Buffer,
    pub paint: &'a mut Paint,
    pub theme: &'a Theme,
    pub pointer: &'a Pointer,
    pub focused: Option<&'a ElementId>,
    /// The blinking caret is in its visible phase and the window has focus.
    pub caret: bool,
}

impl Canvas<'_> {
    pub fn write(&mut self, rect: Rect, text: impl Into<Line<'static>>, style: Style) {
        if rect.width > 0 && rect.height > 0 {
            Paragraph::new(text.into())
                .style(style)
                .render(rect, self.buf);
        }
    }
    pub fn hit(&mut self, id: ElementId, rect: Rect, tooltip: impl Into<String>) {
        if rect.width > 0 && rect.height > 0 {
            self.paint.hits.push(HitRegion {
                id,
                rect,
                tooltip: tooltip.into(),
            });
        }
    }
    pub fn clear(&mut self, rect: Rect) {
        Clear.render(rect, self.buf);
    }
    /// Paints the surface's cells and asks the host to round them.
    pub fn fill(&mut self, surface: RoundedSurface) {
        let rect = surface.rect.intersection(self.buf.area);
        if rect.is_empty() {
            return;
        }
        Block::default()
            .style(Style::default().bg(surface.fill))
            .render(rect, self.buf);
        self.paint.surfaces.push(RoundedSurface { rect, ..surface });
    }
    pub fn surface(&mut self, rect: Rect, fill: Color, radius: f32, inset: f32) {
        self.fill(RoundedSurface::new(rect, fill, radius, inset));
    }
    pub fn rounded(&mut self, rect: Rect, fill: Color) {
        self.surface(rect, fill, SURFACE_RADIUS, 0.0);
    }
    /// Asks the host to draw a surface over cells that keep their own colors.
    pub fn mark(&mut self, surface: RoundedSurface) {
        self.paint.surfaces.push(surface);
    }
    pub fn set_style(&mut self, x: u16, y: u16, style: Style) {
        self.buf[(x, y)].set_style(style);
    }
    pub fn underline(&mut self, x: u16, y: u16) {
        self.set_style(x, y, Style::default().add_modifier(Modifier::UNDERLINED));
    }

    pub fn hovered(&self, id: &ElementId) -> bool {
        self.pointer.hovered.as_ref() == Some(id)
    }
    /// Controls nested in a row light the whole row.
    pub fn row_hovered(&self, id: &ElementId) -> bool {
        self.pointer.hovered.as_ref().map(ElementId::row).as_ref() == Some(id)
    }
    pub fn focused(&self, id: &ElementId) -> bool {
        self.focused == Some(id)
    }
    pub fn pressed(&self, id: &ElementId) -> bool {
        self.pointer
            .press
            .as_ref()
            .is_some_and(|press| &press.id == id)
    }
    pub fn press_inset(&self, id: &ElementId) -> f32 {
        self.pointer
            .press
            .as_ref()
            .filter(|press| &press.id == id)
            .map_or(0.0, |press| press.level * PRESS_INSET)
    }
    /// The element a drag would drop onto.
    pub fn aimed(&self, id: &ElementId) -> bool {
        match (&self.pointer.drop, id) {
            (Some(DropTarget::Into(tab)), ElementId::Tab(target)) => tab == target,
            (Some(DropTarget::Folder(folder)), ElementId::Folder(target)) => folder == target,
            (Some(DropTarget::Space(space)), ElementId::Space(target)) => space == target,
            (Some(DropTarget::NewTab), ElementId::NewTab) => true,
            _ => false,
        }
    }
    pub fn item_style(&self, id: &ElementId, selected: bool) -> Style {
        let mut style = self.theme.base();
        if selected {
            style = style
                .bg(self.theme.selected)
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD);
        }
        if self.hovered(id) {
            style = style.bg(self.theme.card).fg(self.theme.accent);
        }
        style
    }
    /// A row's centered label reaches into its second cell row; overlays start clear of it.
    pub fn clear_of_rows(&self, mut rect: Rect, area: Rect) -> Rect {
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
