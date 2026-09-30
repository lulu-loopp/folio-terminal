//! Same-folder media navigation consumes the files column's ordered listing.

use std::path::{Path, PathBuf};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::{PreviewOpenLane, files::DirListing, preview_open_lane};

/// One outstanding folder question and its eventual answer. The folder is also
/// the response key, so an answer for a previous folder cannot replace this one.
pub struct Folder {
    pub path: PathBuf,
    pub listing: Option<DirListing>,
    pub asked_by: crate::PreviewSurface,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
}

pub fn media(path: &Path) -> bool {
    matches!(
        preview_open_lane(path),
        PreviewOpenLane::Picture | PreviewOpenLane::Video
    )
}

/// Never sorts again: filtering preserves the exact order the column displays.
pub fn paths(current: &Path, listing: &DirListing) -> Vec<PathBuf> {
    let Some(parent) = current.parent().filter(|_| media(current)) else {
        return Vec::new();
    };
    let kind = preview_open_lane(current);
    listing
        .entries
        .iter()
        .filter_map(|entry| {
            let path = parent.join(&entry.name);
            (!entry.is_dir && preview_open_lane(&path) == kind).then_some(path)
        })
        .collect()
}

pub fn neighbour(current: &Path, paths: &[PathBuf], direction: Direction) -> Option<PathBuf> {
    let index = paths.iter().position(|path| path == current)?;
    let next = match direction {
        Direction::Previous => index.checked_sub(1)?,
        Direction::Next => index.checked_add(1)?,
    };
    paths.get(next).cloned()
}

pub fn key(path: &Path, key: &Key, modifiers: ModifiersState, pressed: bool) -> Option<Direction> {
    if !pressed || !modifiers.is_empty() || !media(path) {
        return None;
    }
    match key {
        Key::Named(NamedKey::ArrowLeft) => Some(Direction::Previous),
        Key::Named(NamedKey::ArrowRight) => Some(Direction::Next),
        _ => None,
    }
}

/// Hidden at the ends and until a listing arrives. Paint and hit testing share
/// these rectangles, including the small-pane guard.
pub fn buttons(
    body: [f32; 4],
    scale: f32,
    hovered: bool,
    focused: bool,
    available: [bool; 2],
) -> [Option<[f32; 4]>; 2] {
    let size = 32.0 * scale;
    let inset = 8.0 * scale;
    if !(hovered || focused) || body[2] - body[0] < 2.0 * (size + inset) || body[3] - body[1] < size
    {
        return [None, None];
    }
    let y = (body[1] + body[3] - size) * 0.5;
    let xs = [body[0] + inset, body[2] - inset - size];
    std::array::from_fn(|i| available[i].then_some([xs[i], y, xs[i] + size, y + size]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{DirEntry, compare_entries};

    fn listing(names: &[&str]) -> DirListing {
        DirListing {
            entries: names
                .iter()
                .map(|name| DirEntry {
                    name: (*name).into(),
                    is_dir: false,
                    is_symlink: false,
                })
                .collect(),
            ..DirListing::default()
        }
    }

    #[test]
    fn preview_arrows_keep_column_order_and_separate_kinds() {
        let mut listing = listing(&[
            "z.png",
            "B.JPG",
            "a.mp4",
            "a.png",
            "c.webm",
            "notes.txt",
            "dir.png",
        ]);
        listing.entries.last_mut().unwrap().is_dir = true;
        listing.entries.sort_by(compare_entries);
        assert_eq!(
            paths(Path::new("folder/B.JPG"), &listing),
            ["folder/a.png", "folder/B.JPG", "folder/z.png"].map(PathBuf::from)
        );
        assert_eq!(
            paths(Path::new("folder/a.mp4"), &listing),
            ["folder/a.mp4", "folder/c.webm"].map(PathBuf::from)
        );
        assert!(paths(Path::new("folder/notes.txt"), &listing).is_empty());
        // A future column sort must also survive unchanged: no private re-sort.
        listing.entries.reverse();
        assert_eq!(
            paths(Path::new("folder/a.png"), &listing)[0],
            PathBuf::from("folder/z.png")
        );
    }

    #[test]
    fn preview_arrows_find_current_and_stop_at_both_ends() {
        let paths = paths(
            Path::new("folder/b.png"),
            &listing(&["a.png", "b.png", "c.png"]),
        );
        assert_eq!(
            neighbour(&paths[1], &paths, Direction::Previous),
            Some(paths[0].clone())
        );
        assert_eq!(
            neighbour(&paths[1], &paths, Direction::Next),
            Some(paths[2].clone())
        );
        assert_eq!(neighbour(&paths[0], &paths, Direction::Previous), None);
        assert_eq!(neighbour(&paths[2], &paths, Direction::Next), None);
        assert_eq!(
            neighbour(Path::new("folder/missing.png"), &paths, Direction::Next),
            None
        );
        assert_eq!(neighbour(&paths[0], &paths[..1], Direction::Next), None);
    }

    #[test]
    fn preview_arrows_keys_only_step_unmodified_media_presses() {
        for path in ["a.png", "a.mp4"] {
            for (arrow, direction) in [
                (NamedKey::ArrowLeft, Direction::Previous),
                (NamedKey::ArrowRight, Direction::Next),
            ] {
                let arrow = Key::Named(arrow);
                assert_eq!(
                    key(Path::new(path), &arrow, ModifiersState::empty(), true),
                    Some(direction)
                );
                assert_eq!(
                    key(Path::new(path), &arrow, ModifiersState::empty(), false),
                    None
                );
                for modifiers in [
                    ModifiersState::SHIFT,
                    ModifiersState::CONTROL,
                    ModifiersState::ALT,
                    ModifiersState::SUPER,
                ] {
                    assert_eq!(key(Path::new(path), &arrow, modifiers, true), None);
                }
                assert_eq!(
                    key(Path::new("a.txt"), &arrow, ModifiersState::empty(), true),
                    None
                );
            }
        }
    }

    #[test]
    fn preview_arrows_affordances_share_visibility_and_end_state() {
        let body = [10.0, 20.0, 310.0, 220.0];
        assert_eq!(buttons(body, 1.0, false, false, [true, true]), [None, None]);
        assert_eq!(
            buttons(body, 1.0, true, false, [false, false]),
            [None, None]
        );
        let both = buttons(body, 1.0, true, false, [true, true]);
        assert_eq!(buttons(body, 1.0, false, true, [true, true]), both);
        assert_eq!(
            both,
            [
                Some([18.0, 104.0, 50.0, 136.0]),
                Some([270.0, 104.0, 302.0, 136.0])
            ]
        );
        assert_eq!(
            buttons(body, 1.0, true, false, [false, true]),
            [None, both[1]]
        );
        assert_eq!(
            buttons(body, 1.0, true, false, [true, false]),
            [both[0], None]
        );
        assert_eq!(
            buttons([0.0, 0.0, 50.0, 20.0], 1.0, true, false, [true, true]),
            [None, None]
        );
    }
}
