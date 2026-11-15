//! Image orientation: the 8 combinations of 90° rotations and mirroring
//! (the dihedral group D4). Used for EXIF orientation and user rotate/flip;
//! the view applies it at draw time and pixels are only touched when saving.

use crate::pixels::Frame;

/// "Mirror horizontally (if `flip`), then rotate clockwise `rot` × 90°".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation {
    rot: u8,
    flip: bool,
}

impl Orientation {
    pub const IDENTITY: Self = Self {
        rot: 0,
        flip: false,
    };

    pub const fn new(quarter_turns_cw: u8, flip: bool) -> Self {
        Self {
            rot: quarter_turns_cw % 4,
            flip,
        }
    }

    /// Clockwise rotation in degrees (applied after the mirror).
    pub fn rotation_degrees(self) -> u32 {
        u32::from(self.rot) * 90
    }

    pub fn quarter_turns(self) -> u8 {
        self.rot
    }

    pub fn is_flipped(self) -> bool {
        self.flip
    }

    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// True if width and height trade places.
    pub fn swaps_dimensions(self) -> bool {
        self.rot % 2 == 1
    }

    /// Size after applying this orientation to a `w`×`h` image.
    pub fn apply_size(self, w: u32, h: u32) -> (u32, u32) {
        if self.swaps_dimensions() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// Orientation needed to display an image whose EXIF tag is `value`.
    /// Unknown values are treated as "normal".
    pub fn from_exif(value: u16) -> Self {
        match value {
            2 => Self::new(0, true),
            3 => Self::new(2, false),
            4 => Self::new(2, true),
            5 => Self::new(3, true),
            6 => Self::new(1, false),
            7 => Self::new(1, true),
            8 => Self::new(3, false),
            _ => Self::IDENTITY,
        }
    }

    /// The EXIF orientation tag value describing this orientation.
    pub fn to_exif(self) -> u16 {
        match (self.rot, self.flip) {
            (0, false) => 1,
            (0, true) => 2,
            (2, false) => 3,
            (2, true) => 4,
            (3, true) => 5,
            (1, false) => 6,
            (1, true) => 7,
            _ => 8, // (3, false)
        }
    }

    /// `self` followed by `next`.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        // next ∘ self = R^b F^g R^a F^f; mirroring reverses rotation direction.
        if next.flip {
            Self::new((4 + next.rot - self.rot) % 4, !self.flip)
        } else {
            Self::new(next.rot + self.rot, self.flip)
        }
    }

    #[must_use]
    pub fn rotate_cw(self) -> Self {
        self.then(Self::new(1, false))
    }

    #[must_use]
    pub fn rotate_ccw(self) -> Self {
        self.then(Self::new(3, false))
    }

    /// Mirrors the *displayed* image left–right.
    #[must_use]
    pub fn flip_horizontal(self) -> Self {
        self.then(Self::new(0, true))
    }

    /// Mirrors the *displayed* image top–bottom.
    #[must_use]
    pub fn flip_vertical(self) -> Self {
        self.then(Self::new(2, true))
    }

    #[must_use]
    pub fn inverse(self) -> Self {
        if self.flip {
            self // every mirror-containing element is an involution
        } else {
            Self::new(4 - self.rot, false)
        }
    }

    /// Maps source pixel `(x, y)` of a `w`×`h` image to its destination.
    pub fn map_point(self, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
        let (x, w2) = if self.flip { (w - 1 - x, w) } else { (x, w) };
        match self.rot {
            0 => (x, y),
            1 => (h - 1 - y, x),
            2 => (w2 - 1 - x, h - 1 - y),
            _ => (y, w2 - 1 - x),
        }
    }

    /// Produces a new frame with the pixels physically transformed.
    pub fn apply_to_frame(self, src: &Frame) -> Frame {
        if self.is_identity() {
            return src.clone();
        }
        let bpp = src.format.bytes_per_pixel();
        let (w, h) = (src.width, src.height);
        let (dw, dh) = self.apply_size(w, h);
        let dstride = dw as usize * bpp;
        let mut out = vec![0u8; dstride * dh as usize];
        for y in 0..h {
            let row = &src.data[y as usize * src.stride..][..w as usize * bpp];
            for x in 0..w {
                let (dx, dy) = self.map_point(x, y, w, h);
                let d = dy as usize * dstride + dx as usize * bpp;
                let s = x as usize * bpp;
                out[d..d + bpp].copy_from_slice(&row[s..s + bpp]);
            }
        }
        Frame {
            width: dw,
            height: dh,
            stride: dstride,
            format: src.format,
            data: std::sync::Arc::new(out),
            delay: src.delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixels::PixelFormat;

    fn all() -> impl Iterator<Item = Orientation> {
        (0..8).map(|i| Orientation::new(i % 4, i >= 4))
    }

    #[test]
    fn exif_round_trips() {
        for v in 1..=8 {
            assert_eq!(Orientation::from_exif(v).to_exif(), v);
        }
        assert_eq!(Orientation::from_exif(0), Orientation::IDENTITY);
        assert_eq!(Orientation::from_exif(99), Orientation::IDENTITY);
    }

    #[test]
    fn four_rotations_are_identity() {
        for o in all() {
            let r = o.rotate_cw().rotate_cw().rotate_cw().rotate_cw();
            assert_eq!(r, o);
            assert_eq!(o.rotate_cw().rotate_ccw(), o);
        }
    }

    #[test]
    fn flips_are_involutions() {
        for o in all() {
            assert_eq!(o.flip_horizontal().flip_horizontal(), o);
            assert_eq!(o.flip_vertical().flip_vertical(), o);
            assert_eq!(
                o.flip_horizontal().flip_vertical(),
                o.rotate_cw().rotate_cw()
            );
        }
    }

    #[test]
    fn inverse_cancels() {
        for o in all() {
            assert_eq!(o.then(o.inverse()), Orientation::IDENTITY);
            assert_eq!(o.inverse().then(o), Orientation::IDENTITY);
        }
    }

    /// `then` must agree with applying the two point maps in sequence.
    #[test]
    fn composition_matches_point_mapping() {
        let (w, h) = (5, 3);
        for a in all() {
            for b in all() {
                let (aw, ah) = a.apply_size(w, h);
                for y in 0..h {
                    for x in 0..w {
                        let (x1, y1) = a.map_point(x, y, w, h);
                        let seq = b.map_point(x1, y1, aw, ah);
                        assert_eq!(a.then(b).map_point(x, y, w, h), seq, "{a:?} then {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn exif_6_rotates_clockwise() {
        // Top-left pixel of the stored image ends up top-right when displayed.
        let o = Orientation::from_exif(6);
        assert_eq!(o.map_point(0, 0, 4, 2), (1, 0));
        assert_eq!(o.apply_size(4, 2), (2, 4));
    }

    #[test]
    fn exif_5_is_transpose() {
        let o = Orientation::from_exif(5);
        for (x, y) in [(0, 0), (3, 1), (2, 0)] {
            assert_eq!(o.map_point(x, y, 4, 2), (y, x));
        }
    }

    #[test]
    fn apply_to_frame_rotates_pixels() {
        // 2×1 RGB: red, green → rotate CW → 1×2: red on top, green below.
        let f = Frame::new(2, 1, PixelFormat::Rgb8, vec![255, 0, 0, 0, 255, 0]).expect("frame");
        let r = Orientation::new(1, false).apply_to_frame(&f);
        assert_eq!((r.width, r.height, r.stride), (1, 2, 3));
        assert_eq!(&r.data[..], &[255, 0, 0, 0, 255, 0]);
        let m = Orientation::new(0, true).apply_to_frame(&f);
        assert_eq!(&m.data[..], &[0, 255, 0, 255, 0, 0]);
    }
}
