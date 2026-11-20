//! Frosted-glass backgrounds for on-screen controls. GTK CSS has no
//! `backdrop-filter`, so [`BlurBin`] draws a blurred copy of another widget
//! (the image view) behind its child, clipped to its own rounded bounds.

use std::cell::RefCell;

use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};

/// Blur standard deviation in logical pixels.
const BLUR_RADIUS: f32 = 18.0;
/// Must match `border-radius` for `blurbin > .osd` in `theme::CSS`.
pub const CORNER_RADIUS: f32 = 12.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BlurBin {
        pub child: RefCell<Option<gtk::Widget>>,
        pub source: RefCell<Option<gtk::WidgetPaintable>>,
        pub invalidate: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BlurBin {
        const NAME: &'static str = "RustniorBlurBin";
        type Type = super::BlurBin;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
            klass.set_css_name("blurbin");
        }
    }

    impl ObjectImpl for BlurBin {
        fn dispose(&self) {
            if let (Some(p), Some(id)) = (self.source.take(), self.invalidate.take()) {
                p.disconnect(id);
            }
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for BlurBin {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            if let Some(paintable) = self.source.borrow().as_ref() {
                obj.snapshot_backdrop(paintable, snapshot);
            }
            if let Some(child) = self.child.borrow().as_ref() {
                obj.snapshot_child(child, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct BlurBin(ObjectSubclass<imp::BlurBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl BlurBin {
    /// Wraps `child`, blurring `source` behind it. `source` must not be an
    /// ancestor of the new widget (it would draw itself recursively).
    pub fn new(child: &impl IsA<gtk::Widget>, source: &impl IsA<gtk::Widget>) -> Self {
        let this: Self = glib::Object::new();
        child.as_ref().set_parent(&this);
        this.imp().child.replace(Some(child.as_ref().clone()));

        let paintable = gtk::WidgetPaintable::new(Some(source));
        // Redraw when the image view does (new image, zoom, animation).
        let weak = this.downgrade();
        let id = paintable.connect_invalidate_contents(move |_| {
            if let Some(this) = weak.upgrade()
                && this.is_drawable()
            {
                this.queue_draw();
            }
        });
        this.imp().source.replace(Some(paintable));
        this.imp().invalidate.replace(Some(id));
        this
    }

    fn snapshot_backdrop(&self, paintable: &gtk::WidgetPaintable, snapshot: &gtk::Snapshot) {
        let Some(source) = paintable.widget() else {
            return;
        };
        // Where this widget sits on the source (follows the revealer's
        // slide animation too).
        let Some(bounds) = self.compute_bounds(&source) else {
            return;
        };
        let (w, h) = (self.width() as f32, self.height() as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let area = graphene::Rect::new(0.0, 0.0, w, h);
        let corner = graphene::Size::new(CORNER_RADIUS, CORNER_RADIUS);
        let rounded = gsk::RoundedRect::new(area, corner, corner, corner, corner);
        snapshot.push_rounded_clip(&rounded);
        snapshot.push_blur(f64::from(BLUR_RADIUS));
        // Only the neighbourhood the blur reads, not the whole image.
        let margin = BLUR_RADIUS * 3.0;
        let read = area.inset_r(-margin, -margin);
        snapshot.push_clip(&read);
        snapshot.save();
        snapshot.translate(&graphene::Point::new(-bounds.x(), -bounds.y()));
        // Controls near a screen edge would blur in the transparent space
        // past it (letting the sharp image show through), so the source is
        // mirrored across its edges where the blur reads beyond them.
        let (sw, sh) = (source.width() as f32, source.height() as f32);
        let read = read.offset_r(bounds.x(), bounds.y());
        for i in [-1.0f32, 0.0, 1.0] {
            for j in [-1.0f32, 0.0, 1.0] {
                let cell = graphene::Rect::new(i * sw, j * sh, sw, sh);
                if cell.intersection(&read).is_none() {
                    continue;
                }
                snapshot.save();
                // Reflection about x = 0 (i = -1) or x = sw (i = 1); same for y.
                let (fx, fy) = (
                    if i == 0.0 { 1.0 } else { -1.0 },
                    if j == 0.0 { 1.0 } else { -1.0 },
                );
                let (tx, ty) = (
                    if i > 0.0 { 2.0 * sw } else { 0.0 },
                    if j > 0.0 { 2.0 * sh } else { 0.0 },
                );
                snapshot.translate(&graphene::Point::new(tx, ty));
                snapshot.scale(fx, fy);
                paintable.snapshot(snapshot, f64::from(sw), f64::from(sh));
                snapshot.restore();
            }
        }
        snapshot.restore();
        snapshot.pop(); // clip
        snapshot.pop(); // blur
        snapshot.pop(); // rounded clip
    }
}
