//! Text ownership and replacement records for the preview editor.
use std::{
    ops::{Deref, Range},
    sync::Arc,
};

/// Short-lived read snapshots share storage. The event loop drops its snapshots
/// before the next edit; a retained snapshot still gets normal copy-on-write semantics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Text(Arc<String>);
impl From<String> for Text {
    fn from(text: String) -> Self {
        Self(Arc::new(text))
    }
}
impl Deref for Text {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl Text {
    pub fn make_mut(&mut self) -> &mut String {
        #[cfg(test)]
        if Arc::strong_count(&self.0) > 1 {
            crate::preview_typing::count("edit copied bytes", self.len());
        }
        Arc::make_mut(&mut self.0)
    }
}

/// Every mutation uses this one replacement operation. There is no untracked
/// mutable String reference on the keyboard's door.
pub trait EditTarget: Deref<Target = str> {
    fn replace(&mut self, range: Range<usize>, inserted: &str);
}
impl EditTarget for String {
    fn replace(&mut self, range: Range<usize>, inserted: &str) {
        self.replace_range(range, inserted);
    }
}

pub struct EditText<'a> {
    text: &'a mut String,
    pub change: Option<crate::preview_undo::Change>,
}
impl<'a> EditText<'a> {
    pub fn new(text: &'a mut String) -> Self {
        Self { text, change: None }
    }
}
impl Deref for EditText<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.text
    }
}
impl EditTarget for EditText<'_> {
    fn replace(&mut self, range: Range<usize>, inserted: &str) {
        assert!(
            self.change.is_none(),
            "one replacement per keyboard command"
        );
        #[cfg(test)]
        let clock = crate::preview_typing::Timer::new("undo diff");
        let removed = &self.text[range.clone()];
        self.change = crate::preview_undo::Change::between(
            removed,
            inserted,
            Default::default(),
            Default::default(),
        )
        .map(|mut change| {
            change.at += range.start;
            change
        });
        #[cfg(test)]
        drop(clock);
        self.text.replace_range(range, inserted);
    }
}

/// Byte positions are maintained from replacements, never rediscovered by an
/// arrow key. Updating positions scans inserted bytes and shifts line entries;
/// width measurement reads only the affected lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LineIndex {
    pub starts: Vec<usize>,
    widths: Vec<usize>,
    width_counts: std::collections::BTreeMap<usize, usize>,
}
impl LineIndex {
    pub fn new(text: &str) -> Self {
        #[cfg(test)]
        let _clock = crate::preview_typing::Timer::new("width/index");
        let starts = crate::preview_edit::line_starts(text);
        let widths: Vec<_> = (0..starts.len())
            .map(|line| width(crate::preview_edit::line_text(text, &starts, line)))
            .collect();
        let mut width_counts = std::collections::BTreeMap::new();
        for &width in &widths {
            *width_counts.entry(width).or_default() += 1;
        }
        Self {
            starts,
            widths,
            width_counts,
        }
    }
    pub fn max_columns(&self) -> usize {
        self.width_counts
            .last_key_value()
            .map_or(0, |(&width, _)| width)
    }
    pub fn replace(&mut self, text: &str, at: usize, removed: usize, inserted: &str) {
        #[cfg(test)]
        let _clock = crate::preview_typing::Timer::new("width/index");
        let first = crate::preview_edit::line_index(&self.starts, at);
        let last = crate::preview_edit::line_index(&self.starts, at + removed);
        for old in &self.widths[first..=last] {
            let count = self.width_counts.get_mut(old).expect("indexed width");
            *count -= 1;
            if *count == 0 {
                self.width_counts.remove(old);
            }
        }
        for start in &mut self.starts[last + 1..] {
            *start = *start - removed + inserted.len();
        }
        self.starts.splice(
            first + 1..last + 1,
            inserted
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| at + i + 1),
        );
        let new_last = first + inserted.bytes().filter(|&b| b == b'\n').count();
        let widths: Vec<_> = (first..=new_last)
            .map(|line| width(crate::preview_edit::line_text(text, &self.starts, line)))
            .collect();
        for &width in &widths {
            *self.width_counts.entry(width).or_default() += 1;
        }
        self.widths.splice(first..=last, widths);
    }
}

fn width(line: &str) -> usize {
    #[cfg(test)]
    crate::preview_typing::count("width bytes", line.len());
    bt_unicode::text_width(&crate::preview::expand_tabs(line))
}
