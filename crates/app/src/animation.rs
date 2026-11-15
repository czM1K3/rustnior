//! Plays multi-frame images by swapping textures on a timer.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gtk::{gdk, glib};
use rustnior_core::DecodedImage;

use crate::imageview::ImageView;
use crate::texture::frame_to_texture;

pub struct Animator {
    image: Arc<DecodedImage>,
    view: ImageView,
    /// Textures are created on first use and kept, so each frame is
    /// uploaded to the GPU only once.
    textures: RefCell<Vec<Option<gdk::Texture>>>,
    index: Cell<usize>,
    playing: Cell<bool>,
    timer: RefCell<Option<glib::SourceId>>,
}

impl Animator {
    /// Starts playing `image`, whose first frame is already on screen.
    pub fn start(
        image: Arc<DecodedImage>,
        view: ImageView,
        first: Option<gdk::Texture>,
    ) -> Rc<Self> {
        let n = image.frames.len();
        let mut textures = vec![None; n];
        if let Some(t) = first {
            textures[0] = Some(t);
        }
        let this = Rc::new(Self {
            image,
            view,
            textures: RefCell::new(textures),
            index: Cell::new(0),
            playing: Cell::new(true),
            timer: RefCell::new(None),
        });
        Self::schedule(&this);
        this
    }

    pub fn frame_count(&self) -> usize {
        self.image.frames.len()
    }

    /// `P`: pause or resume.
    pub fn toggle(self: &Rc<Self>) {
        if self.playing.get() {
            self.pause();
        } else {
            self.playing.set(true);
            Self::schedule(self);
        }
    }

    /// `J`: pause and advance one frame.
    pub fn step(&self) {
        self.pause();
        self.advance();
    }

    pub fn stop(&self) {
        self.pause();
    }

    fn pause(&self) {
        self.playing.set(false);
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }

    fn advance(&self) {
        let n = self.frame_count();
        if n == 0 {
            return;
        }
        let i = (self.index.get() + 1) % n;
        self.index.set(i);
        let texture = {
            let mut t = self.textures.borrow_mut();
            t[i].get_or_insert_with(|| frame_to_texture(&self.image.frames[i]))
                .clone()
        };
        self.view.replace_texture(texture);
    }

    fn schedule(this: &Rc<Self>) {
        let delay = this.image.frames[this.index.get()].delay;
        let weak: Weak<Self> = Rc::downgrade(this);
        let id = glib::timeout_add_local_once(delay, move || {
            let Some(a) = weak.upgrade() else { return };
            a.timer.replace(None);
            if a.playing.get() {
                a.advance();
                Self::schedule(&a);
            }
        });
        this.timer.replace(Some(id));
    }
}

impl Drop for Animator {
    fn drop(&mut self) {
        if let Some(id) = self.timer.take() {
            id.remove();
        }
    }
}
