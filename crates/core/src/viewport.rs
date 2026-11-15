//! Zoom/pan math for showing an image inside a view, independent of the toolkit.
//!
//! Units: view coordinates are logical pixels; `zoom` is *device* pixels per
//! image pixel, so 100% shows every image pixel on exactly one screen pixel on
//! HiDPI displays too.

pub const ZOOM_MIN: f64 = 0.01;
pub const ZOOM_MAX: f64 = 50.0;
pub const ZOOM_STEP: f64 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoomMode {
    /// Shrink to fit the view (upscale only if allowed); follows view resizes.
    #[default]
    Fit,
    /// User-chosen zoom level.
    Manual,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    image_w: f64,
    image_h: f64,
    view_w: f64,
    view_h: f64,
    dpr: f64,
    zoom: f64,
    offset_x: f64,
    offset_y: f64,
    mode: ZoomMode,
    /// Whether `Fit` may enlarge images smaller than the view.
    pub upscale_on_fit: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            image_w: 1.0,
            image_h: 1.0,
            view_w: 1.0,
            view_h: 1.0,
            dpr: 1.0,
            zoom: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            mode: ZoomMode::Fit,
            upscale_on_fit: false,
        }
    }
}

/// Rectangle in view coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Viewport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    pub fn mode(&self) -> ZoomMode {
        self.mode
    }

    pub fn image_size(&self) -> (f64, f64) {
        (self.image_w, self.image_h)
    }

    pub fn view_size(&self) -> (f64, f64) {
        (self.view_w, self.view_h)
    }

    pub fn dpr(&self) -> f64 {
        self.dpr
    }

    /// Logical pixels per image pixel.
    pub fn scale(&self) -> f64 {
        self.zoom / self.dpr
    }

    /// Where the image is drawn, in view coordinates.
    pub fn image_rect(&self) -> Rect {
        Rect {
            x: self.offset_x,
            y: self.offset_y,
            w: self.image_w * self.scale(),
            h: self.image_h * self.scale(),
        }
    }

    /// True when the whole image is visible (nothing to pan).
    pub fn fits(&self) -> bool {
        let r = self.image_rect();
        r.w <= self.view_w + 0.5 && r.h <= self.view_h + 0.5
    }

    /// Zoom that makes the image fit the view.
    pub fn fit_zoom(&self) -> f64 {
        let z = (self.view_w / self.image_w).min(self.view_h / self.image_h) * self.dpr;
        let z = if self.upscale_on_fit { z } else { z.min(1.0) };
        z.clamp(ZOOM_MIN, ZOOM_MAX)
    }

    /// Sets the image size (after orientation) and re-applies the zoom mode.
    /// In manual mode the zoom is kept and the image is centred.
    pub fn set_image_size(&mut self, w: u32, h: u32) {
        self.image_w = f64::from(w.max(1));
        self.image_h = f64::from(h.max(1));
        if self.mode == ZoomMode::Fit {
            self.zoom = self.fit_zoom();
        }
        self.center();
    }

    /// Updates the view size, keeping the image point at the view centre fixed.
    pub fn set_view_size(&mut self, w: f64, h: f64, dpr: f64) {
        let (cx, cy) = (self.view_w / 2.0, self.view_h / 2.0);
        let (ix, iy) = self.view_to_image(cx, cy);
        self.view_w = w.max(1.0);
        self.view_h = h.max(1.0);
        self.dpr = if dpr > 0.0 { dpr } else { 1.0 };
        if self.mode == ZoomMode::Fit {
            self.zoom = self.fit_zoom();
            self.center();
        } else {
            let s = self.scale();
            self.offset_x = self.view_w / 2.0 - ix * s;
            self.offset_y = self.view_h / 2.0 - iy * s;
            self.clamp();
        }
    }

    /// Switches to fit mode.
    pub fn fit(&mut self) {
        self.mode = ZoomMode::Fit;
        self.zoom = self.fit_zoom();
        self.center();
    }

    /// Sets an absolute zoom keeping the view centre fixed.
    pub fn set_zoom(&mut self, zoom: f64) {
        self.set_zoom_at(zoom, self.view_w / 2.0, self.view_h / 2.0);
    }

    /// Sets an absolute zoom keeping the image point under `(ax, ay)` fixed.
    pub fn set_zoom_at(&mut self, zoom: f64, ax: f64, ay: f64) {
        let (ix, iy) = self.view_to_image(ax, ay);
        self.mode = ZoomMode::Manual;
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let s = self.scale();
        self.offset_x = ax - ix * s;
        self.offset_y = ay - iy * s;
        self.clamp();
    }

    /// Next zoom level when stepping in (`dir > 0`) or out. Steps snap to 100%
    /// when they would cross it.
    pub fn step_zoom(zoom: f64, dir: i32) -> f64 {
        let next = if dir > 0 {
            zoom * ZOOM_STEP
        } else {
            zoom / ZOOM_STEP
        };
        let crosses_one = (zoom < 1.0 && next > 1.0) || (zoom > 1.0 && next < 1.0);
        let next = if crosses_one { 1.0 } else { next };
        next.clamp(ZOOM_MIN, ZOOM_MAX)
    }

    /// Moves the image by `(dx, dy)` logical pixels.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.offset_x += dx;
        self.offset_y += dy;
        self.clamp();
    }

    /// Positions the image so that its top-left is at `(x, y)` (then clamps).
    pub fn set_offset(&mut self, x: f64, y: f64) {
        self.offset_x = x;
        self.offset_y = y;
        self.clamp();
    }

    pub fn offset(&self) -> (f64, f64) {
        (self.offset_x, self.offset_y)
    }

    pub fn view_to_image(&self, x: f64, y: f64) -> (f64, f64) {
        let s = self.scale();
        ((x - self.offset_x) / s, (y - self.offset_y) / s)
    }

    pub fn image_to_view(&self, x: f64, y: f64) -> (f64, f64) {
        let s = self.scale();
        (self.offset_x + x * s, self.offset_y + y * s)
    }

    fn center(&mut self) {
        let r = self.image_rect();
        self.offset_x = (self.view_w - r.w) / 2.0;
        self.offset_y = (self.view_h - r.h) / 2.0;
        self.clamp();
    }

    /// Centres axes where the image is smaller than the view; otherwise keeps
    /// the image covering the view (no empty margins while panning).
    fn clamp(&mut self) {
        let r = self.image_rect();
        self.offset_x = clamp_axis(self.offset_x, r.w, self.view_w);
        self.offset_y = clamp_axis(self.offset_y, r.h, self.view_h);
    }
}

fn clamp_axis(offset: f64, size: f64, view: f64) -> f64 {
    if size <= view {
        ((view - size) / 2.0).round()
    } else {
        offset.clamp(view - size, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(img: (u32, u32), view: (f64, f64)) -> Viewport {
        let mut v = Viewport::new();
        v.set_view_size(view.0, view.1, 1.0);
        v.set_image_size(img.0, img.1);
        v
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fit_shrinks_large_images_and_centres() {
        let v = vp((2000, 1000), (1000.0, 1000.0));
        assert!(approx(v.zoom(), 0.5));
        let r = v.image_rect();
        assert!(approx(r.w, 1000.0) && approx(r.h, 500.0));
        assert!(approx(r.y, 250.0));
        assert!(v.fits());
    }

    #[test]
    fn fit_does_not_upscale_by_default() {
        let mut v = vp((100, 50), (1000.0, 1000.0));
        assert!(approx(v.zoom(), 1.0));
        v.upscale_on_fit = true;
        v.fit();
        assert!(approx(v.zoom(), 10.0));
    }

    #[test]
    fn hidpi_100_percent_maps_to_device_pixels() {
        let mut v = Viewport::new();
        v.set_view_size(500.0, 500.0, 2.0);
        v.set_image_size(400, 400);
        v.set_zoom(1.0);
        assert!(approx(v.image_rect().w, 200.0));
    }

    #[test]
    fn zoom_keeps_anchor_point_fixed() {
        let mut v = vp((4000, 4000), (1000.0, 1000.0));
        v.set_zoom(1.0);
        let anchor = (300.0, 700.0);
        let before = v.view_to_image(anchor.0, anchor.1);
        v.set_zoom_at(2.0, anchor.0, anchor.1);
        let after = v.view_to_image(anchor.0, anchor.1);
        assert!(approx(before.0, after.0) && approx(before.1, after.1));
        assert_eq!(v.mode(), ZoomMode::Manual);
    }

    #[test]
    fn pan_is_clamped_to_image_edges() {
        let mut v = vp((4000, 3000), (1000.0, 1000.0));
        v.set_zoom(1.0);
        v.pan(1e6, 1e6);
        assert_eq!(v.offset(), (0.0, 0.0));
        v.pan(-1e7, -1e7);
        assert_eq!(v.offset(), (-3000.0, -2000.0));
        assert!(!v.fits());
    }

    #[test]
    fn small_axis_stays_centred_while_panning() {
        let mut v = vp((4000, 100), (1000.0, 1000.0));
        v.set_zoom(1.0);
        v.pan(-500.0, -500.0);
        assert!(approx(v.offset().1, 450.0));
    }

    #[test]
    fn step_zoom_snaps_to_one() {
        assert!(approx(Viewport::step_zoom(0.9, 1), 1.0));
        assert!(approx(Viewport::step_zoom(1.1, -1), 1.0));
        assert!(approx(Viewport::step_zoom(1.0, 1), ZOOM_STEP));
        assert!(approx(Viewport::step_zoom(ZOOM_MAX, 1), ZOOM_MAX));
    }

    #[test]
    fn resize_in_fit_mode_refits() {
        let mut v = vp((2000, 2000), (1000.0, 1000.0));
        v.set_view_size(500.0, 500.0, 1.0);
        assert!(approx(v.zoom(), 0.25));
    }

    #[test]
    fn resize_in_manual_mode_keeps_centre_point() {
        let mut v = vp((4000, 4000), (1000.0, 1000.0));
        v.set_zoom(1.0);
        v.pan(-700.0, -300.0);
        let c = v.view_to_image(500.0, 500.0);
        v.set_view_size(800.0, 600.0, 1.0);
        let c2 = v.view_to_image(400.0, 300.0);
        assert!(approx(c.0, c2.0) && approx(c.1, c2.1));
    }
}
