use super::SettingsPage;
use crate::icons;
use crate::settings::SettingTarget;
use serde_json::Value;
use vtabs_core::{Model, SettingDescriptor, Settings, distros, keybinds, padding, settings};

pub(super) const CATEGORIES: &[(&str, &str, &str)] = &[
    ("all", "All", icons::ALL),
    ("layout", "Layout", icons::LAYOUT),
    ("quick_terminal", "Quick terminal", icons::TERMINAL),
    ("appearance", "Tabs", icons::TAB),
    ("spaces", "Spaces", icons::SPACE),
    ("folders", "Folders", icons::FOLDER),
    ("theme", "Colors", icons::PALETTE),
    ("motion", "Motion", icons::MOTION),
    ("behavior", "General", icons::TUNE),
    ("keybinds", "Keybinds", icons::KEYBOARD_OUTLINE),
];

/// Settings from other groups also listed under a category, with the label they take there.
const ALSO_IN: &[(&str, &str, &str)] = &[
    (
        "theme",
        "quick_terminal_border_color",
        "Quick terminal border",
    ),
    (
        "theme",
        "quick_terminal_background_color",
        "Quick terminal background",
    ),
    ("appearance", "selected_background", "Selected background"),
    ("appearance", "hover_background", "Hover background"),
    ("appearance", "tab_foreground", "Text"),
    ("appearance", "selected_foreground", "Selected text"),
    ("appearance", "local_glyph_color", "Local glyph"),
];

pub(super) fn choice_label(choice: &str) -> String {
    let mut chars = choice.chars().map(|c| if c == '_' { ' ' } else { c });
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

pub(super) fn action(field: &SettingDescriptor) -> Option<&'static str> {
    field.key.strip_prefix("shortcut.")
}

/// The heading above each row that starts a section: groups in All, remote glyphs anywhere.
pub(super) fn sections(fields: &[&SettingDescriptor], all: bool) -> Vec<Option<&'static str>> {
    let section = |field: &SettingDescriptor| match field.key {
        key if distros::id(key).is_some() => Some("Remote glyphs"),
        _ if all => Some(
            CATEGORIES
                .iter()
                .find(|(id, ..)| *id == field.group)
                .map_or(field.group, |(_, name, _)| name),
        ),
        _ => None,
    };
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = section(field)?;
            (index == 0 || section(fields[index - 1]) != Some(name)).then_some(name)
        })
        .collect()
}

pub(super) fn heading(sections: &[Option<&str>], index: usize) -> u16 {
    match sections[index] {
        None => 0,
        Some(_) if index == 0 => 1,
        Some(_) => 2,
    }
}

pub(super) fn visible_end(
    sections: &[Option<&str>],
    start: usize,
    height: u16,
    line: u16,
) -> usize {
    let mut remaining = height;
    let mut end = start;
    for index in start..sections.len() {
        let needed = heading(sections, index) + line;
        if needed > remaining {
            break;
        }
        remaining -= needed;
        end += 1;
    }
    end
}

pub(super) struct Changed {
    current: Value,
    defaults: Value,
}

impl Changed {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            current: serde_json::to_value(settings).unwrap_or_default(),
            defaults: serde_json::to_value(Settings::default()).unwrap_or_default(),
        }
    }
    pub(super) fn contains(&self, field: &SettingDescriptor) -> bool {
        SettingTarget::of(field.key).changed(&self.current, &self.defaults)
    }
}

impl SettingsPage {
    pub(crate) fn fields(&self, model: &Model) -> Vec<&'static SettingDescriptor> {
        if let Some(chord) = &self.chord {
            return keybinds::DESCRIPTORS
                .iter()
                .filter(|field| {
                    action(field).is_some_and(|action| {
                        keybinds::bindings(&model.settings, action).contains(chord)
                    })
                })
                .collect();
        }
        let query = self.query.text().trim().to_lowercase();
        let borrowed = ALSO_IN
            .iter()
            .filter(|(category, ..)| *category == self.category)
            .filter_map(|(_, key, _)| settings::descriptor(key));
        let mut fields: Vec<_> = settings::DESCRIPTORS
            .iter()
            .filter(|field| field.key != "keybinds")
            .flat_map(|field| match field.key {
                "distro_colors" => distros::DESCRIPTORS,
                "quick_terminal_window_padding" => padding::DESCRIPTORS,
                _ => std::slice::from_ref(field),
            })
            .chain(keybinds::DESCRIPTORS.iter())
            .filter(|field| {
                self.category == "all"
                    || field.group == self.category
                    || (self.category == "keybinds" && action(field).is_some())
            })
            .chain(borrowed)
            .filter(|field| {
                query.is_empty()
                    || [field.key, self.label(field), field.description, field.group]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&query))
            })
            .collect();
        // Remote glyphs close the Colors tab, after the quick terminal colors.
        if self.category == "theme" {
            fields.sort_by_key(|field| distros::id(field.key).is_some());
        } else if self.category == "all" {
            fields.sort_by_key(|field| CATEGORIES.iter().position(|(id, ..)| *id == field.group));
        }
        fields
    }

    pub(crate) fn label(&self, field: &SettingDescriptor) -> &'static str {
        ALSO_IN
            .iter()
            .find(|(category, key, _)| *category == self.category && *key == field.key)
            .map_or(field.label, |(.., label)| label)
    }
}
