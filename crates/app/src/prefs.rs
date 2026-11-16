//! Preferences window. Every change is applied and saved immediately.

use gtk::{gdk, prelude::*};
use rustnior_core::Config;
use rustnior_core::config::hex_color;
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
    let (custom, color, opacity) = background_controls(win, &c);
    view.add("Custom background colour", &custom);
    view.add("Background colour", &color);
    view.add("Background opacity", &opacity);
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

/// The custom-background switch, colour button and opacity slider. Opacity
/// below 100% makes the window see-through behind the image.
fn background_controls(
    win: &Window,
    c: &Config,
) -> (gtk::Switch, gtk::ColorDialogButton, gtk::Scale) {
    let (rgb, alpha) = match c.background_rgba() {
        Some((r, g, b, a)) => (gdk::RGBA::new(r, g, b, 1.0), a),
        None => (gdk::RGBA::new(0.12, 0.12, 0.12, 1.0), 1.0),
    };
    let custom = gtk::Switch::builder()
        .active(!c.background.is_empty())
        .build();
    // Alpha is set with the opacity slider, which is easier to find than
    // the colour chooser's own alpha control.
    let dialog = gtk::ColorDialog::builder().with_alpha(false).build();
    let color = gtk::ColorDialogButton::new(Some(dialog));
    color.set_rgba(&rgb);
    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    opacity.set_value((f64::from(alpha) * 100.0).round());
    opacity.set_draw_value(true);
    opacity.set_value_pos(gtk::PositionType::Left);
    opacity.set_format_value_func(|_, v| format!("{v:.0}%"));
    opacity.set_size_request(200, -1);
    opacity.set_tooltip_text(Some(
        "Below 100% the desktop shows through behind the image (needs a compositor)",
    ));
    for w in [color.upcast_ref::<gtk::Widget>(), opacity.upcast_ref()] {
        w.set_sensitive(custom.is_active());
    }

    let store = {
        let (w, custom, color, opacity) =
            (win.clone(), custom.clone(), color.clone(), opacity.clone());
        move || {
            let hex = if custom.is_active() {
                let rgba = color.rgba();
                let alpha = (opacity.value() / 100.0) as f32;
                hex_color(rgba.red(), rgba.green(), rgba.blue(), alpha)
            } else {
                String::new()
            };
            if w.inner.config.borrow().background != hex {
                update(&w, |c| c.background = hex);
            }
        }
    };
    let (s, cb, op) = (store.clone(), color.clone(), opacity.clone());
    custom.connect_active_notify(move |sw| {
        cb.set_sensitive(sw.is_active());
        op.set_sensitive(sw.is_active());
        s();
    });
    let s = store.clone();
    color.connect_rgba_notify(move |_| s());
    opacity.connect_value_changed(move |_| store());

    (custom, color, opacity)
}
