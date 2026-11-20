//! Fullscreen mode with auto-hiding on-screen controls, and the slideshow.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::window::Window;

/// Controls and cursor hide after this long without pointer movement.
const AUTOHIDE: Duration = Duration::from_secs(2);
pub const DEFAULT_SLIDESHOW_SECS: u32 = 5;

pub struct FsUi {
    pub revealer: gtk::Revealer,
    pub interval: gtk::SpinButton,
    hide_timer: RefCell<Option<glib::SourceId>>,
    slideshow_timer: RefCell<Option<glib::SourceId>>,
    slideshow: Cell<bool>,
    last_pointer: Cell<(f64, f64)>,
    hovered: Rc<Cell<bool>>,
}

impl FsUi {
    /// `backdrop` is blurred behind the controls.
    pub fn new(backdrop: &impl IsA<gtk::Widget>) -> Self {
        let button = |icon: &str, action: &str, tip: &str| {
            gtk::Button::builder()
                .icon_name(icon)
                .action_name(action)
                .tooltip_text(tip)
                .focusable(false)
                .build()
        };
        let play = gtk::ToggleButton::builder()
            .icon_name("media-playback-start-symbolic")
            .action_name("win.slideshow")
            .tooltip_text("Slideshow (F5)")
            .focusable(false)
            .build();
        let interval = gtk::SpinButton::with_range(1.0, 3600.0, 1.0);
        interval.set_value(f64::from(DEFAULT_SLIDESHOW_SECS));
        interval.set_tooltip_text(Some("Seconds per image"));
        interval.set_focusable(false);

        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .css_classes(["osd", "toolbar"])
            .build();
        controls.append(&button("go-previous-symbolic", "win.prev", "Previous"));
        controls.append(&button("go-next-symbolic", "win.next", "Next"));
        controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        controls.append(&play);
        controls.append(&interval);
        controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        controls.append(&button(
            "zoom-fit-best-symbolic",
            "win.zoom-fit",
            "Best Fit",
        ));
        controls.append(&button(
            "object-rotate-left-symbolic",
            "win.rotate-ccw",
            "Rotate Anticlockwise",
        ));
        controls.append(&button(
            "object-rotate-right-symbolic",
            "win.rotate-cw",
            "Rotate Clockwise",
        ));
        controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        controls.append(&button(
            "view-restore-symbolic",
            "win.fullscreen",
            "Leave Fullscreen (F11)",
        ));

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(12)
            .reveal_child(false)
            .visible(false)
            .child(&crate::blur::BlurBin::new(&controls, backdrop))
            .build();
        let hovered = Rc::new(Cell::new(false));
        let hover = gtk::EventControllerMotion::new();
        let h = Rc::clone(&hovered);
        hover.connect_enter(move |_, _, _| h.set(true));
        let h = Rc::clone(&hovered);
        hover.connect_leave(move |_| h.set(false));
        controls.add_controller(hover);
        Self {
            revealer,
            interval,
            hovered,
            hide_timer: RefCell::new(None),
            slideshow_timer: RefCell::new(None),
            slideshow: Cell::new(false),
            last_pointer: Cell::new((-1.0, -1.0)),
        }
    }
}

pub fn setup(win: &Window) {
    let inner = &win.inner;

    let weak = Rc::downgrade(inner);
    inner.window.connect_fullscreened_notify(move |w| {
        if let Some(inner) = weak.upgrade() {
            Window { inner }.on_fullscreen_changed(w.is_fullscreen());
        }
    });

    // Any real pointer movement reveals the controls and the cursor.
    let motion = gtk::EventControllerMotion::new();
    let weak = Rc::downgrade(inner);
    motion.connect_motion(move |_, x, y| {
        let Some(inner) = weak.upgrade() else { return };
        let fs = &inner.fs;
        let (lx, ly) = fs.last_pointer.replace((x, y));
        if (lx - x).abs() < 1.0 && (ly - y).abs() < 1.0 {
            return; // Synthetic motion after a redraw.
        }
        if inner.window.is_fullscreen() {
            Window { inner }.reveal_controls();
        }
    });
    inner.overlay.add_controller(motion);

    let weak = Rc::downgrade(inner);
    inner.fs.interval.connect_value_changed(move |spin| {
        if let Some(inner) = weak.upgrade() {
            inner.config.borrow_mut().slideshow_seconds = spin.value() as u32;
            Window { inner }.restart_slideshow_timer();
        }
    });
}

impl Window {
    pub fn toggle_fullscreen(&self) {
        let w = &self.inner.window;
        if w.is_fullscreen() {
            w.unfullscreen();
        } else {
            w.fullscreen();
        }
    }

    /// Esc / Q: leave slideshow and fullscreen first, otherwise close.
    pub fn escape(&self) {
        if self.inner.window.is_fullscreen() {
            self.inner.window.unfullscreen();
        } else if self.inner.fs.slideshow.get() {
            self.set_slideshow(false);
        } else {
            self.inner.window.close();
        }
    }

    fn on_fullscreen_changed(&self, fullscreen: bool) {
        let inner = &self.inner;
        inner
            .header
            .set_visible(!fullscreen && inner.config.borrow().show_toolbar);
        inner
            .status
            .root
            .set_visible(!fullscreen && inner.config.borrow().show_statusbar);
        inner
            .strip
            .root
            .set_visible(!fullscreen && inner.config.borrow().show_thumbnails);
        inner.fs.revealer.set_visible(fullscreen);
        crate::actions::set_action_state(&inner.window, "fullscreen", fullscreen);
        if fullscreen {
            if inner.config.borrow().fit_on_fullscreen {
                inner.view.fit();
            }
            self.reveal_controls();
        } else {
            self.cancel_autohide();
            inner.fs.revealer.set_reveal_child(false);
            inner.view.set_cursor(None);
            self.set_slideshow(false);
        }
    }

    fn reveal_controls(&self) {
        let inner = &self.inner;
        inner.fs.revealer.set_reveal_child(true);
        inner.view.set_cursor(None);
        self.cancel_autohide();
        let weak = Rc::downgrade(inner);
        let id = glib::timeout_add_local_once(AUTOHIDE, move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.fs.hide_timer.replace(None);
            // Keep the controls while the pointer rests on them.
            if inner.fs.hovered.get() {
                Window { inner }.reveal_controls();
                return;
            }
            inner.fs.revealer.set_reveal_child(false);
            inner.view.set_cursor_from_name(Some("none"));
        });
        inner.fs.hide_timer.replace(Some(id));
    }

    fn cancel_autohide(&self) {
        if let Some(id) = self.inner.fs.hide_timer.take() {
            id.remove();
        }
    }

    pub fn slideshow_running(&self) -> bool {
        self.inner.fs.slideshow.get()
    }

    /// Starts or stops the slideshow. Starting enters fullscreen, like Viewnior.
    pub fn set_slideshow(&self, on: bool) {
        let inner = &self.inner;
        inner.fs.slideshow.set(on);
        crate::actions::set_action_state(&inner.window, "slideshow", on);
        if on && !inner.window.is_fullscreen() {
            inner.window.fullscreen();
        }
        self.restart_slideshow_timer();
    }

    /// (Re)arms the timer for the next slide. Called whenever an image is
    /// shown, so manual navigation also resets the countdown.
    pub fn restart_slideshow_timer(&self) {
        let fs = &self.inner.fs;
        if let Some(id) = fs.slideshow_timer.take() {
            id.remove();
        }
        if !fs.slideshow.get() {
            return;
        }
        let secs = fs.interval.value().max(1.0);
        let weak = Rc::downgrade(&self.inner);
        let id = glib::timeout_add_local_once(Duration::from_secs_f64(secs), move || {
            let Some(inner) = weak.upgrade() else { return };
            inner.fs.slideshow_timer.replace(None);
            Window { inner }.navigate(1);
        });
        fs.slideshow_timer.replace(Some(id));
    }
}
