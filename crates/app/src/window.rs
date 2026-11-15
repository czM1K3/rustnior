//! The main window: header bar, image view, actions and key handling.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk::{gdk, gio, glib, prelude::*};
use rustnior_core::{DecodeRequest, DecodedImage, Decoders, Result as DecodeResult};

use crate::imageview::{ImageView, PAN_STEP};
use crate::texture::frame_to_texture;

pub type LoadResult = (PathBuf, DecodeResult<DecodedImage>);

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: gtk::ApplicationWindow,
    view: ImageView,
    message: gtk::Label,
    zoom_label: gtk::Label,
    decoders: Arc<Decoders>,
    current: RefCell<Option<PathBuf>>,
    /// Bumped on every load request; results for older generations are dropped.
    generation: Cell<u64>,
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
        window.set_child(Some(&overlay));

        let zoom_label = gtk::Label::builder()
            .width_chars(6)
            .css_classes(["numeric"])
            .build();
        window.set_titlebar(Some(&build_header(&zoom_label)));

        let this = Self {
            inner: Rc::new(Inner {
                window,
                view,
                message,
                zoom_label,
                decoders,
                current: RefCell::new(None),
                generation: Cell::new(0),
            }),
        };
        this.setup_actions(app);
        this.setup_input();
        this.inner.view.connect_view_changed(glib::clone!(
            #[weak(rename_to = inner)]
            this.inner,
            move |view| {
                let pct = (view.zoom() * 100.0).round();
                inner.zoom_label.set_text(&format!("{pct}%"));
            }
        ));
        this.show_message(Some("Open an image with Ctrl+O, or drop one here"));
        this
    }

    pub fn present(&self) {
        self.inner.window.present();
        self.inner.view.grab_focus();
    }

    /// Accepts the result of a decode started before the window existed.
    pub fn receive_early(&self, rx: async_channel::Receiver<LoadResult>) {
        let generation = self.next_generation();
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(result) = rx.recv().await {
                this.finish_load(generation, result);
            }
        });
    }

    /// Loads `path` on a worker thread.
    pub fn open(&self, path: PathBuf) {
        let generation = self.next_generation();
        let decoders = Arc::clone(&self.inner.decoders);
        let (tx, rx) = async_channel::bounded::<LoadResult>(1);
        std::thread::spawn(move || {
            let result = decoders.decode_path(&path, &DecodeRequest::default());
            let _ = tx.send_blocking((path, result));
        });
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(result) = rx.recv().await {
                this.finish_load(generation, result);
            }
        });
    }

    fn next_generation(&self) -> u64 {
        let g = self.inner.generation.get() + 1;
        self.inner.generation.set(g);
        g
    }

    fn finish_load(&self, generation: u64, (path, result): LoadResult) {
        if generation != self.inner.generation.get() {
            return;
        }
        self.set_title(&path);
        self.inner.current.replace(Some(path.clone()));
        match result {
            Ok(img) => {
                let texture = img.first_frame().map(frame_to_texture);
                self.inner.view.set_texture(texture, true);
                self.show_message(None);
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "failed to load image");
                self.inner.view.set_texture(None, true);
                self.show_message(Some(&format!("Could not open {}\n{err}", file_name(&path))));
            }
        }
    }

    fn set_title(&self, path: &Path) {
        self.inner
            .window
            .set_title(Some(&format!("{} — rustnior", file_name(path))));
    }

    fn show_message(&self, text: Option<&str>) {
        let m = &self.inner.message;
        m.set_visible(text.is_some());
        m.set_text(text.unwrap_or_default());
    }

    fn choose_file(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Images"));
        filter.add_pixbuf_formats();
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Open Image")
            .modal(true)
            .filters(&filters)
            .build();
        let this = self.clone();
        dialog.open(
            Some(&self.inner.window),
            gio::Cancellable::NONE,
            move |res| {
                if let Some(path) = res.ok().and_then(|f| f.path()) {
                    this.open(path);
                }
            },
        );
    }

    fn setup_actions(&self, app: &gtk::Application) {
        let view = &self.inner.view;
        let win = &self.inner.window;
        let add = |name: &str, accels: &[&str], f: Box<dyn Fn()>| {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate(move |_, _| f());
            win.add_action(&action);
            if !accels.is_empty() {
                app.set_accels_for_action(&format!("win.{name}"), accels);
            }
        };

        let v = view.clone();
        add(
            "zoom-in",
            &[
                "plus",
                "equal",
                "KP_Add",
                "<Control>plus",
                "<Control>equal",
                "<Control>KP_Add",
            ],
            Box::new(move || v.zoom_step(1)),
        );
        let v = view.clone();
        add(
            "zoom-out",
            &[
                "minus",
                "KP_Subtract",
                "<Control>minus",
                "<Control>KP_Subtract",
            ],
            Box::new(move || v.zoom_step(-1)),
        );
        let v = view.clone();
        add("zoom-fit", &["f", "0", "KP_0"], Box::new(move || v.fit()));
        let v = view.clone();
        add(
            "zoom-normal",
            &["1", "KP_1", "<Control>0"],
            Box::new(move || v.set_zoom(1.0)),
        );
        let v = view.clone();
        add("zoom-2", &["2", "KP_2"], Box::new(move || v.set_zoom(2.0)));
        let v = view.clone();
        add("zoom-3", &["3", "KP_3"], Box::new(move || v.set_zoom(3.0)));

        let this = self.clone();
        add(
            "open",
            &["<Control>o"],
            Box::new(move || this.choose_file()),
        );
        let w = win.clone();
        add(
            "close",
            &["<Control>w", "Escape", "q"],
            Box::new(move || w.close()),
        );
    }

    fn setup_input(&self) {
        let view = self.inner.view.clone();

        // Keyboard panning: arrows step, Shift+arrows page.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            view,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| {
                let page = state.contains(gdk::ModifierType::SHIFT_MASK);
                let (dx, dy) = match key {
                    gdk::Key::Left | gdk::Key::KP_Left => (1.0, 0.0),
                    gdk::Key::Right | gdk::Key::KP_Right => (-1.0, 0.0),
                    gdk::Key::Up | gdk::Key::KP_Up => (0.0, 1.0),
                    gdk::Key::Down | gdk::Key::KP_Down => (0.0, -1.0),
                    _ => return glib::Propagation::Proceed,
                };
                if page {
                    view.pan_pages(dx, dy);
                } else {
                    view.pan(dx * PAN_STEP, dy * PAN_STEP);
                }
                glib::Propagation::Stop
            }
        ));
        self.inner.window.add_controller(keys);

        // Wheel: zoom around the pointer. Touchpads pan, or zoom with Ctrl.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let pointer = Rc::new(Cell::new((0.0, 0.0)));
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[strong]
            pointer,
            move |_, x, y| pointer.set((x, y))
        ));
        view.add_controller(motion);
        scroll.connect_scroll(glib::clone!(
            #[weak]
            view,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |c, dx, dy| {
                let ctrl = c
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK);
                let wheel = c.unit() == gdk::ScrollUnit::Wheel;
                if wheel {
                    if dy != 0.0 {
                        view.zoom_step_at(if dy < 0.0 { 1 } else { -1 }, pointer.get());
                    }
                } else if ctrl {
                    view.zoom_by_at((-dy * 0.01).exp(), pointer.get());
                } else {
                    view.pan(-dx, -dy);
                }
                glib::Propagation::Stop
            }
        ));
        view.add_controller(scroll);

        // Drag and drop of files.
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let this = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let path = value
                .get::<gdk::FileList>()
                .ok()
                .and_then(|l| l.files().first().and_then(gio::prelude::FileExt::path));
            match path {
                Some(p) => {
                    this.open(p);
                    true
                }
                None => false,
            }
        });
        self.inner.window.add_controller(drop);
    }
}

fn build_header(zoom_label: &gtk::Label) -> gtk::HeaderBar {
    let header = gtk::HeaderBar::new();
    let button = |icon: &str, action: &str, tip: &str| {
        gtk::Button::builder()
            .icon_name(icon)
            .action_name(action)
            .tooltip_text(tip)
            .build()
    };
    header.pack_start(&button(
        "document-open-symbolic",
        "win.open",
        "Open Image (Ctrl+O)",
    ));

    let zoom = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    zoom.add_css_class("linked");
    zoom.append(&button("zoom-out-symbolic", "win.zoom-out", "Zoom Out (-)"));
    zoom.append(&button(
        "zoom-fit-best-symbolic",
        "win.zoom-fit",
        "Best Fit (F)",
    ));
    zoom.append(&button(
        "zoom-original-symbolic",
        "win.zoom-normal",
        "Normal Size (1)",
    ));
    zoom.append(&button("zoom-in-symbolic", "win.zoom-in", "Zoom In (+)"));
    header.pack_end(zoom_label);
    header.pack_end(&zoom);
    header
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
