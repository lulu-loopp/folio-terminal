//! One window's Settings geometry. Hover/focus are paint state, not layout inputs.
use super::*;
use std::sync::Arc;

/// Owned inputs, compared by value: readers cannot forget to invalidate a mutation
/// in a setting, worker result, profile, shortcut, or disclosure.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub surface: [f32; 2],
    pub scale: f32,
    pub font_revision: u64,
    language: u64,
    schemes: u64,
    profiles_revision: u64,
    // Published font slices are immutable and retained by their owners.
    families: (usize, usize),
    cjk_families: (usize, usize),
    descriptions: Vec<&'static str>,
    rows: Vec<SettingsRow>,
    shortcuts: Vec<crate::shortcuts::ShortcutRow>,
    profiles: Vec<crate::profiles::ProfileLine>,
    scheme_files: Vec<SchemeFileLine>,
    advanced: AdvancedOpen,
    pub advanced_reveal: Option<(SettingsCategory, f32)>,
    editor: Option<EditorSubject>,
    pub values: SettingsValues,
    pub category: SettingsCategory,
    pub menu: Option<SettingsRow>,
    pub row_menu: Option<usize>,
    pub scroll: f32,
    pub menu_scroll: f32,
}

impl Inputs {
    pub fn new(
        surface: [f32; 2],
        scale: f32,
        font_revision: u64,
        panel: &SettingsPanel,
        scroll: f32,
        content: SettingsContent<'_>,
    ) -> Self {
        Self {
            surface,
            scale,
            font_revision,
            language: crate::i18n::lang_revision(),
            schemes: crate::schemes::revision(),
            profiles_revision: crate::profiles::profile_revision(),
            families: slice_identity(monospace_families()),
            cjk_families: slice_identity(cjk_families()),
            descriptions: content
                .rows
                .iter()
                .map(|row| row.description(content.values))
                .collect(),
            rows: content.rows.to_vec(),
            shortcuts: content.shortcuts.to_vec(),
            profiles: content.profiles.to_vec(),
            scheme_files: content.scheme_files.to_vec(),
            advanced: content.advanced,
            advanced_reveal: content.advanced_reveal,
            editor: content.editor,
            values: content.values.clone(),
            category: panel.category(),
            menu: panel.menu(),
            row_menu: panel.row_menu(),
            scroll,
            menu_scroll: panel.menu_scroll(),
        }
    }

    pub fn content(&self) -> SettingsContent<'_> {
        SettingsContent {
            rows: &self.rows,
            shortcuts: &self.shortcuts,
            profiles: &self.profiles,
            scheme_files: &self.scheme_files,
            advanced: self.advanced,
            advanced_reveal: self.advanced_reveal,
            editor: self.editor,
            values: &self.values,
        }
    }

    pub fn layout(&self, measure: &mut dyn FnMut(&str, f32) -> f32) -> Option<SettingsLayout> {
        layout_for_menus(
            self.surface[0],
            self.surface[1],
            self.scale,
            self.menu,
            self.row_menu,
            self.content(),
            self.category,
            self.scroll,
            self.menu_scroll,
            measure,
        )
    }
}

fn slice_identity<T>(slice: &'static [T]) -> (usize, usize) {
    (slice.as_ptr() as usize, slice.len())
}

#[derive(Default)]
pub struct Geometry {
    held: Option<(Inputs, Option<Arc<SettingsLayout>>)>,
}

impl Geometry {
    pub fn clear(&mut self) {
        self.held = None;
    }

    pub fn read(
        &mut self,
        mut inputs: Inputs,
        compute: impl FnOnce(&Inputs) -> Option<SettingsLayout>,
    ) -> Option<Arc<SettingsLayout>> {
        if let Some((held, layout)) = &mut self.held {
            if *held == inputs {
                return layout.clone();
            }
            // A layout already clamps the page offset. Writing that clamp back
            // must not lay the page out again. A picker scroll only repositions
            // its measured menu; opening it and revealing the selected item is
            // ONE complete page-layout operation.
            let (scroll, menu_scroll) = (inputs.scroll, inputs.menu_scroll);
            inputs.scroll = held.scroll;
            inputs.menu_scroll = held.menu_scroll;
            let same_content = *held == inputs;
            inputs.scroll = scroll;
            inputs.menu_scroll = menu_scroll;
            if same_content
                && let Some(layout) = layout
                && scroll.clamp(0.0, layout.max_scroll()) == held.scroll
            {
                let menu_scroll = menu_scroll.clamp(0.0, layout.menu_max_scroll());
                if menu_scroll != held.menu_scroll {
                    Arc::make_mut(layout).move_menu_to(menu_scroll);
                    held.menu_scroll = menu_scroll;
                }
                return Some(layout.clone());
            }
        }
        let layout = compute(&inputs).map(Arc::new);
        if let Some(layout) = &layout {
            inputs.scroll = inputs.scroll.clamp(0.0, layout.max_scroll());
            inputs.menu_scroll = inputs.menu_scroll.clamp(0.0, layout.menu_max_scroll());
        }
        self.held = Some((inputs, layout.clone()));
        layout
    }
}

/// The real Settings pointer handler, with only window effects supplied by the
/// host. Tests drive this same handler without constructing a GUI/GPU window.
pub trait PointerHost {
    fn settings_geometry(&mut self) -> Option<Arc<SettingsLayout>>;
    fn settings_drag(&mut self, layout: &SettingsLayout, x: f64, y: f64) -> anyhow::Result<bool>;
    fn settings_values(&self) -> SettingsValues;
    fn settings_hover(&mut self, target: SettingsTarget) -> anyhow::Result<()>;
}

pub fn pointer_moved(host: &mut impl PointerHost, x: f64, y: f64) -> anyhow::Result<bool> {
    let Some(layout) = host.settings_geometry() else {
        return Ok(false);
    };
    if !host.settings_drag(&layout, x, y)? {
        let target = hit(&layout, &host.settings_values(), x, y);
        host.settings_hover(target)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        let rows = visible_rows(TabLayoutMode::Horizontal);
        let values = SettingsValues::sample();
        let content = SettingsContent {
            rows: &rows,
            shortcuts: &[],
            profiles: &[],
            scheme_files: &[],
            advanced: AdvancedOpen::default(),
            advanced_reveal: None,
            editor: None,
            values: &values,
        };
        let mut panel = SettingsPanel::default();
        panel.toggle(content);
        Inputs::new([1200.0, 900.0], 1.0, 0, &panel, 0.0, content)
    }

    fn read(owner: &mut Geometry, input: &Inputs) -> Option<Arc<SettingsLayout>> {
        owner.read(input.clone(), |input| {
            input.layout(&mut |text, size| text.len() as f32 * size / 2.0)
        })
    }

    #[test]
    fn settings_geometry_inputs_invalidate_once_and_readers_share_the_value() {
        let mut owner = Geometry::default();
        let mut input = inputs();
        LAYOUT_CALLS.set(0);
        let first = read(&mut owner, &input).unwrap();
        let second = read(&mut owner, &input).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(LAYOUT_CALLS.get(), 1);
        let changes: &[fn(&mut Inputs)] = &[
            |i| i.surface[0] -= 100.0,
            |i| i.scale = 1.25,
            |i| i.font_revision += 1,
            |i| i.language += 1,
            |i| i.schemes += 1,
            |i| i.profiles_revision += 1,
            |i| i.values.key_hints = !i.values.key_hints,
            |i| i.category = SettingsCategory::Appearance,
            |i| i.menu = Some(SettingsRow::Theme),
            |i| i.menu = None,
            |i| i.row_menu = Some(0),
            |i| {
                i.advanced.set(SettingsCategory::Appearance, true);
            },
            |i| i.advanced_reveal = Some((SettingsCategory::Appearance, 0.5)),
            |i| {
                i.rows.pop().unwrap();
            },
            |i| i.descriptions.push("worker changed a row's sentence"),
            |i| {
                i.scheme_files.push(SchemeFileLine {
                    name: "custom".into(),
                    file: "custom.json".into(),
                })
            },
        ];
        for (index, change) in changes.iter().enumerate() {
            change(&mut input);
            read(&mut owner, &input);
            for _ in 0..10 {
                read(&mut owner, &input);
            }
            assert_eq!(
                LAYOUT_CALLS.get(),
                index + 2,
                "input change {index}: exactly one layout"
            );
        }
    }

    #[test]
    fn settings_geometry_font_publication_open_and_failure_budgets() {
        let mut owner = Geometry::default();
        let mut input = inputs();
        LAYOUT_CALLS.set(0);
        read(&mut owner, &input);
        // Local slots use the production publication mechanism, without changing
        // process-global font lists underneath other parallel tests.
        let mono = MonospaceFamilySlot::new();
        let cjk = CjkFamilySlot::new();
        mono.publish(
            vec![bt_platform::MonospaceFamily {
                name: "Worker mono".into(),
                files: vec![],
            }],
            true,
        );
        cjk.publish(
            vec![bt_platform::CjkFamily {
                name: "Worker CJK".into(),
                ..Default::default()
            }],
            true,
        );
        input.families = slice_identity(mono.published());
        input.cjk_families = slice_identity(cjk.adopted());
        for _ in 0..64 {
            read(&mut owner, &input);
        }
        assert_eq!(LAYOUT_CALLS.get(), 2, "open + both adopted font lists");

        input.surface = [1.0, 1.0];
        for _ in 0..64 {
            assert!(read(&mut owner, &input).is_none());
        }
        assert_eq!(
            LAYOUT_CALLS.get(),
            3,
            "unhostable is also one computed result"
        );
        input.surface = [1200.0, 900.0];
        assert!(read(&mut owner, &input).is_some());
        assert_eq!(LAYOUT_CALLS.get(), 4, "resize restores the dialog");
        owner.clear();
        for _ in 0..64 {
            read(&mut owner, &input);
        }
        assert_eq!(
            LAYOUT_CALLS.get(),
            5,
            "close retires geometry; reopen computes once"
        );
    }

    #[test]
    fn settings_geometry_open_picker_including_selected_item_costs_one_layout() {
        let mut owner = Geometry::default();
        let mut input = inputs();
        input.category = SettingsCategory::Appearance;
        read(&mut owner, &input);
        LAYOUT_CALLS.set(0);
        input.menu = Some(SettingsRow::FontSize);
        let layout = read(&mut owner, &input).unwrap();
        let last = layout.items.len() - 1;
        input.menu_scroll = layout.menu_scroll_to_show(last, 0.0);
        assert!(
            input.menu_scroll > 0.0,
            "exercise the second reader after opening a long picker"
        );
        let scrolled = read(&mut owner, &input).unwrap();
        assert!(scrolled.shows_item(scrolled.items[last]));
        for _ in 0..32 {
            read(&mut owner, &input);
        }
        assert_eq!(LAYOUT_CALLS.get(), 1, "open + reveal selected item + draw");
        // Reuse the same placement function: scrolling must also preserve all
        // hit boxes, clipping and scrollbar geometry, not just the budget.
        let expected = input
            .layout(&mut |text, size| text.len() as f32 * size / 2.0)
            .unwrap();
        assert_eq!(*scrolled, expected);
    }

    #[test]
    fn settings_geometry_scroll_clamp_and_animation_sample_are_shared() {
        let mut owner = Geometry::default();
        let mut input = inputs();
        input.scroll = 100_000.0;
        input.advanced_reveal = Some((SettingsCategory::Appearance, 0.5));
        LAYOUT_CALLS.set(0);
        let laid = read(&mut owner, &input).unwrap();
        input.scroll = input.scroll.clamp(0.0, laid.max_scroll());
        for _ in 0..64 {
            read(&mut owner, &input);
        }
        assert_eq!(
            LAYOUT_CALLS.get(),
            1,
            "write back clamp + readers of one clock sample"
        );
        input.advanced_reveal = Some((SettingsCategory::Appearance, 0.6));
        for _ in 0..64 {
            read(&mut owner, &input);
        }
        assert_eq!(LAYOUT_CALLS.get(), 2, "the clock changes the content once");
    }
}
