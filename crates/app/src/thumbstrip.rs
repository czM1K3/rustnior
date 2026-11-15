//! Horizontal strip of thumbnails for the current collection.
//!
//! Only list rows that are actually bound (visible) request thumbnails, and
//! the request set is replaced as the strip scrolls, so off-screen work is
//! cancelled by the loader.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gtk::{gio, glib, prelude::*};
use rustnior_core::{Collection, Decoders, Frame, LoadEvent, Loader, LoaderConfig, Thumbnailer};

use crate::texture::frame_to_texture;

/// Thumbnail box size in logical pixels.
const THUMB: i32 = 72;

pub struct ThumbStrip {
    pub root: gtk::ScrolledWindow,
    list: gtk::ListView,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    state: Rc<StripState>,
}

struct StripState {
    loader: Loader<Frame>,
    /// Bound pictures waiting for (or showing) a thumbnail, by path, with
    /// their list position.
    bound: RefCell<HashMap<PathBuf, (u32, glib::WeakRef<gtk::Picture>)>>,
    want_scheduled: Cell<bool>,
    /// Position of the current image; nearer thumbnails load first.
    current: Cell<u32>,
}

impl StripState {
    fn schedule_want(self: &Rc<Self>) {
        if self.want_scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            let Some(s) = weak.upgrade() else { return };
            s.want_scheduled.set(false);
            let cur = s.current.get();
            let mut items: Vec<(u32, PathBuf)> = s
                .bound
                .borrow()
                .iter()
                .map(|(p, (pos, _))| (pos.abs_diff(cur), p.clone()))
                .collect();
            items.sort_unstable();
            let refs: Vec<&std::path::Path> = items.iter().map(|(_, p)| p.as_path()).collect();
            s.loader.want(&refs);
        });
    }
}

impl ThumbStrip {
    pub fn new(decoders: Arc<Decoders>, scale: i32) -> Self {
        let cache = Some(glib::user_cache_dir().join("thumbnails"));
        let thumbnailer = Thumbnailer::new(decoders, cache);
        let (tx, rx) = async_channel::unbounded::<LoadEvent<Frame>>();
        let loader = Loader::with_job(
            LoaderConfig {
                threads: 2,
                cache_bytes: 96 * 1024 * 1024,
            },
            move |path, req| {
                let size = req.target_size.map_or(128, |(w, h)| w.max(h));
                thumbnailer.thumbnail(path, size, req)
            },
            move |ev| {
                let _ = tx.send_blocking(ev);
            },
        );
        let px = (THUMB * scale.max(1)) as u32;
        loader.set_target_size(Some((px, px)));

        let state = Rc::new(StripState {
            loader,
            bound: RefCell::new(HashMap::new()),
            want_scheduled: Cell::new(false),
            current: Cell::new(0),
        });

        let weak = Rc::downgrade(&state);
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                let Some(state) = weak.upgrade() else { break };
                if let LoadEvent::Loaded { path, image } = ev
                    && let Some(pic) = state
                        .bound
                        .borrow()
                        .get(&path)
                        .and_then(|(_, w)| w.upgrade())
                {
                    pic.set_paintable(Some(&frame_to_texture(&image)));
                }
            }
        });

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let pic = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Contain)
                .can_shrink(true)
                .width_request(THUMB)
                .height_request(THUMB)
                .margin_start(3)
                .margin_end(3)
                .margin_top(3)
                .margin_bottom(3)
                .build();
            item.set_child(Some(&pic));
        });
        let weak = Rc::downgrade(&state);
        factory.connect_bind(move |_, item| {
            let (Some(state), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            let (Some(pic), Some(path)) = (
                item.child().and_downcast::<gtk::Picture>(),
                item.item()
                    .and_downcast::<gio::File>()
                    .and_then(|f| f.path()),
            ) else {
                return;
            };
            pic.set_tooltip_text(path.file_name().and_then(|n| n.to_str()));
            let cached = state.loader.cached(&path);
            pic.set_paintable(cached.map(|f| frame_to_texture(&f)).as_ref());
            state
                .bound
                .borrow_mut()
                .insert(path, (item.position(), pic.downgrade()));
            state.schedule_want();
        });
        let weak = Rc::downgrade(&state);
        factory.connect_unbind(move |_, item| {
            let (Some(state), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            let pic = item.child().and_downcast::<gtk::Picture>();
            if let Some(path) = item
                .item()
                .and_downcast::<gio::File>()
                .and_then(|f| f.path())
            {
                // Another row may already have bound this path (the model was
                // replaced); only forget the entry if it is ours.
                let mut bound = state.bound.borrow_mut();
                if bound.get(&path).and_then(|(_, w)| w.upgrade()) == pic {
                    bound.remove(&path);
                }
                drop(bound);
                state.schedule_want();
            }
        });

        let store = gio::ListStore::new::<gio::File>();
        let selection = gtk::SingleSelection::builder()
            .model(&store)
            .autoselect(false)
            .can_unselect(true)
            .build();
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .orientation(gtk::Orientation::Horizontal)
            .single_click_activate(true)
            .focusable(false)
            .build();
        let root = gtk::ScrolledWindow::builder()
            .child(&list)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .visible(false)
            .build();
        Self {
            root,
            list,
            store,
            selection,
            state,
        }
    }

    /// Replaces the strip's contents with the collection's entries.
    pub fn set_collection(&self, c: &Collection) {
        let files: Vec<gio::File> = c
            .entries()
            .iter()
            .map(|e| gio::File::for_path(&e.path))
            .collect();
        self.store.splice(0, self.store.n_items(), &files);
        self.select(c.current_index());
    }

    /// Highlights and scrolls to `index` without emitting activation.
    pub fn select(&self, index: Option<usize>) {
        let pos = index
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        self.selection.set_selected(pos);
        if pos != gtk::INVALID_LIST_POSITION {
            self.state.current.set(pos);
            self.state.schedule_want();
        }
        if pos != gtk::INVALID_LIST_POSITION && self.root.is_visible() {
            self.list.scroll_to(pos, gtk::ListScrollFlags::NONE, None);
        }
    }

    /// Calls `f` with the index of a thumbnail the user clicked.
    pub fn connect_activate(&self, f: impl Fn(usize) + 'static) {
        self.list.connect_activate(move |_, pos| f(pos as usize));
    }

    /// Drops the cached thumbnail for a file that changed or was deleted.
    #[allow(dead_code)] // Used by file operations (M5).
    pub fn invalidate(&self, path: &std::path::Path) {
        self.state.loader.invalidate(path);
    }
}
