//! `ImageView`: a widget that draws one texture with zoom and pan.
//!
//! Scaling happens on the GPU through GSK scaled-texture nodes (mipmapped
//! when zoomed out), so zooming never touches pixel data on the CPU.

use std::cell::{Cell, RefCell};

use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};
use rustnior_core::viewport::ZoomMode;
use rustnior_core::{Orientation, Viewport};

/// Duration of smooth zoom animations, in microseconds.
const ZOOM_ANIM_US: f64 = 160_000.0;
/// Keyboard panning step, in logical pixels.
pub const PAN_STEP: f64 = 64.0;

mod imp {
    use std::sync::OnceLock;

    use glib::subclass::Signal;

    use super::*;

    pub struct ZoomAnim {
        pub from: f64,
        pub to: f64,
        pub anchor: (f64, f64),
        pub start_us: i64,
    }

    #[derive(Default)]
    pub struct ImageView {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub has_alpha: Cell<bool>,
        pub orientation: Cell<Orientation>,
        pub viewport: RefCell<Viewport>,
        pub anim: RefCell<Option<ZoomAnim>>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
        pub drag_origin: Cell<(f64, f64)>,
        pub pointer: Cell<Option<(f64, f64)>>,
        pub smooth: Cell<bool>,
        pub checkerboard: Cell<bool>,
        pub background: Cell<Option<gdk::RGBA>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ImageView {
        const NAME: &'static str = "RnImageView";
        type Type = super::ImageView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("imageview");
        }
    }

    impl ObjectImpl for ImageView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("view-changed").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.smooth.set(true);
            self.checkerboard.set(true);
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.setup_controllers();
        }
    }

    impl WidgetImpl for ImageView {
        fn measure(&self, _o: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (1, 1, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let dpr = self.obj().device_scale();
            self.viewport
                .borrow_mut()
                .set_view_size(f64::from(width), f64::from(height), dpr);
            self.obj().emit_view_changed();
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct ImageView(ObjectSubclass<imp::ImageView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ImageView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ImageView {
    pub fn new() -> Self {
        Self::default()
    }

    fn device_scale(&self) -> f64 {
        self.native()
            .and_then(|n| n.surface())
            .map(|s| s.scale())
            .unwrap_or_else(|| f64::from(self.scale_factor()))
    }

    fn emit_view_changed(&self) {
        self.emit_by_name::<()>("view-changed", &[]);
    }

    pub fn connect_view_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("view-changed", false, move |args| {
            if let Some(Ok(obj)) = args.first().map(|v| v.get::<Self>()) {
                f(&obj);
            }
            None
        })
    }

    /// Shows a new texture with the given display orientation. `reset_zoom`
    /// re-applies fit mode; otherwise the current zoom mode is kept (useful
    /// when refining a preview or swapping animation frames).
    pub fn set_texture(
        &self,
        texture: Option<gdk::Texture>,
        orientation: Orientation,
        reset_zoom: bool,
    ) {
        let imp = self.imp();
        self.stop_animation();
        imp.orientation.set(orientation);
        if let Some(t) = &texture {
            imp.has_alpha.set(texture_has_alpha(t));
            let mut vp = imp.viewport.borrow_mut();
            if reset_zoom {
                vp.fit();
            }
            let (w, h) = orientation.apply_size(t.width() as u32, t.height() as u32);
            if reset_zoom || vp.image_size() != (f64::from(w), f64::from(h)) {
                vp.set_image_size(w, h);
            }
        }
        imp.texture.replace(texture);
        self.changed();
    }

    /// Replaces the pixels without touching zoom or pan (animation frames).
    pub fn replace_texture(&self, texture: gdk::Texture) {
        self.imp().texture.replace(Some(texture));
        self.queue_draw();
    }

    pub fn orientation(&self) -> Orientation {
        self.imp().orientation.get()
    }

    /// Changes the display orientation (rotate/flip), re-fitting in fit mode.
    pub fn set_orientation(&self, orientation: Orientation) {
        let imp = self.imp();
        imp.orientation.set(orientation);
        if let Some(t) = imp.texture.borrow().as_ref() {
            let (w, h) = orientation.apply_size(t.width() as u32, t.height() as u32);
            imp.viewport.borrow_mut().set_image_size(w, h);
        }
        self.changed();
    }

    pub fn texture(&self) -> Option<gdk::Texture> {
        self.imp().texture.borrow().clone()
    }

    pub fn zoom(&self) -> f64 {
        self.imp().viewport.borrow().zoom()
    }

    pub fn zoom_mode(&self) -> ZoomMode {
        self.imp().viewport.borrow().mode()
    }

    /// True when the whole image is visible.
    pub fn image_fits(&self) -> bool {
        self.imp().viewport.borrow().fits()
    }

    pub fn set_smooth(&self, smooth: bool) {
        self.imp().smooth.set(smooth);
        self.queue_draw();
    }

    pub fn set_checkerboard(&self, on: bool) {
        self.imp().checkerboard.set(on);
        self.queue_draw();
    }

    pub fn set_background(&self, color: Option<gdk::RGBA>) {
        self.imp().background.set(color);
        self.queue_draw();
    }

    pub fn fit(&self) {
        self.stop_animation();
        self.imp().viewport.borrow_mut().fit();
        self.changed();
    }

    pub fn set_zoom(&self, zoom: f64) {
        self.stop_animation();
        self.imp().viewport.borrow_mut().set_zoom(zoom);
        self.changed();
    }

    /// Zooms one step in (`dir > 0`) or out, animated, around the pointer if
    /// it is over the view, otherwise around the centre.
    pub fn zoom_step(&self, dir: i32) {
        let anchor = self.imp().pointer.get().unwrap_or_else(|| self.center());
        self.zoom_step_at(dir, anchor);
    }

    pub fn zoom_step_at(&self, dir: i32, anchor: (f64, f64)) {
        let base = self
            .imp()
            .anim
            .borrow()
            .as_ref()
            .map_or_else(|| self.zoom(), |a| a.to);
        self.animate_zoom(Viewport::step_zoom(base, dir), anchor);
    }

    /// Multiplies the zoom by `factor` immediately (pinch, smooth scroll).
    pub fn zoom_by_at(&self, factor: f64, anchor: (f64, f64)) {
        self.stop_animation();
        let z = self.zoom() * factor;
        self.imp()
            .viewport
            .borrow_mut()
            .set_zoom_at(z, anchor.0, anchor.1);
        self.changed();
    }

    pub fn pan(&self, dx: f64, dy: f64) {
        self.imp().viewport.borrow_mut().pan(dx, dy);
        self.changed();
    }

    /// Pans by a fraction of the view size (keyboard page scrolling).
    pub fn pan_pages(&self, fx: f64, fy: f64) {
        let (w, h) = self.imp().viewport.borrow().view_size();
        self.pan(fx * w * 0.9, fy * h * 0.9);
    }

    fn center(&self) -> (f64, f64) {
        let (w, h) = self.imp().viewport.borrow().view_size();
        (w / 2.0, h / 2.0)
    }

    fn changed(&self) {
        self.emit_view_changed();
        self.queue_draw();
    }

    fn animate_zoom(&self, to: f64, anchor: (f64, f64)) {
        let imp = self.imp();
        let start_us = self.frame_clock().map_or(0, |c| c.frame_time());
        imp.anim.replace(Some(imp::ZoomAnim {
            from: self.zoom(),
            to,
            anchor,
            start_us,
        }));
        if imp.tick.borrow().is_some() {
            return;
        }
        let id = self.add_tick_callback(|view, clock| {
            let imp = view.imp();
            let Some((z, done, anchor)) = imp.anim.borrow().as_ref().map(|a| {
                let t = ((clock.frame_time() - a.start_us) as f64 / ZOOM_ANIM_US).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - t).powi(3);
                // Geometric interpolation feels uniform across zoom levels.
                let z = a.from * (a.to / a.from).powf(eased);
                (z, t >= 1.0, a.anchor)
            }) else {
                imp.tick.replace(None);
                return glib::ControlFlow::Break;
            };
            imp.viewport.borrow_mut().set_zoom_at(z, anchor.0, anchor.1);
            view.changed();
            if done {
                imp.anim.replace(None);
                imp.tick.replace(None);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        imp.tick.replace(Some(id));
    }

    fn stop_animation(&self) {
        let imp = self.imp();
        imp.anim.replace(None);
        if let Some(id) = imp.tick.take() {
            id.remove();
        }
    }

    fn setup_controllers(&self) {
        // Drag to pan (primary and middle buttons).
        let drag = gtk::GestureDrag::builder().button(0).build();
        drag.connect_drag_begin(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |g, _, _| {
                if g.current_button() == gdk::BUTTON_SECONDARY {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                view.stop_animation();
                let origin = view.imp().viewport.borrow().offset();
                view.imp().drag_origin.set(origin);
                if !view.image_fits() {
                    view.set_cursor_from_name(Some("grabbing"));
                }
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, dx, dy| {
                let (ox, oy) = view.imp().drag_origin.get();
                view.imp()
                    .viewport
                    .borrow_mut()
                    .set_offset(ox + dx, oy + dy);
                view.changed();
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, _, _| view.set_cursor(None)
        ));
        self.add_controller(drag);

        // Track the pointer so keyboard zoom can centre on it.
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, x, y| view.imp().pointer.set(Some((x, y)))
        ));
        motion.connect_leave(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_| view.imp().pointer.set(None)
        ));
        self.add_controller(motion);

        // Touchpad pinch.
        let pinch = gtk::GestureZoom::new();
        let last_scale = std::rc::Rc::new(Cell::new(1.0));
        pinch.connect_begin(glib::clone!(
            #[strong]
            last_scale,
            move |_, _| last_scale.set(1.0)
        ));
        pinch.connect_scale_changed(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[strong]
            last_scale,
            move |g, scale| {
                let anchor = g.bounding_box_center().unwrap_or_else(|| view.center());
                view.zoom_by_at(scale / last_scale.get(), anchor);
                last_scale.set(scale);
            }
        ));
        self.add_controller(pinch);
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let (w, h) = (self.width() as f32, self.height() as f32);
        if let Some(bg) = imp.background.get() {
            snapshot.append_color(&bg, &graphene::Rect::new(0.0, 0.0, w, h));
        }
        let Some(texture) = imp.texture.borrow().clone() else {
            return;
        };

        // Keep the viewport in sync with fractional-scale changes.
        let dpr = self.device_scale();
        let vp = {
            let mut vp = imp.viewport.borrow_mut();
            if (vp.dpr() - dpr).abs() > f64::EPSILON {
                let (vw, vh) = vp.view_size();
                vp.set_view_size(vw, vh, dpr);
            }
            vp.clone()
        };
        let r = vp.image_rect();
        let zoom = vp.zoom();
        // Align to device pixels so 100% is pixel-exact.
        let snap = |v: f64| ((v * dpr).round() / dpr) as f32;
        let rect = graphene::Rect::new(snap(r.x), snap(r.y), snap(r.w), snap(r.h));

        if imp.has_alpha.get() && imp.checkerboard.get() {
            append_checkerboard(snapshot, &rect);
        }
        let filter = if (zoom - 1.0).abs() < 1e-6 || !imp.smooth.get() {
            gsk::ScalingFilter::Nearest
        } else if zoom < 1.0 {
            gsk::ScalingFilter::Trilinear
        } else {
            gsk::ScalingFilter::Linear
        };
        let o = imp.orientation.get();
        if o.is_identity() {
            snapshot.append_scaled_texture(&texture, filter, &rect);
            return;
        }
        // Draw the unrotated texture centred on the origin, mirrored and
        // rotated by GSK, then moved into place.
        let (tw, th) = if o.swaps_dimensions() {
            (rect.height(), rect.width())
        } else {
            (rect.width(), rect.height())
        };
        snapshot.save();
        snapshot.translate(&graphene::Point::new(
            rect.x() + rect.width() / 2.0,
            rect.y() + rect.height() / 2.0,
        ));
        snapshot.rotate(o.rotation_degrees() as f32);
        if o.is_flipped() {
            snapshot.scale(-1.0, 1.0);
        }
        snapshot.append_scaled_texture(
            &texture,
            filter,
            &graphene::Rect::new(-tw / 2.0, -th / 2.0, tw, th),
        );
        snapshot.restore();
    }
}

fn texture_has_alpha(t: &gdk::Texture) -> bool {
    !matches!(
        t.format(),
        gdk::MemoryFormat::G8 | gdk::MemoryFormat::R8g8b8 | gdk::MemoryFormat::B8g8r8
    )
}

fn append_checkerboard(snapshot: &gtk::Snapshot, bounds: &graphene::Rect) {
    const TILE: f32 = 8.0;
    let light = gdk::RGBA::new(0.8, 0.8, 0.8, 1.0);
    let dark = gdk::RGBA::new(0.6, 0.6, 0.6, 1.0);
    let cell = graphene::Rect::new(bounds.x(), bounds.y(), TILE * 2.0, TILE * 2.0);
    snapshot.push_repeat(bounds, Some(&cell));
    snapshot.append_color(&light, &cell);
    snapshot.append_color(
        &dark,
        &graphene::Rect::new(bounds.x() + TILE, bounds.y(), TILE, TILE),
    );
    snapshot.append_color(
        &dark,
        &graphene::Rect::new(bounds.x(), bounds.y() + TILE, TILE, TILE),
    );
    snapshot.pop();
}
