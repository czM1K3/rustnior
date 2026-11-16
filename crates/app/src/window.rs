//! The main window: layout, the image collection and the load flow.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk::{glib, prelude::*};
use rustnior_core::collection::absolute;
use rustnior_core::config::ZoomPref;
use rustnior_core::{
    Collection, Config, DecodedImage, Decoders, LoadEvent, Loader, LoaderConfig, Orientation,
    Result as DecodeResult, ScanOptions,
};

use crate::animation::Animator;
use crate::imageview::ImageView;
use crate::texture::Picture;
use crate::thumbstrip::ThumbStrip;

/// Results of the decode started before the window existed (see `main`).
pub enum EarlyLoad {
    Preview(DecodedImage),
    Full(DecodeResult<DecodedImage>),
}

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
    pub animator: RefCell<Option<Rc<Animator>>>,
    /// The shown pixels are a reduced preview awaiting the full decode.
    pub preview_shown: Cell<bool>,
    /// What was opened, for rescans.
    pub opened: RefCell<Vec<PathBuf>>,
    pub monitor: RefCell<Option<gtk::gio::FileMonitor>>,
    pub rescan_timer: RefCell<Option<glib::SourceId>>,
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
    pub fn new(app: &gtk::Application, decoders: Arc<Decoders>, config: Config) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("rustnior")
            .default_width(config.window.width)
            .default_height(config.window.height)
            .maximized(config.window.maximized || config.start_maximized)
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
        // The bars paint the theme's window background themselves, so they
        // stay opaque when the window background is translucent.
        status.root.add_css_class("background");
        strip.root.add_css_class("background");
        crate::theme::install_css();
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
        let loader_config = LoaderConfig {
            cache_bytes: config.cache_mib as usize * 1024 * 1024,
            previews: true,
            ..LoaderConfig::default()
        };
        let loader = Loader::new(decoders, loader_config, move |ev| {
            let _ = tx.send_blocking(ev);
        });
        loader.set_target_size(Some((
            config.window.monitor_width,
            config.window.monitor_height,
        )));

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
                scan_options: Cell::new(ScanOptions {
                    show_hidden: config.show_hidden,
                    order: config.sort_order(),
                }),
                shown: RefCell::new(None),
                early_pending: RefCell::new(None),
                scan_generation: Cell::new(0),
                image_orientation: Cell::new(Orientation::IDENTITY),
                user_orientation: Cell::new(Orientation::IDENTITY),
                animator: RefCell::new(None),
                preview_shown: Cell::new(false),
                opened: RefCell::new(Vec::new()),
                monitor: RefCell::new(None),
                rescan_timer: RefCell::new(None),
                config: RefCell::new(config),
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
        this.apply_config();
        let weak = Rc::downgrade(&this.inner);
        this.inner.window.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                Window { inner }.update_monitor_size();
            }
        });
        this.show_message(Some("Open an image with Ctrl+O, or drop one here"));
        this
    }

    /// Decodes are sized for the monitor the window is on (previews, reduced
    /// copies of large images, SVG rasterisation).
    fn update_monitor_size(&self) {
        let w = &self.inner.window;
        let Some(surface) = w.surface() else { return };
        let Some(monitor) = WidgetExt::display(w).monitor_at_surface(&surface) else {
            return;
        };
        let g = monitor.geometry();
        let scale = monitor.scale();
        let size = (
            (f64::from(g.width()) * scale).round() as u32,
            (f64::from(g.height()) * scale).round() as u32,
        );
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        self.inner.loader.set_target_size(Some(size));
        let mut c = self.inner.config.borrow_mut();
        c.window.monitor_width = size.0;
        c.window.monitor_height = size.1;
    }

    /// Persists window geometry and settings.
    fn on_close(&self) {
        let w = &self.inner.window;
        let mut c = self.inner.config.borrow_mut();
        c.window.maximized = w.is_maximized();
        if !w.is_maximized() && !w.is_fullscreen() {
            let (width, height) = w.default_size();
            if width > 0 && height > 0 {
                c.window.width = width;
                c.window.height = height;
            }
        }
        crate::settings::save(&c);
    }

    /// Applies every preference to the UI. Called at startup and whenever
    /// the preferences dialog changes something.
    pub fn apply_config(&self) {
        let inner = &self.inner;
        let c = inner.config.borrow().clone();
        let view = &inner.view;
        view.set_smooth(c.smooth_images);
        view.set_checkerboard(c.checkerboard);
        view.set_background(
            c.background_rgba()
                .map(|(r, g, b, a)| gtk::gdk::RGBA::new(r, g, b, a)),
        );
        crate::theme::set_translucent(&inner.window, c.background_is_translucent());
        view.set_upscale_on_fit(c.zoom_mode == ZoomPref::Fit);
        inner.fs.interval.set_value(f64::from(c.slideshow_seconds));
        inner
            .loader
            .set_cache_budget(c.cache_mib as usize * 1024 * 1024);
        crate::theme::apply(c.theme);

        self.set_toolbar_visible(c.show_toolbar);
        self.set_statusbar_visible(c.show_statusbar);
        self.set_thumbnails_visible(c.show_thumbnails);
        for (name, on) in [
            ("show-toolbar", c.show_toolbar),
            ("show-statusbar", c.show_statusbar),
            ("show-thumbnails", c.show_thumbnails),
        ] {
            crate::actions::set_action_state(&inner.window, name, on);
        }

        // Hidden files or sort order changed: rebuild the collection.
        let opts = ScanOptions {
            show_hidden: c.show_hidden,
            order: c.sort_order(),
        };
        let old = inner.scan_options.replace(opts);
        if old.order != opts.order {
            inner.collection.borrow_mut().set_order(opts.order);
            self.collection_changed();
        }
        if old.show_hidden != opts.show_hidden {
            self.rescan();
        }
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
                    while let Ok(ev) = rx.recv().await {
                        match ev {
                            EarlyLoad::Preview(img) => {
                                if this.current_path().as_deref() == Some(abs.as_path()) {
                                    this.display(&abs, &Arc::new(img));
                                }
                            }
                            EarlyLoad::Full(result) => {
                                this.finish_early(abs, result);
                                break;
                            }
                        }
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

        self.watch_folder(&paths);
        self.inner.opened.replace(paths.clone());
        self.scan(paths, generation);
    }

    /// Lists `paths` on a worker thread and installs the result, keeping the
    /// current image selected (or its neighbour, if it disappeared).
    fn scan(&self, paths: Vec<PathBuf>, generation: u64) {
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
            let old_index = this.inner.collection.borrow().current_index();
            if let Some(cur) = this.current_path()
                && !scanned.select_path(&cur)
                && let Some(i) = old_index
                && !scanned.is_empty()
            {
                scanned.select(i.min(scanned.len() - 1));
            }
            this.inner.collection.replace(scanned);
            this.collection_changed();
            this.show_current();
        });
    }

    /// Re-lists the opened files/folder (after changes on disk).
    pub fn rescan(&self) {
        let paths = self.inner.opened.borrow().clone();
        if paths.is_empty() {
            return;
        }
        let generation = self.inner.scan_generation.get() + 1;
        self.inner.scan_generation.set(generation);
        self.scan(paths, generation);
    }

    /// Watches the browsed folder so added, removed and changed images show up.
    fn watch_folder(&self, paths: &[PathBuf]) {
        let dir = match paths {
            [p] if p.is_dir() => Some(absolute(p)),
            [p] => absolute(p).parent().map(Path::to_owned),
            _ => None,
        };
        let monitor = dir.and_then(|d| {
            gtk::gio::File::for_path(d)
                .monitor_directory(
                    gtk::gio::FileMonitorFlags::WATCH_MOVES,
                    gtk::gio::Cancellable::NONE,
                )
                .ok()
        });
        if let Some(m) = &monitor {
            let weak = Rc::downgrade(&self.inner);
            m.connect_changed(move |_, file, _, event| {
                let Some(inner) = weak.upgrade() else { return };
                Window { inner }.on_folder_changed(file.path(), event);
            });
        }
        if let Some(old) = self.inner.monitor.replace(monitor) {
            old.cancel();
        }
    }

    fn on_folder_changed(&self, path: Option<PathBuf>, event: gtk::gio::FileMonitorEvent) {
        use gtk::gio::FileMonitorEvent as E;
        match event {
            E::ChangesDoneHint => {
                // Contents changed: drop stale decodes; reload if on screen.
                if let Some(p) = path {
                    self.inner.loader.invalidate(&p);
                    self.inner.strip.invalidate(&p);
                    self.inner.collection.borrow_mut().refresh(&p);
                    if self.current_path().as_deref() == Some(p.as_path())
                        && !self.has_unsaved_changes()
                    {
                        self.inner.shown.replace(None);
                        self.show_current();
                    }
                }
            }
            E::Created | E::Deleted | E::MovedIn | E::MovedOut | E::Renamed => {
                // Batch bursts (copying many files) into one rescan.
                if let Some(id) = self.inner.rescan_timer.take() {
                    id.remove();
                }
                let weak = Rc::downgrade(&self.inner);
                let id = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(400),
                    move || {
                        let Some(inner) = weak.upgrade() else { return };
                        inner.rescan_timer.replace(None);
                        Window { inner }.rescan();
                    },
                );
                self.inner.rescan_timer.replace(Some(id));
            }
            _ => {}
        }
    }

    /// Resizes the window so the image is shown at 100% (within 90% of the
    /// monitor), like Viewnior's "Adjust window size".
    pub fn fit_window_to_image(&self) {
        let inner = &self.inner;
        let w = &inner.window;
        let Some(p) = self.current_path().and_then(|p| inner.loader.cached(&p)) else {
            return;
        };
        let o = inner
            .image_orientation
            .get()
            .then(inner.user_orientation.get());
        let (iw, ih) = o.apply_size(p.source_width, p.source_height);
        let dpr = w.surface().map_or(1.0, |s| s.scale());
        // Chrome = everything around the image view.
        let chrome_w = w.width() - inner.view.width();
        let chrome_h = w.height() - inner.view.height();
        let (max_w, max_h) = w
            .surface()
            .and_then(|s| WidgetExt::display(w).monitor_at_surface(&s))
            .map(|m| {
                let g = m.geometry();
                (f64::from(g.width()) * 0.9, f64::from(g.height()) * 0.9)
            })
            .unwrap_or((1600.0, 1000.0));
        let target_w = (f64::from(iw) / dpr + f64::from(chrome_w))
            .min(max_w)
            .max(320.0);
        let target_h = (f64::from(ih) / dpr + f64::from(chrome_h))
            .min(max_h)
            .max(240.0);
        if w.is_maximized() {
            w.unmaximize();
        }
        w.set_default_size(target_w as i32, target_h as i32);
        inner.view.set_zoom(1.0);
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
            let radius = usize::from(inner.config.borrow().preload);
            let mut want: Vec<PathBuf> = Vec::with_capacity(1 + 2 * radius);
            if inner.early_pending.borrow().as_ref() != Some(&entry.path) {
                want.push(entry.path.clone());
            }
            want.extend(c.neighbours(radius).into_iter().map(Path::to_owned));
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
            LoadEvent::Loaded { path, image } | LoadEvent::Preview { path, image } => {
                self.display(&path, &image);
            }
            LoadEvent::Failed { path, error } => {
                tracing::warn!(path = %path.display(), %error, "failed to load image");
                if let Some(old) = self.inner.animator.take() {
                    old.stop();
                }
                self.inner
                    .view
                    .set_picture(None, Orientation::IDENTITY, true);
                self.inner.shown.replace(Some(path.clone()));
                self.show_message(Some(&format!(
                    "Could not open {}\n{error}",
                    file_name(&path)
                )));
                self.update_status();
            }
        }
    }

    /// Shows `img` for `path`. A full image replaces a preview of the same
    /// path in place, keeping zoom and pan.
    fn display(&self, path: &Path, img: &Arc<DecodedImage>) {
        let inner = &self.inner;
        let same = inner.shown.borrow().as_deref() == Some(path) && inner.view.has_image();
        let refining = same && inner.preview_shown.get() && !img.is_preview;
        if same && !refining {
            return;
        }
        let Some(first) = img.first_frame() else {
            return;
        };
        let picture = Picture::new(
            first,
            img.reduced.as_ref(),
            (img.source_width, img.source_height),
        );
        let animator = (img.is_animated() && !img.is_preview)
            .then(|| Animator::start(Arc::clone(img), inner.view.clone()));
        if let Some(old) = inner.animator.replace(animator) {
            old.stop();
        }
        let mode = inner.config.borrow().zoom_mode;
        if refining {
            let o = img.orientation.then(inner.user_orientation.get());
            inner.view.set_picture(Some(picture), o, false);
        } else {
            inner.image_orientation.set(img.orientation);
            inner.user_orientation.set(Orientation::IDENTITY);
            inner
                .view
                .set_picture(Some(picture), img.orientation, mode != ZoomPref::LastUsed);
            if mode == ZoomPref::Normal {
                inner.view.set_zoom(1.0);
            }
        }
        inner.preview_shown.set(img.is_preview);
        inner.shown.replace(Some(path.to_owned()));
        self.show_message(None);
        self.update_status();
    }

    /// Applies a rotate/flip to the shown image (display only until saved).
    pub fn transform(&self, f: impl FnOnce(Orientation) -> Orientation) {
        let inner = &self.inner;
        if !inner.view.has_image() {
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
        self.inner.config.borrow_mut().show_thumbnails = on;
        self.inner
            .strip
            .root
            .set_visible(on && !self.inner.window.is_fullscreen());
        self.inner
            .strip
            .select(self.inner.collection.borrow().current_index());
    }

    /// Shows or hides the header bar (hidden in fullscreen anyway).
    pub fn set_toolbar_visible(&self, on: bool) {
        self.inner.config.borrow_mut().show_toolbar = on;
        self.inner
            .header
            .set_visible(on && !self.inner.window.is_fullscreen());
    }

    /// Shows or hides the status bar (always hidden in fullscreen).
    pub fn set_statusbar_visible(&self, on: bool) {
        self.inner.config.borrow_mut().show_statusbar = on;
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
            .set_picture(None, Orientation::IDENTITY, true);
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
