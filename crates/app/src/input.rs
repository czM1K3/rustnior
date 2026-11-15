//! Pointer, wheel, keyboard-navigation and drag-and-drop handling.

use std::cell::Cell;
use std::rc::Rc;

use gtk::{gdk, gio, glib, prelude::*};

use rustnior_core::config::{ClickPref, WheelPref};

use crate::imageview::PAN_STEP;
use crate::window::Window;

pub fn setup(win: &Window) {
    setup_keys(win);
    setup_scroll(win);
    setup_clicks(win);
    setup_context_menu(win);
    setup_drop(win);
}

/// Arrow keys navigate while the whole image is visible, otherwise they pan
/// (Shift pans a page), like Viewnior.
fn setup_keys(win: &Window) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(&win.inner);
    keys.connect_key_pressed(move |_, key, _, state| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let w = Window { inner };
        let view = &w.inner.view;
        let (dx, dy) = match key {
            gdk::Key::Left | gdk::Key::KP_Left => (1.0, 0.0),
            gdk::Key::Right | gdk::Key::KP_Right => (-1.0, 0.0),
            gdk::Key::Up | gdk::Key::KP_Up => (0.0, 1.0),
            gdk::Key::Down | gdk::Key::KP_Down => (0.0, -1.0),
            _ => return glib::Propagation::Proceed,
        };
        if state.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::CONTROL_MASK) {
            return glib::Propagation::Proceed; // Alt+arrows are accelerators.
        }
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        if view.image_fits() && !shift {
            w.navigate(if dx + dy > 0.0 { -1 } else { 1 });
        } else if shift {
            view.pan_pages(dx, dy);
        } else {
            view.pan(dx * PAN_STEP, dy * PAN_STEP);
        }
        glib::Propagation::Stop
    });
    win.inner.window.add_controller(keys);
}

fn setup_scroll(win: &Window) {
    let view = win.inner.view.clone();
    let pointer = Rc::new(Cell::new((0.0, 0.0)));
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[strong]
        pointer,
        move |_, x, y| pointer.set((x, y))
    ));
    view.add_controller(motion);

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let weak = Rc::downgrade(&win.inner);
    scroll.connect_scroll(move |c, dx, dy| {
        let Some(inner) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let w = Window { inner };
        let view = &w.inner.view;
        let state = c.current_event_state();
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let anchor = pointer.get();

        if c.unit() != gdk::ScrollUnit::Wheel {
            // Touchpad / precise scrolling: pan, or zoom smoothly with Ctrl.
            if ctrl {
                view.zoom_by_at((-dy * 0.01).exp(), anchor);
            } else {
                view.pan(-dx, -dy);
            }
            return glib::Propagation::Stop;
        }

        let mode = if ctrl {
            WheelPref::Zoom
        } else {
            w.inner.config.borrow().wheel
        };
        // Horizontal wheel and Shift+wheel navigate in zoom mode (Viewnior).
        let horizontal = if dx != 0.0 {
            dx
        } else if shift {
            dy
        } else {
            0.0
        };
        match mode {
            WheelPref::Zoom if horizontal != 0.0 && !ctrl => {
                w.navigate(if horizontal > 0.0 { 1 } else { -1 });
            }
            WheelPref::Zoom => {
                if dy != 0.0 {
                    view.zoom_step_at(if dy < 0.0 { 1 } else { -1 }, anchor);
                }
            }
            WheelPref::Navigate if shift || dx != 0.0 => {
                let d = if dx != 0.0 { dx } else { dy };
                view.zoom_step_at(if d < 0.0 { 1 } else { -1 }, anchor);
            }
            WheelPref::Navigate => {
                if dy != 0.0 {
                    w.navigate(if dy > 0.0 { 1 } else { -1 });
                }
            }
            WheelPref::Scroll => {
                let (fx, fy) = if shift { (-dy, 0.0) } else { (-dx, -dy) };
                view.pan_pages(fx * 0.25, fy * 0.25);
            }
        }
        glib::Propagation::Stop
    });
    view.add_controller(scroll);
}

/// Double-click: toggle fit/100% (Viewnior's default), fullscreen, or next.
fn setup_clicks(win: &Window) {
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_PRIMARY)
        .build();
    let weak = Rc::downgrade(&win.inner);
    click.connect_pressed(move |_, n, _, _| {
        let Some(inner) = weak.upgrade() else { return };
        if n != 2 {
            return;
        }
        let action = inner.config.borrow().double_click;
        let w = Window { inner };
        match action {
            ClickPref::Zoom => {
                let view = &w.inner.view;
                if view.zoom_mode() == rustnior_core::viewport::ZoomMode::Fit {
                    view.set_zoom(1.0);
                } else {
                    view.fit();
                }
            }
            ClickPref::Fullscreen => w.toggle_fullscreen(),
            ClickPref::Next => w.navigate(1),
        }
    });
    win.inner.view.add_controller(click);
}

fn setup_context_menu(win: &Window) {
    let view = &win.inner.view;
    let popover = gtk::PopoverMenu::from_model(Some(&crate::actions::context_menu()));
    popover.set_parent(view);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    click.connect_pressed(move |_, _, x, y| {
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    });
    view.add_controller(click);
}

fn setup_drop(win: &Window) {
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let weak = Rc::downgrade(&win.inner);
    drop.connect_drop(move |_, value, _, _| {
        let Some(inner) = weak.upgrade() else {
            return false;
        };
        let paths: Vec<_> = value
            .get::<gdk::FileList>()
            .map(|l| {
                l.files()
                    .iter()
                    .filter_map(gio::prelude::FileExt::path)
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return false;
        }
        Window { inner }.open_paths(paths, None);
        true
    });
    win.inner.window.add_controller(drop);
}
