//! Keyboard shortcuts overview, generated from the live shortcut table.

use gtk::{gdk, glib, prelude::*};

use crate::actions::{SHORTCUTS, accels_for};
use crate::window::Window;

/// Human-readable form of a GTK accelerator string ("<Control>r" → "Ctrl+R").
fn pretty(accel: &str) -> Option<String> {
    let (key, mods) = gtk::accelerator_parse(accel)?;
    Some(gtk::accelerator_get_label(key, mods).to_string())
}

pub fn show(win: &Window) {
    let grid = gtk::Grid::builder()
        .column_spacing(24)
        .row_spacing(6)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    let mut row = 0;
    for (action, label, _) in SHORTCUTS {
        let keys: Vec<String> = accels_for(win, action)
            .iter()
            .filter_map(|a| pretty(a))
            .collect();
        if keys.is_empty() {
            continue;
        }
        let name = gtk::Label::builder().label(*label).xalign(0.0).build();
        let value = gtk::Label::builder()
            .label(keys.join(", "))
            .xalign(0.0)
            .css_classes(["dim-label", "monospace"])
            .build();
        grid.attach(&name, 0, row, 1, 1);
        grid.attach(&value, 1, row, 1, 1);
        row += 1;
    }
    let hint = gtk::Label::builder()
        .label("Mouse: wheel zooms (Shift+wheel or tilt navigates), Ctrl+wheel always zooms, drag pans, double-click toggles fit / 100%, right-click opens the menu. Arrow keys switch images when the whole image is visible and pan otherwise.\nShortcuts can be changed in the [shortcuts] table of config.toml.")
        .wrap(true)
        .xalign(0.0)
        .max_width_chars(60)
        .css_classes(["dim-label"])
        .margin_top(12)
        .build();
    grid.attach(&hint, 0, row, 2, 1);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(640)
        .child(&grid)
        .build();
    let dialog = gtk::Window::builder()
        .title("Keyboard Shortcuts")
        .transient_for(win.window())
        .destroy_with_parent(true)
        .child(&scrolled)
        .build();
    let keys = gtk::EventControllerKey::new();
    let d = dialog.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape
            && let Some(d) = d.upgrade()
        {
            d.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialog.add_controller(keys);
    dialog.present();
}
