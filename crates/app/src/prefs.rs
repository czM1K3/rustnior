//! Preferences window. Every change is applied and saved immediately.

use gtk::{gdk, prelude::*};
use rustnior_core::Config;
use rustnior_core::config::{ClickPref, ModifyPref, SortPref, ThemePref, WheelPref, ZoomPref};

use crate::window::Window;

fn update(win: &Window, f: impl FnOnce(&mut Config)) {
    f(&mut win.inner.config.borrow_mut());
    win.apply_config();
    crate::settings::save(&win.inner.config.borrow());
}

/// A page of labelled controls, one per row.
struct Page {
    grid: gtk::Box,
}

impl Page {
    fn new() -> Self {
        let grid = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        Self { grid }
    }

    fn add(&mut self, label: &str, widget: &impl IsA<gtk::Widget>) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        let l = gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .hexpand(true)
            .build();
        widget.as_ref().set_valign(gtk::Align::Center);
        row.append(&l);
        row.append(widget);
        self.grid.append(&row);
    }
}

fn switch(win: &Window, value: bool, set: fn(&mut Config, bool)) -> gtk::Switch {
    let s = gtk::Switch::builder().active(value).build();
    let w = win.clone();
    s.connect_active_notify(move |s| {
        let on = s.is_active();
        update(&w, |c| set(c, on));
    });
    s
}

fn choice<T: Copy + PartialEq + 'static>(
    win: &Window,
    options: &'static [(T, &'static str)],
    value: T,
    set: fn(&mut Config, T),
) -> gtk::DropDown {
    let labels: Vec<&str> = options.iter().map(|(_, l)| *l).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    let index = options.iter().position(|(v, _)| *v == value).unwrap_or(0);
    dd.set_selected(index as u32);
    let w = win.clone();
    dd.connect_selected_notify(move |dd| {
        if let Some((v, _)) = options.get(dd.selected() as usize) {
            let v = *v;
            update(&w, |c| set(c, v));
        }
    });
    dd
}

fn spin(
    win: &Window,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
    set: fn(&mut Config, f64),
) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_value(value);
    let w = win.clone();
    s.connect_value_changed(move |s| {
        let v = s.value();
        update(&w, |c| set(c, v));
    });
    s
}

const ZOOM: &[(ZoomPref, &str)] = &[
    (ZoomPref::Smart, "Fit large images"),
    (ZoomPref::Fit, "Always fit"),
    (ZoomPref::Normal, "Normal size (100%)"),
    (ZoomPref::LastUsed, "Last used"),
];
const THEME: &[(ThemePref, &str)] = &[
    (ThemePref::System, "Follow system"),
    (ThemePref::Light, "Light"),
    (ThemePref::Dark, "Dark"),
];
const WHEEL: &[(WheelPref, &str)] = &[
    (WheelPref::Zoom, "Zoom"),
    (WheelPref::Navigate, "Next/previous image"),
    (WheelPref::Scroll, "Scroll"),
];
const CLICK: &[(ClickPref, &str)] = &[
    (ClickPref::Zoom, "Toggle fit / 100%"),
    (ClickPref::Fullscreen, "Toggle fullscreen"),
    (ClickPref::Next, "Next image"),
];
const MODIFY: &[(ModifyPref, &str)] = &[
    (ModifyPref::Ask, "Ask"),
    (ModifyPref::Save, "Save automatically"),
    (ModifyPref::Ignore, "Discard"),
];
const SORT: &[(SortPref, &str)] = &[
    (SortPref::Name, "Name"),
    (SortPref::Modified, "Date modified"),
    (SortPref::Size, "Size"),
];

pub fn show(win: &Window) {
    let c = win.inner.config.borrow().clone();

    let mut view = Page::new();
    view.add("Theme", &choice(win, THEME, c.theme, |c, v| c.theme = v));
    view.add(
        "Initial zoom",
        &choice(win, ZOOM, c.zoom_mode, |c, v| c.zoom_mode = v),
    );
    view.add(
        "Smooth zooming",
        &switch(win, c.smooth_images, |c, v| c.smooth_images = v),
    );
    view.add(
        "Checkerboard behind transparency",
        &switch(win, c.checkerboard, |c, v| c.checkerboard = v),
    );
    let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    if let Some((r, g, b)) = c.background_rgb() {
        color.set_rgba(&gdk::RGBA::new(r, g, b, 1.0));
    } else {
        color.set_rgba(&gdk::RGBA::new(0.12, 0.12, 0.12, 1.0));
    }
    color.set_sensitive(!c.background.is_empty());
    let custom = gtk::Switch::builder()
        .active(!c.background.is_empty())
        .build();
    let (w, cb) = (win.clone(), color.clone());
    custom.connect_active_notify(move |s| {
        cb.set_sensitive(s.is_active());
        let hex = if s.is_active() {
            to_hex(&cb.rgba())
        } else {
            String::new()
        };
        update(&w, |c| c.background = hex);
    });
    let w = win.clone();
    color.connect_rgba_notify(move |b| {
        let hex = to_hex(&b.rgba());
        update(&w, |c| c.background = hex);
    });
    view.add("Custom background colour", &custom);
    view.add("Background colour", &color);
    view.add(
        "Fit when entering fullscreen",
        &switch(win, c.fit_on_fullscreen, |c, v| c.fit_on_fullscreen = v),
    );

    let mut behaviour = Page::new();
    behaviour.add(
        "Mouse wheel",
        &choice(win, WHEEL, c.wheel, |c, v| c.wheel = v),
    );
    behaviour.add(
        "Double-click",
        &choice(win, CLICK, c.double_click, |c, v| c.double_click = v),
    );
    behaviour.add(
        "Sort images by",
        &choice(win, SORT, c.sort_by, |c, v| c.sort_by = v),
    );
    behaviour.add(
        "Show hidden files",
        &switch(win, c.show_hidden, |c, v| c.show_hidden = v),
    );
    behaviour.add(
        "Slideshow interval (seconds)",
        &spin(
            win,
            1.0,
            3600.0,
            1.0,
            f64::from(c.slideshow_seconds),
            |c, v| c.slideshow_seconds = v as u32,
        ),
    );
    behaviour.add(
        "Start maximised",
        &switch(win, c.start_maximized, |c, v| c.start_maximized = v),
    );

    let mut files = Page::new();
    files.add(
        "Unsaved rotation when leaving an image",
        &choice(win, MODIFY, c.on_modified, |c, v| c.on_modified = v),
    );
    files.add(
        "Confirm before moving to trash",
        &switch(win, c.confirm_delete, |c, v| c.confirm_delete = v),
    );
    files.add(
        "JPEG quality",
        &spin(win, 1.0, 100.0, 1.0, f64::from(c.jpeg_quality), |c, v| {
            c.jpeg_quality = v as u8
        }),
    );
    files.add(
        "PNG compression (0–9)",
        &spin(win, 0.0, 9.0, 1.0, f64::from(c.png_compression), |c, v| {
            c.png_compression = v as u8
        }),
    );

    let mut perf = Page::new();
    perf.add(
        "Image cache (MiB)",
        &spin(win, 64.0, 65536.0, 64.0, f64::from(c.cache_mib), |c, v| {
            c.cache_mib = v as u32
        }),
    );
    perf.add(
        "Images to preload on each side",
        &spin(win, 0.0, 8.0, 1.0, f64::from(c.preload), |c, v| {
            c.preload = v as u8
        }),
    );

    // A Stack rather than a Notebook: GTK 4.24 warns about GtkSwitch
    // baselines inside notebooks.
    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .vhomogeneous(false)
        .build();
    for (page, name, title) in [
        (view, "view", "View"),
        (behaviour, "behaviour", "Behaviour"),
        (files, "files", "Files"),
        (perf, "performance", "Performance"),
    ] {
        stack.add_titled(&page.grid, Some(name), title);
    }
    let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
    let header = gtk::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let dialog = gtk::Window::builder()
        .title("Preferences")
        .transient_for(win.window())
        .destroy_with_parent(true)
        .modal(true)
        .resizable(false)
        .titlebar(&header)
        .child(&stack)
        .build();
    let keys = gtk::EventControllerKey::new();
    let d = dialog.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape
            && let Some(d) = d.upgrade()
        {
            d.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    dialog.add_controller(keys);
    dialog.present();
}

fn to_hex(c: &gdk::RGBA) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.red()), b(c.green()), b(c.blue()))
}
