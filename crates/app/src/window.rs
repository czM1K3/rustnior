//! The main window: layout, the image collection and the load flow.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk::{glib, prelude::*};
use rustnior_core::collection::absolute;
use rustnior_core::{
    Collection, Config, DecodedImage, Decoders, LoadEvent, Loader, LoaderConfig, Orientation,
    Result as DecodeResult, ScanOptions,
};

use crate::animation::Animator;
use crate::imageview::ImageView;
use crate::texture::frame_to_texture;
use crate::thumbstrip::ThumbStrip;

/// An image decoded before the window existed (see `main`).
pub type EarlyLoad = (PathBuf, DecodeResult<DecodedImage>);

/// How many images on each side of the current one are preloaded.
const PRELOAD_RADIUS: usize = 2;

#[derive(Clone)]
pub struct Window {
    pub(crate) inner: Rc<Inner>,
}

pub(crate) struct Inner {
    pub window: gtk::ApplicationWindow,
    pub view: ImageView,
    pub message: gtk::Label,
    pub status: StatusBar,
    pub header: gtk::HeaderBar,
    pub overlay: gtk::Overlay,
    pub fs: crate::fullscreen::FsUi,
    pub strip: ThumbStrip,
    pub zoom_label: gtk::Label,
    pub loader: Loader,
    pub collection: RefCell<Collection>,
    pub scan_options: Cell<ScanOptions>,
    /// Path whose pixels are currently on screen.
    pub shown: RefCell<Option<PathBuf>>,
    /// Path being decoded by the pre-GTK early load; not requested twice.
    pub early_pending: RefCell<Option<PathBuf>>,
    /// Bumped per open request so a slow folder scan cannot clobber a newer one.
    pub scan_generation: Cell<u64>,
    /// EXIF orientation of the shown image.
    pub image_orientation: Cell<Orientation>,
    /// Rotations/flips the user applied to the shown image (unsaved).
    pub user_orientation: Cell<Orientation>,
    pub show_statusbar: Cell<bool>,
    pub show_thumbnails: Cell<bool>,
    pub animator: RefCell<Option<Rc<Animator>>>,
    pub config: RefCell<Config>,
    pub toast: Toast,
    /// Set once unsaved changes were resolved and the window may close.
    pub closing: Cell<bool>,
}

pub(crate) struct Toast {
    pub revealer: gtk::Revealer,
    label: gtk::Label,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    fn new() -> Self {
        let label = gtk::Label::builder().wrap(true).build();
        let frame = gtk::Box::builder().css_classes(["osd", "toolbar"]).build();
        frame.append(&label);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(24)
            .can_target(false)
            .child(&frame)
            .build();
        Self {
            revealer,
            label,
            timer: Rc::new(RefCell::new(None)),
        }
    }

    /// Shows `text` briefly over the image.
    pub fn show(&self, text: &str) {
        self.label.set_text(text);
        self.revealer.set_reveal_child(true);
        if let Some(id) = self.timer.take() {
            id.remove();
        }
        let revealer = self.revealer.downgrade();
        let timer = Rc::clone(&self.timer);
        let id = glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            timer.replace(None);
            if let Some(r) = revealer.upgrade() {
                r.set_reveal_child(false);
            }
        });
        self.timer.replace(Some(id));
    }
}

pub(crate) struct StatusBar {
    pub root: gtk::Box,
    pub name: gtk::Label,
    pub dims: gtk::Label,
    pub size: gtk::Label,
    pub zoom: gtk::Label,
    pub position: gtk::Label,
}

impl StatusBar {
    fn new() -> Self {
        let label = |xalign: f32| {
            gtk::Label::builder()
                .xalign(xalign)
                .css_classes(["caption", "numeric"])
                .build()
        };
        let name = label(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name.set_hexpand(true);
        let (dims, size, zoom, position) = (label(1.0), label(1.0), label(1.0), label(1.0));
        zoom.set_width_chars(6);
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(18)
            .margin_start(10)
            .margin_end(10)
            .margin_top(3)
            .margin_bottom(3)
            .build();
        for w in [&name, &dims, &size, &zoom, &position] {
            row.append(w);
        }
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&row);
        Self {
            root,
            name,
            dims,
            size,
            zoom,
            position,
        }
    }
}

impl Window {
    pub fn new(app: &gtk::Application, decoders: Arc<Decoders>) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("rustnior")
            .default_width(1000)
            .default_height(700)
            .build();

        let view = ImageView::new();
        view.set_vexpand(true);
        let message = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .visible(false)
            .css_classes(["dim-label", "title-4"])
            .build();
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&view));
        overlay.add_overlay(&message);

        let status = StatusBar::new();
        let strip = ThumbStrip::new(Arc::clone(&decoders), window.scale_factor());
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&overlay);
        content.append(&strip.root);
        content.append(&status.root);
        window.set_child(Some(&content));

        let zoom_label = gtk::Label::builder()
            .width_chars(6)
            .css_classes(["numeric"])
            .build();
        let header = crate::actions::build_header(&zoom_label);
        window.set_titlebar(Some(&header));
        let fs = crate::fullscreen::FsUi::new();
        overlay.add_overlay(&fs.revealer);
        let toast = Toast::new();
        overlay.add_overlay(&toast.revealer);

        // Loader events arrive on worker threads; forward them to the main loop.
        let (tx, rx) = async_channel::unbounded::<LoadEvent>();
        let loader = Loader::new(decoders, LoaderConfig::default(), move |ev| {
            let _ = tx.send_blocking(ev);
        });

        let this = Self {
            inner: Rc::new(Inner {
                window,
                view,
                message,
                status,
                header,
                overlay,
                fs,
                strip,
                zoom_label,
                loader,
                collection: RefCell::new(Collection::default()),
                scan_options: Cell::new(ScanOptions::default()),
                shown: RefCell::new(None),
                early_pending: RefCell::new(None),
                scan_generation: Cell::new(0),
                image_orientation: Cell::new(Orientation::IDENTITY),
                user_orientation: Cell::new(Orientation::IDENTITY),
                show_statusbar: Cell::new(true),
                show_thumbnails: Cell::new(false),
                animator: RefCell::new(None),
                config: RefCell::new(Config::default()),
                toast,
                closing: Cell::new(false),
            }),
        };

        let weak = Rc::downgrade(&this.inner);
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { break };
                Window { inner }.on_load_event(ev);
            }
        });

        // The GTK window owns our state: this handler holds the only strong
        // reference and is dropped when the window is destroyed.
        let keep_alive = this.clone();
        this.inner.window.connect_close_request(move |_| {
            if keep_alive.has_unsaved_changes() && !keep_alive.inner.closing.get() {
                keep_alive.leave_current(|w| {
                    w.inner.closing.set(true);
                    w.inner.window.close();
                });
                return glib::Propagation::Stop;
            }
            keep_alive.on_close();
            glib::Propagation::Proceed
        });

        crate::actions::setup(&this, app);
        crate::input::setup(&this);
        crate::fullscreen::setup(&this);
        let weak = Rc::downgrade(&this.inner);
        this.inner.strip.connect_activate(move |i| {
            let Some(inner) = weak.upgrade() else { return };
            Window { inner }.go(move |c| c.select(i).is_some());
        });
        this.inner.view.connect_view_changed(glib::clone!(
            #[weak(rename_to = inner)]
            this.inner,
            move |view| {
                let pct = format!("{}%", (view.zoom() * 100.0).round());
                inner.zoom_label.set_text(&pct);
                inner.status.zoom.set_text(&pct);
            }
        ));
        this.show_message(Some("Open an image with Ctrl+O, or drop one here"));
        this
    }

    fn on_close(&self) {
        tracing::debug!("window closing");
    }

    pub fn present(&self) {
        self.inner.window.present();
        self.inner.view.grab_focus();
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.inner.window
    }

    /// Opens files and/or folders, after dealing with unsaved changes.
    /// `early` is a decode of `paths[0]` started before the window existed.
    pub fn open_paths(
        &self,
        paths: Vec<PathBuf>,
        early: Option<async_channel::Receiver<EarlyLoad>>,
    ) {
        if paths.is_empty() {
            return;
        }
        self.leave_current(move |w| w.open_paths_now(paths, early));
    }

    fn open_paths_now(
        &self,
        paths: Vec<PathBuf>,
        early: Option<async_channel::Receiver<EarlyLoad>>,
    ) {
        let generation = self.inner.scan_generation.get() + 1;
        self.inner.scan_generation.set(generation);

        // A single file is shown right away; its folder is scanned meanwhile.
        if let [single] = paths.as_slice()
            && single.is_file()
        {
            let abs = absolute(single);
            if let Some(rx) = early {
                self.inner.early_pending.replace(Some(abs.clone()));
                let this = self.clone();
                glib::spawn_future_local(async move {
                    if let Ok((_, result)) = rx.recv().await {
                        this.finish_early(abs, result);
                    }
                });
            }
            self.inner
                .collection
                .replace(Collection::single(single.clone()));
            self.collection_changed();
            self.show_current();
        } else {
            self.show_message(Some("Loading…"));
        }

        let opts = self.inner.scan_options.get();
        let (tx, rx) = async_channel::bounded::<Collection>(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(Collection::from_paths(&paths, &opts));
        });
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(mut scanned) = rx.recv().await else {
                return;
            };
            if this.inner.scan_generation.get() != generation {
                return;
            }
            // Keep whatever the user navigated to while the scan ran.
            if let Some(cur) = this.current_path() {
                scanned.select_path(&cur);
            }
            this.inner.collection.replace(scanned);
            this.collection_changed();
            this.show_current();
        });
    }

    fn finish_early(&self, path: PathBuf, result: DecodeResult<DecodedImage>) {
        self.inner.early_pending.replace(None);
        match result {
            Ok(img) => {
                self.inner.loader.insert(path.clone(), Arc::new(img));
                if self.current_path().as_deref() == Some(path.as_path()) {
                    self.show_current();
                }
            }
            Err(error) => self.on_load_event(LoadEvent::Failed {
                path,
                error: Arc::new(error),
            }),
        }
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.inner
            .collection
            .borrow()
            .current()
            .map(|e| e.path.clone())
    }

    /// Moves the cursor with `f` (after dealing with unsaved changes) and
    /// shows the result.
    pub fn go(&self, f: impl FnOnce(&mut Collection) -> bool + 'static) {
        self.leave_current(move |w| {
            if f(&mut w.inner.collection.borrow_mut()) {
                w.show_current();
            }
        });
    }

    /// Moves `delta` images forward/backward (wrapping).
    pub fn navigate(&self, delta: isize) {
        tracing::debug!(delta, "navigate");
        self.go(move |c| c.step(delta).is_some());
    }

    pub fn first(&self) {
        self.go(|c| c.first().is_some());
    }

    pub fn last(&self) {
        self.go(|c| c.last().is_some());
    }

    /// Displays the current collection entry (from cache if possible) and
    /// updates which images the loader should be working on.
    pub fn show_current(&self) {
        let inner = &self.inner;
        let (path, want) = {
            let c = inner.collection.borrow();
            let Some(entry) = c.current() else {
                drop(c);
                self.clear();
                return;
            };
            let mut want: Vec<PathBuf> = Vec::with_capacity(1 + 2 * PRELOAD_RADIUS);
            if inner.early_pending.borrow().as_ref() != Some(&entry.path) {
                want.push(entry.path.clone());
            }
            want.extend(c.neighbours(PRELOAD_RADIUS).into_iter().map(Path::to_owned));
            (entry.path.clone(), want)
        };
        self.update_status();
        self.inner
            .strip
            .select(inner.collection.borrow().current_index());
        tracing::debug!(path = %path.display(), want = want.len(), "show current");

        if let Some(img) = inner.loader.cached(&path) {
            self.display(&path, &img);
        }
        let refs: Vec<&Path> = want.iter().map(PathBuf::as_path).collect();
        inner.loader.want(&refs);
        self.restart_slideshow_timer();
    }

    fn on_load_event(&self, ev: LoadEvent) {
        if self.current_path().as_deref() != Some(ev.path()) {
            return; // A preload finished; it is in the cache now.
        }
        match ev {
            LoadEvent::Loaded { path, image } => self.display(&path, &image),
            LoadEvent::Failed { path, error } => {
                tracing::warn!(path = %path.display(), %error, "failed to load image");
                if let Some(old) = self.inner.animator.take() {
                    old.stop();
                }
                self.inner
                    .view
                    .set_texture(None, Orientation::IDENTITY, true);
                self.inner.shown.replace(Some(path.clone()));
                self.show_message(Some(&format!(
                    "Could not open {}\n{error}",
                    file_name(&path)
                )));
                self.update_status();
            }
        }
    }

    fn display(&self, path: &Path, img: &Arc<DecodedImage>) {
        if self.inner.shown.borrow().as_deref() == Some(path) && self.inner.view.texture().is_some()
        {
            return;
        }
        let texture = img.first_frame().map(frame_to_texture);
        let animator = img
            .is_animated()
            .then(|| Animator::start(Arc::clone(img), self.inner.view.clone(), texture.clone()));
        if let Some(old) = self.inner.animator.replace(animator) {
            old.stop();
        }
        self.inner.image_orientation.set(img.orientation);
        self.inner.user_orientation.set(Orientation::IDENTITY);
        self.inner.view.set_texture(texture, img.orientation, true);
        self.inner.shown.replace(Some(path.to_owned()));
        self.show_message(None);
        self.update_status();
    }

    /// Applies a rotate/flip to the shown image (display only until saved).
    pub fn transform(&self, f: impl FnOnce(Orientation) -> Orientation) {
        let inner = &self.inner;
        if inner.view.texture().is_none() {
            return;
        }
        let user = f(inner.user_orientation.get());
        inner.user_orientation.set(user);
        inner
            .view
            .set_orientation(inner.image_orientation.get().then(user));
    }

    /// Rebuilds views of the whole collection (thumbnail strip).
    pub fn collection_changed(&self) {
        self.inner
            .strip
            .set_collection(&self.inner.collection.borrow());
    }

    /// Shows or hides the thumbnail strip (hidden in fullscreen).
    pub fn set_thumbnails_visible(&self, on: bool) {
        self.inner.show_thumbnails.set(on);
        self.inner
            .strip
            .root
            .set_visible(on && !self.inner.window.is_fullscreen());
        self.inner
            .strip
            .select(self.inner.collection.borrow().current_index());
    }

    /// Shows or hides the status bar (always hidden in fullscreen).
    pub fn set_statusbar_visible(&self, on: bool) {
        self.inner.show_statusbar.set(on);
        self.inner
            .status
            .root
            .set_visible(on && !self.inner.window.is_fullscreen());
    }

    fn clear(&self) {
        if let Some(old) = self.inner.animator.take() {
            old.stop();
        }
        self.inner
            .view
            .set_texture(None, Orientation::IDENTITY, true);
        self.inner.shown.replace(None);
        self.inner.window.set_title(Some("rustnior"));
        self.show_message(Some("No images to show"));
        self.update_status();
    }

    pub fn update_status(&self) {
        let inner = &self.inner;
        let c = inner.collection.borrow();
        let s = &inner.status;
        let Some(entry) = c.current() else {
            for l in [&s.name, &s.dims, &s.size, &s.position] {
                l.set_text("");
            }
            return;
        };
        inner
            .window
            .set_title(Some(&format!("{} — rustnior", entry.name)));
        s.name.set_text(&entry.name);
        s.size.set_text(&glib::format_size(entry.size));
        s.position.set_text(&format!(
            "{} / {}",
            c.current_index().map_or(0, |i| i + 1),
            c.len()
        ));
        let shown = inner.shown.borrow();
        let dims = if shown.as_deref() == Some(entry.path.as_path()) {
            inner
                .loader
                .cached(&entry.path)
                .map(|img| {
                    let (w, h) = img.oriented_size();
                    format!("{w} × {h} {}", img.format_name)
                })
                .unwrap_or_default()
        } else {
            "Loading…".to_owned()
        };
        s.dims.set_text(&dims);
    }

    pub fn show_message(&self, text: Option<&str>) {
        let m = &self.inner.message;
        m.set_visible(text.is_some());
        m.set_text(text.unwrap_or_default());
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
