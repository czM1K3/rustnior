//! The ordered list of images being browsed and the cursor into it.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::decode::{Decoders, extension_lowercase};
use crate::natsort::natural_cmp_total;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SortOrder {
    pub key: SortKey,
    pub descending: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    pub show_hidden: bool,
    pub order: SortOrder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Entry {
    pub fn from_path(path: PathBuf) -> Self {
        let meta = std::fs::metadata(&path).ok();
        Self::with_meta(path, meta.as_ref())
    }

    fn with_meta(path: PathBuf, meta: Option<&std::fs::Metadata>) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            name,
            size: meta.map_or(0, std::fs::Metadata::len),
            modified: meta.and_then(|m| m.modified().ok()),
            path,
        }
    }
}

/// Images being browsed, in display order, with a current position.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    entries: Vec<Entry>,
    current: Option<usize>,
    order: SortOrder,
}

impl Collection {
    /// Builds a collection the way Viewnior does:
    /// * one file → every image in its folder, positioned at that file;
    /// * one folder → every image in it;
    /// * several paths → only those files (plus the contents of any folders).
    pub fn from_paths(paths: &[PathBuf], opts: &ScanOptions) -> Self {
        let paths: Vec<PathBuf> = paths.iter().map(|p| absolute(p)).collect();
        let mut entries = Vec::new();
        let mut select = None;
        match paths.as_slice() {
            [] => {}
            [single] if single.is_dir() => entries = scan_dir(single, opts),
            [single] => {
                let dir = single.parent().unwrap_or(Path::new("/"));
                entries = scan_dir(dir, opts);
                if !entries.iter().any(|e| e.path == *single) {
                    // Opened explicitly, so show it even if hidden or unknown type.
                    entries.push(Entry::from_path(single.clone()));
                }
                select = Some(single.clone());
            }
            many => {
                for p in many {
                    if p.is_dir() {
                        entries.extend(scan_dir(p, opts));
                    } else if p.exists() {
                        select.get_or_insert_with(|| p.clone());
                        entries.push(Entry::from_path(p.clone()));
                    }
                }
                entries.dedup_by(|a, b| a.path == b.path);
            }
        }
        let mut c = Self {
            entries,
            current: None,
            order: opts.order,
        };
        c.sort_entries();
        c.current = match select {
            Some(p) => c.position(&p),
            None if c.entries.is_empty() => None,
            None => Some(0),
        };
        c
    }

    /// A collection holding just one file (shown while its folder is scanned).
    pub fn single(path: PathBuf) -> Self {
        Self {
            entries: vec![Entry::from_path(absolute(&path))],
            current: Some(0),
            order: SortOrder::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&Entry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    pub fn get(&self, i: usize) -> Option<&Entry> {
        self.entries.get(i)
    }

    pub fn position(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|e| e.path == path)
    }

    pub fn order(&self) -> SortOrder {
        self.order
    }

    /// Re-sorts, keeping the current image selected.
    pub fn set_order(&mut self, order: SortOrder) {
        let cur = self.current().map(|e| e.path.clone());
        self.order = order;
        self.sort_entries();
        self.current = cur.and_then(|p| self.position(&p));
    }

    pub fn select(&mut self, i: usize) -> Option<&Entry> {
        if i < self.entries.len() {
            self.current = Some(i);
        }
        self.current()
    }

    pub fn select_path(&mut self, path: &Path) -> bool {
        match self.position(path) {
            Some(i) => {
                self.current = Some(i);
                true
            }
            None => false,
        }
    }

    /// Moves by `delta` positions, wrapping around at the ends.
    pub fn step(&mut self, delta: isize) -> Option<&Entry> {
        let i = self.offset_index(delta)?;
        self.select(i)
    }

    pub fn first(&mut self) -> Option<&Entry> {
        self.select(0)
    }

    pub fn last(&mut self) -> Option<&Entry> {
        self.select(self.entries.len().checked_sub(1)?)
    }

    /// Index `delta` positions away from the current one, wrapping.
    pub fn offset_index(&self, delta: isize) -> Option<usize> {
        let n = self.entries.len() as isize;
        let cur = self.current? as isize;
        (n > 0).then(|| (cur + delta).rem_euclid(n) as usize)
    }

    /// Paths to preload around the current image, nearest first, with the
    /// forward direction preferred: +1, -1, +2, -2, ...
    pub fn neighbours(&self, radius: usize) -> Vec<&Path> {
        let mut out: Vec<&Path> = Vec::new();
        let Some(cur) = self.current else {
            return out;
        };
        for d in 1..=radius as isize {
            for delta in [d, -d] {
                if let Some(i) = self.offset_index(delta)
                    && i != cur
                {
                    let p = self.entries[i].path.as_path();
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// Removes the current entry (e.g. after deleting the file). The cursor
    /// moves to the entry that followed it, or to the new last one.
    pub fn remove_current(&mut self) -> Option<Entry> {
        let i = self.current?;
        let removed = self.entries.remove(i);
        self.current = if self.entries.is_empty() {
            None
        } else {
            Some(i.min(self.entries.len() - 1))
        };
        Some(removed)
    }

    /// Replaces an entry's metadata after the file changed on disk.
    pub fn refresh(&mut self, path: &Path) {
        if let Some(i) = self.position(path) {
            self.entries[i] = Entry::from_path(path.to_owned());
        }
    }

    fn sort_entries(&mut self) {
        let order = self.order;
        self.entries.sort_by(|a, b| {
            let ord = match order.key {
                SortKey::Name => Ordering::Equal,
                SortKey::Modified => a.modified.cmp(&b.modified),
                SortKey::Size => a.size.cmp(&b.size),
            }
            .then_with(|| natural_cmp_total(&a.name, &b.name));
            if order.descending { ord.reverse() } else { ord }
        });
    }
}

/// Lists supported images in `dir` (not recursive). Unreadable dirs yield nothing.
pub fn scan_dir(dir: &Path, opts: &ScanOptions) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        tracing::warn!(dir = %dir.display(), "cannot read directory");
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|de| opts.show_hidden || !de.file_name().as_encoded_bytes().starts_with(b"."))
        .filter(|de| {
            extension_lowercase(Path::new(&de.file_name()))
                .is_some_and(|e| Decoders::is_supported_extension(&e))
        })
        .filter_map(|de| {
            // Follows symlinks, so linked images are included.
            let meta = std::fs::metadata(de.path()).ok()?;
            meta.is_file()
                .then(|| Entry::with_meta(de.path(), Some(&meta)))
        })
        .collect()
}

pub fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(names: &[&str]) -> Collection {
        let mut c = Collection {
            entries: names
                .iter()
                .enumerate()
                .map(|(i, n)| Entry {
                    path: PathBuf::from(format!("/x/{n}")),
                    name: (*n).to_owned(),
                    size: (names.len() - i) as u64,
                    modified: Some(
                        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i as u64),
                    ),
                })
                .collect(),
            current: Some(0),
            order: SortOrder::default(),
        };
        c.sort_entries();
        c
    }

    fn names(c: &Collection) -> Vec<&str> {
        c.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn sorts_naturally_by_default() {
        let c = fake(&["a10.jpg", "a2.jpg", "a1.jpg"]);
        assert_eq!(names(&c), ["a1.jpg", "a2.jpg", "a10.jpg"]);
    }

    #[test]
    fn sort_by_size_and_date_keeps_selection() {
        // sizes: a=3, b=2, c=1; mtimes: a=0, b=1, c=2
        let mut c = fake(&["a", "b", "c"]);
        c.select(1);
        c.set_order(SortOrder {
            key: SortKey::Size,
            descending: false,
        });
        assert_eq!(names(&c), ["c", "b", "a"]);
        assert_eq!(c.current().map(|e| e.name.as_str()), Some("b"));
        c.set_order(SortOrder {
            key: SortKey::Modified,
            descending: true,
        });
        assert_eq!(names(&c), ["c", "b", "a"]);
    }

    #[test]
    fn navigation_wraps() {
        let mut c = fake(&["a", "b", "c"]);
        assert_eq!(c.step(-1).map(|e| e.name.as_str()), Some("c"));
        assert_eq!(c.step(1).map(|e| e.name.as_str()), Some("a"));
        assert_eq!(c.last().map(|e| e.name.as_str()), Some("c"));
        assert_eq!(c.first().map(|e| e.name.as_str()), Some("a"));
    }

    #[test]
    fn neighbours_prefer_forward_and_dedupe() {
        let c = fake(&["a", "b", "c", "d", "e"]);
        let n: Vec<_> = c
            .neighbours(2)
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(n, ["/x/b", "/x/e", "/x/c", "/x/d"]);
        let small = fake(&["a", "b"]);
        assert_eq!(small.neighbours(3).len(), 1);
        assert!(fake(&["a"]).neighbours(2).is_empty());
    }

    #[test]
    fn remove_current_moves_to_next() {
        let mut c = fake(&["a", "b", "c"]);
        c.select(1);
        assert_eq!(c.remove_current().map(|e| e.name), Some("b".into()));
        assert_eq!(c.current().map(|e| e.name.as_str()), Some("c"));
        c.remove_current();
        assert_eq!(c.current().map(|e| e.name.as_str()), Some("a"));
        c.remove_current();
        assert!(c.current().is_none());
        assert!(c.remove_current().is_none());
    }
}
