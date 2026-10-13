//! Cached title controls and their pointer/drag interaction.
use super::{Input, TermWindow, UiHost};
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{
    BorderColor, ComputedElement, DisplayType, Element, ElementColors, ElementContent, Float,
    LayoutContext,
};
use crate::termwindow::{UIItem, UIItemType};
use std::time::Instant;
use window::color::LinearRgba;
use window::{MouseEvent, WindowOps};

#[derive(Default)]
pub(super) struct Chrome {
    element: Option<ComputedElement>,
    items: Vec<UIItem>,
    key: (usize, usize, usize, usize, usize, bool),
    dragging: bool,
}
impl Chrome {
    pub(super) fn element(&self) -> Option<&ComputedElement> {
        self.element.as_ref()
    }
}

impl TermWindow {
    pub(super) fn ensure_vtabs_titlebar(
        &mut self,
        ui: &mut UiHost,
        width: f32,
    ) -> anyhow::Result<()> {
        let height = self.vtabs_titlebar_height();
        if height == 0. || width <= 0. {
            ui.chrome.element = None;
            ui.chrome.items.clear();
            return Ok(());
        }
        let key = (
            width as usize,
            self.dimensions.dpi,
            self.config.generation(),
            self.shape_generation,
            self.window_state.bits() as usize,
            self.focused.is_some(),
        );
        if ui.chrome.element.is_some() && ui.chrome.key == key {
            return Ok(());
        }
        let font = self.fonts.title_font()?;
        let metrics = crate::utilsprites::RenderMetrics::with_font_metrics(&font.metrics());
        let colors = ElementColors {
            border: BorderColor::default(),
            bg: if self.vtabs_integrated_macos() {
                LinearRgba::TRANSPARENT
            } else {
                if self.focused.is_some() {
                    self.config.window_frame.active_titlebar_bg
                } else {
                    self.config.window_frame.inactive_titlebar_bg
                }
                .to_linear()
            }
            .into(),
            text: if self.focused.is_some() {
                self.config.window_frame.active_titlebar_fg
            } else {
                self.config.window_frame.inactive_titlebar_fg
            }
            .to_linear()
            .into(),
        };
        let maximized = self
            .window_state
            .intersects(window::WindowState::MAXIMIZED | window::WindowState::FULL_SCREEN);
        // AppKit owns native controls; other styles reuse WezTerm's window buttons, not tabs.
        let buttons = if self.config.integrated_title_button_style
            == window::IntegratedTitleButtonStyle::MacOsNative
        {
            Vec::new()
        } else {
            self.config
                .integrated_title_buttons
                .iter()
                .map(|button| {
                    crate::termwindow::render::window_buttons::window_button_element(
                        *button,
                        maximized,
                        &font,
                        &metrics,
                        &self.config,
                    )
                })
                .collect()
        };
        let group = Element::new(&font, ElementContent::Children(buttons))
            .colors(colors.clone())
            .float(
                if self.config.integrated_title_button_alignment
                    == window::IntegratedTitleButtonAlignment::Right
                {
                    Float::Right
                } else {
                    Float::None
                },
            );
        let border = self.get_os_border();
        let element = Element::new(&font, ElementContent::Children(vec![group]))
            .display(DisplayType::Block)
            .item_type(UIItemType::TabBar(TabBarItem::None))
            .min_width(Some(config::Dimension::Pixels(width)))
            .min_height(Some(config::Dimension::Pixels(height)))
            .colors(colors);
        let chrome = self.compute_element(
            &LayoutContext {
                height: config::DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: config::DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(
                    border.left.get() as f32,
                    border.top.get() as f32,
                    width,
                    height,
                ),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 10,
            },
            &element,
        )?;
        ui.chrome.items = chrome.ui_items();
        ui.chrome.element = Some(chrome);
        ui.chrome.key = key;
        Ok(())
    }
    pub(super) fn vtabs_titlebar_mouse(&mut self, event: &MouseEvent) -> bool {
        use window::{MouseEventKind as Kind, MousePress};
        let geometry = self.vtabs_geometry();
        let Some(window) = self.window.clone() else {
            return false;
        };
        let Some(mut ui) = self.ui_host.take() else {
            return false;
        };
        if let Err(error) = self.ensure_vtabs_titlebar(&mut ui, geometry.header_width()) {
            log::error!("window controls: {error}");
            self.ui_host = Some(ui);
            return false;
        }
        let item = ui
            .chrome
            .items
            .iter()
            .rev()
            .find(|item| item.hit_test(event.coords.x, event.coords.y))
            .cloned();
        if item.is_none() && !ui.chrome.dragging {
            let was_hovered = self.current_mouse_event.as_ref().map_or(false, |old| {
                ui.chrome
                    .items
                    .iter()
                    .any(|item| item.hit_test(old.coords.x, old.coords.y))
            });
            if was_hovered {
                self.current_mouse_event = Some(event.clone());
                window.invalidate();
            }
            self.ui_host = Some(ui);
            return false;
        }
        self.current_mouse_event = Some(event.clone());
        // Header clicks use current hit regions, including resized AppKit toolbars.
        ui.geometry = geometry;
        ui.provider.render(geometry, Instant::now());
        if ui.provider.input(Input::Mouse(event, geometry)) {
            ui.needs_commit = true;
            ui.chrome.dragging = false;
            self.window_drag_position = None;
            self.ui_host = Some(ui);
            window.invalidate();
            return true;
        }
        let action = item
            .as_ref()
            .and_then(|item| match &item.item_type {
                UIItemType::TabBar(action) => Some(action.clone()),
                _ => None,
            })
            .unwrap_or(TabBarItem::None);
        if matches!(event.kind, Kind::Press(MousePress::Left)) {
            let cell = self.render_metrics.cell_size;
            let position = wezterm_term::input::ClickPosition {
                column: (event.coords.x / cell.width.max(1)).max(0) as usize,
                row: (event.coords.y / cell.height.max(1)) as i64,
                x_pixel_offset: 0,
                y_pixel_offset: 0,
            };
            let button = wezterm_term::input::MouseButton::Left;
            self.last_mouse_click = Some(match self.last_mouse_click.take() {
                Some(click) => click.add(button, position),
                None => wezterm_term::input::LastMouseClick::new(button, position),
            });
            ui.chrome.dragging = matches!(action, TabBarItem::None);
        }
        let moving = ui.chrome.dragging && matches!(event.kind, Kind::Move);
        if matches!(event.kind, Kind::Release(MousePress::Left)) {
            ui.chrome.dragging = false;
            self.window_drag_position = None;
        }
        self.last_ui_item = item;
        self.ui_host = Some(ui);
        if moving {
            if let Some(start) = &self.window_drag_position {
                window.set_window_position(window::ScreenPoint::new(
                    event.screen_coords.x - start.coords.x,
                    event.screen_coords.y - start.coords.y,
                ));
            }
        } else if !matches!(event.kind, Kind::VertWheel(_) | Kind::HorzWheel(_)) {
            self.mouse_event_tab_bar(action, event.clone(), &window);
        }
        window.set_cursor(Some(window::CursorIcon::Default));
        window.invalidate();
        true
    }
}
