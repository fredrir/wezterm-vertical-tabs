use std::collections::BTreeMap;

use vtabs_engine::config::{
    CloseButton, HoverMode, PinnedStyle, Position, ScrollIndicator, Separator, TabHeight,
};
use vtabs_engine::scene::{
    Hover, Item, Padding, RenderCfg, RenderInput, SpaceEntry, Strip, StripButton, Toggle,
};

/// A compact, explicit sidebar model shared by layout and input integration tests.
pub fn sidebar() -> RenderInput {
    RenderInput {
        cols: 28,
        rows: 20,
        rail: false,
        items: vec![
            Item {
                tab_id: 1,
                index: 1,
                is_active: false,
                is_pinned: true,
                is_private: false,
                title: "pinned".into(),
                meta: None,
                icon: "p".into(),
                has_unseen: false,
            },
            Item {
                tab_id: 2,
                index: 2,
                is_active: true,
                is_pinned: false,
                is_private: false,
                title: "active".into(),
                meta: None,
                icon: "a".into(),
                has_unseen: false,
            },
            Item {
                tab_id: 3,
                index: 3,
                is_active: false,
                is_pinned: false,
                is_private: false,
                title: "unseen".into(),
                meta: None,
                icon: "u".into(),
                has_unseen: true,
            },
        ],
        theme: vtabs_engine::theme::resolve(&Default::default(), &Default::default(), false),
        cfg: RenderCfg {
            padding: Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            frame: false,
            position: Position::Left,
            new_tab_button: true,
            new_tab_label: "New tab".into(),
            row_gap: 0,
            separator: Separator::Gap,
            tab_height: TabHeight::Card,
            meta: false,
            meta_sep: None,
            show_index: false,
            icons: true,
            close_button: CloseButton::Hover,
            hover: HoverMode::Follow,
            pinned_style: PinnedStyle::Dense,
            scroll_indicator: ScrollIndicator::Auto,
        },
        glyphs: BTreeMap::new(),
        strip: Some(Strip {
            rows: 1,
            cols: 0,
            toggle_row: None,
            cell_w: None,
            toggle: Some(Toggle {
                row: 1,
                x: 2,
                x1: 1,
                x2: 4,
            }),
        }),
        strip_buttons: ["toggle_sidebar", "new_tab", "open_settings"]
            .map(|id| StripButton {
                id: id.into(),
                icon: None,
            })
            .into(),
        hover: None,
        drag: None,
        scroll: 0,
        focus_index: None,
        ensure_visible: None,
        footer: Vec::new(),
        spaces: Vec::new(),
        private: false,
        user_scrolled: false,
        popover: None,
    }
}

pub fn hover_close(view: &mut RenderInput) {
    view.hover = Some(Hover { x: 26, y: 7 });
}

pub fn spaces(view: &mut RenderInput) {
    view.spaces = vec![
        SpaceEntry {
            id: "home".into(),
            name: "Home".into(),
            icon: "~".into(),
            is_active: false,
            has_unseen: false,
        },
        SpaceEntry {
            id: "claude".into(),
            name: "Claude".into(),
            icon: "*".into(),
            is_active: true,
            has_unseen: false,
        },
        SpaceEntry {
            id: "pi".into(),
            name: "Pi".into(),
            icon: "@".into(),
            is_active: false,
            has_unseen: true,
        },
    ];
}

#[allow(dead_code)]
pub fn rail(view: &mut RenderInput, cols: i64) {
    view.cols = cols;
    view.rows = 16;
    view.rail = true;
    view.strip = Some(Strip {
        rows: 2,
        cols: 0,
        toggle_row: None,
        cell_w: None,
        toggle: Some(Toggle {
            row: 1,
            x: (cols + 1) / 2,
            x1: 1,
            x2: cols,
        }),
    });
}
