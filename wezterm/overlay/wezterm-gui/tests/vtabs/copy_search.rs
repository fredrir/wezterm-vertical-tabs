use super::*;

const CELL: (f32, f32) = (8., 16.);
const PANE: Bounds = Bounds {
    x: 40.,
    y: 30.,
    width: 800.,
    height: 400.,
};

fn view(query: &str) -> SearchView {
    SearchView {
        query: query.into(),
        cursor: query.len(),
        editing: true,
        pattern: PatternType::CaseSmartString,
        active: None,
        matches: 0,
        searching: None,
        scroll: 0,
    }
}

fn palette() -> SearchPalette {
    let gray = |value: f32| SrgbaTuple(value, value, value, 1.);
    SearchPalette {
        background: gray(0.1),
        border: Some(gray(0.2)),
        text: gray(0.9),
        muted: gray(0.6),
        accent: SrgbaTuple(0.6, 0.7, 1., 1.),
        error: SrgbaTuple(1., 0.4, 0.4, 1.),
        active_match: (gray(0.8), gray(0.1)),
        other_match: (gray(0.3), gray(0.9)),
    }
}

fn text(line: &Line) -> String {
    line.as_str().into_owned()
}

#[test]
fn widget_sits_inside_the_top_right_of_the_pane_on_whole_pixels() {
    for dpi in [96., 144., 192.] {
        let scale = dpi / 96.;
        let cell = (CELL.0 * scale, CELL.1 * scale);
        let pane = Bounds {
            width: PANE.width * scale,
            height: PANE.height * scale,
            ..PANE
        };
        let layout = Layout::new(pane, cell, dpi, 3, 14).unwrap();
        let inset = (8. * scale).round();
        let panel = layout.panel;
        assert_eq!(panel.x + panel.width, pane.x + pane.width - inset);
        assert_eq!(panel.y, pane.y + inset);
        assert!(panel.height > cell.1);
        assert_eq!(panel.x.fract(), 0.);
        assert_eq!(panel.y.fract(), 0.);
        assert!(layout.text.0 > panel.x && layout.text.1 > panel.y);
        assert!(layout.text.1 + cell.1 < panel.y + panel.height);
        assert_eq!(
            layout.text.0 + layout.columns() as f32 * cell.0,
            panel.x + panel.width - (layout.text.0 - panel.x)
        );
    }
}

#[test]
fn field_grows_with_the_query_between_its_bounds() {
    let field = |query| Layout::new(PANE, CELL, 96., query, 14).unwrap().field;
    assert_eq!(field(0), MIN_FIELD);
    assert_eq!(field(MIN_FIELD - 1), MIN_FIELD);
    assert_eq!(field(20), 21);
    assert_eq!(field(500), MAX_FIELD);
}

#[test]
fn narrow_panes_shrink_the_field_then_drop_the_count() {
    let narrow = |columns: f32| Bounds {
        width: columns * CELL.0 + 2. * (8. + CELL.0),
        ..PANE
    };
    let layout = Layout::new(narrow(24.), CELL, 96., 30, 14).unwrap();
    assert_eq!((layout.field, layout.trail), (8, 14));
    assert!(layout.panel.x >= PANE.x);
    let layout = Layout::new(narrow(10.), CELL, 96., 30, 14).unwrap();
    assert_eq!((layout.field, layout.trail), (8, 0));
    assert!(Layout::new(narrow(2.), CELL, 96., 30, 14).is_none());
    let short = Bounds {
        height: CELL.1,
        ..PANE
    };
    assert!(Layout::new(short, CELL, 96., 3, 14).is_none());
}

#[test]
fn long_queries_scroll_only_as_far_as_the_caret_needs() {
    assert_eq!(scroll(0, 5, 5, 12), 0);
    assert_eq!(scroll(0, 30, 30, 12), 19);
    assert_eq!(scroll(19, 25, 30, 12), 19);
    assert_eq!(scroll(19, 10, 30, 12), 10);
    assert_eq!(scroll(19, 3, 3, 12), 0);
}

#[test]
fn count_reads_one_based_from_the_newest_and_waits_for_the_search() {
    assert_eq!(count(&view("")), None);
    let mut searching = view("dar");
    searching.searching = Some(400);
    assert_eq!(count(&searching), None);
    assert!(!no_results(&searching));
    let missing = view("dar");
    assert_eq!(count(&missing).as_deref(), Some("No results"));
    assert!(no_results(&missing));
    let found = SearchView {
        active: Some(1),
        matches: 7,
        searching: Some(0),
        ..view("dar")
    };
    assert_eq!(count(&found).as_deref(), Some("2 of 7"));
    assert!(!no_results(&found));
}

#[test]
fn only_types_other_than_smart_case_show_a_mode() {
    assert_eq!(mode(&PatternType::CaseSmartString), None);
    assert_eq!(mode(&PatternType::CaseSensitiveString), Some("Aa"));
    assert_eq!(mode(&PatternType::CaseInSensitiveString), Some("aa"));
    assert_eq!(mode(&PatternType::Regex), Some(".*"));
}

#[test]
fn line_shows_prompt_query_and_a_right_aligned_mode_and_count() {
    let found = SearchView {
        active: Some(1),
        matches: 3,
        pattern: PatternType::Regex,
        ..view("dar")
    };
    let layout = Layout::new(PANE, CELL, 96., 3, trail(&found)).unwrap();
    let shown = line(&found, &layout, 0, &palette());
    assert_eq!(text(&shown).len(), layout.columns());
    assert!(text(&shown).starts_with("/ dar "));
    assert!(text(&shown).ends_with(".*  2 of 3"));
    let count = shown.get_cell(layout.columns() - 1).unwrap();
    assert_eq!(
        count.attrs().foreground(),
        ColorAttribute::TrueColorWithDefaultFallback(palette().muted)
    );
    let missing = line(&view("zzz"), &layout, 0, &palette());
    let last = missing.get_cell(layout.columns() - 1).unwrap();
    assert!(text(&missing).ends_with("No results"));
    assert_eq!(
        last.attrs().foreground(),
        ColorAttribute::TrueColorWithDefaultFallback(palette().error)
    );
}

#[test]
fn scrolled_query_shows_the_columns_around_the_caret() {
    let query = "abcdefghijklmnopqrstuvwxyz";
    let layout = Layout::new(PANE, CELL, 96., 4, 14).unwrap();
    let start = scroll(0, query.len(), query.len(), layout.field);
    let shown = text(&line(&view(query), &layout, start, &palette()));
    let visible = &query[start..];
    assert_eq!(&shown[PROMPT..PROMPT + visible.len()], visible);
}

#[test]
fn highlights_use_the_palette_pairs() {
    let highlights = highlights(&palette());
    assert_eq!(
        highlights.active,
        (
            ColorAttribute::TrueColorWithDefaultFallback(palette().active_match.0),
            ColorAttribute::TrueColorWithDefaultFallback(palette().active_match.1),
        )
    );
    assert_eq!(
        highlights.other.0,
        ColorAttribute::TrueColorWithDefaultFallback(palette().other_match.0)
    );
}
