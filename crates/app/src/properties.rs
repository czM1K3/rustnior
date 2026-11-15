//! "Image Properties" window (Alt+Return): file details and EXIF metadata.

use std::time::UNIX_EPOCH;

use gtk::{gdk, glib, prelude::*};

use crate::window::Window;

pub fn show(win: &Window) {
    let Some(entry) = win.inner.collection.borrow().current().cloned() else {
        return;
    };
    let image = win.inner.loader.cached(&entry.path);

    let mut general: Vec<(&str, String)> = vec![("Name", entry.name.clone())];
    if let Some(dir) = entry.path.parent() {
        general.push(("Folder", dir.display().to_string()));
    }
    if let Some(img) = &image {
        general.push(("Type", img.format_name.to_owned()));
        let (w, h) = img.oriented_size();
        general.push(("Dimensions", format!("{w} × {h} pixels")));
        if img.frames.len() > 1 {
            general.push(("Frames", img.frames.len().to_string()));
        }
        if !img.orientation.is_identity() {
            general.push((
                "Orientation",
                format!("EXIF {} (applied)", img.orientation.to_exif()),
            ));
        }
    }
    general.push((
        "File Size",
        format!("{} ({} bytes)", glib::format_size(entry.size), entry.size),
    ));
    if let Some(dt) = entry
        .modified
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| glib::DateTime::from_unix_local(d.as_secs() as i64).ok())
        .and_then(|dt| dt.format("%c").ok())
    {
        general.push(("Modified", dt.to_string()));
    }
    let exif = image
        .as_ref()
        .and_then(|i| i.exif.as_deref())
        .map(rustnior_core::exif::fields)
        .unwrap_or_default();

    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(6)
        .margin_start(18)
        .margin_end(18)
        .margin_top(18)
        .margin_bottom(18)
        .build();
    let mut row = 0;
    let mut section = |title: &str, rows: &mut dyn Iterator<Item = (String, String)>| {
        let heading = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_top(if row == 0 { 0 } else { 12 })
            .build();
        grid.attach(&heading, 0, row, 2, 1);
        row += 1;
        for (k, v) in rows {
            let key = gtk::Label::builder()
                .label(k)
                .xalign(1.0)
                .yalign(0.0)
                .css_classes(["dim-label"])
                .build();
            let value = gtk::Label::builder()
                .label(v)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .hexpand(true)
                .build();
            grid.attach(&key, 0, row, 1, 1);
            grid.attach(&value, 1, row, 1, 1);
            row += 1;
        }
    };
    section(
        "General",
        &mut general.into_iter().map(|(k, v)| (k.to_owned(), v)),
    );
    if !exif.is_empty() {
        section("EXIF", &mut exif.into_iter());
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&grid)
        .build();
    let dialog = gtk::Window::builder()
        .title(format!("{} — Properties", entry.name))
        .transient_for(win.window())
        .destroy_with_parent(true)
        .default_width(460)
        .default_height(520)
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
    // Don't start with the first selectable label focused and highlighted.
    GtkWindowExt::set_focus(&dialog, gtk::Widget::NONE);
}
