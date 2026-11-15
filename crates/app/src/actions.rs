//! Window actions, their keyboard accelerators and the header bar.

use gtk::{gio, glib, prelude::*};
use rustnior_core::{Orientation, SortKey, SortOrder};

use crate::window::Window;

/// Every shortcut: (action, description, default accelerators). Mirrors
/// Viewnior's bindings plus common extras; drives both the accelerators and
/// the shortcuts window. Users can override them in the config file.
pub const SHORTCUTS: &[(&str, &str, &[&str])] = &[
    (
        "next",
        "Next image",
        &["space", "Page_Down", "<Alt>Right", "KP_Page_Down"],
    ),
    (
        "prev",
        "Previous image",
        &["BackSpace", "Page_Up", "<Alt>Left", "KP_Page_Up"],
    ),
    ("first", "First image", &["Home", "<Alt>Home", "KP_Home"]),
    ("last", "Last image", &["End", "<Alt>End", "KP_End"]),
    (
        "zoom-in",
        "Zoom in",
        &[
            "plus",
            "equal",
            "KP_Add",
            "<Control>plus",
            "<Control>equal",
            "<Control>KP_Add",
        ],
    ),
    (
        "zoom-out",
        "Zoom out",
        &[
            "minus",
            "KP_Subtract",
            "<Control>minus",
            "<Control>KP_Subtract",
        ],
    ),
    ("zoom-fit", "Best fit", &["f", "0", "KP_0"]),
    (
        "zoom-normal",
        "Normal size (100%)",
        &["1", "KP_1", "<Control>0"],
    ),
    ("zoom-2", "Zoom 200%", &["2", "KP_2"]),
    ("zoom-3", "Zoom 300%", &["3", "KP_3"]),
    ("rotate-cw", "Rotate clockwise", &["<Control>r", "r"]),
    (
        "rotate-ccw",
        "Rotate anticlockwise",
        &["<Control><Shift>r", "l"],
    ),
    ("flip-horizontal", "Flip horizontally", &["h"]),
    ("flip-vertical", "Flip vertically", &["v"]),
    ("fullscreen", "Fullscreen", &["F11"]),
    ("slideshow", "Slideshow", &["F5"]),
    ("escape", "Leave fullscreen / quit", &["Escape", "q"]),
    ("animation-toggle", "Pause/play animation", &["p"]),
    ("animation-step", "Next animation frame", &["j"]),
    ("open", "Open image", &["<Control>o"]),
    ("open-folder", "Open folder", &["<Control>f"]),
    ("save", "Save", &["<Control>s"]),
    ("save-as", "Save as", &["<Control><Shift>s"]),
    ("delete", "Move to trash", &["Delete", "KP_Delete"]),
    ("copy", "Copy image", &["<Control>c"]),
    ("copy-path", "Copy path", &["<Control><Shift>c"]),
    ("wallpaper", "Set as wallpaper", &["<Control>F8"]),
    (
        "properties",
        "Properties",
        &["<Alt>Return", "<Alt>KP_Enter"],
    ),
    ("show-toolbar", "Toggle toolbar", &["<Control>h"]),
    ("show-thumbnails", "Toggle thumbnails", &["t"]),
    ("preferences", "Preferences", &["<Control>comma"]),
    (
        "shortcuts",
        "Keyboard shortcuts",
        &["<Control>question", "F1"],
    ),
    ("close", "Close window", &["<Control>w"]),
];

/// Accelerators for `action`, honouring the user's overrides.
pub fn accels_for(win: &Window, action: &str) -> Vec<String> {
    if let Some(custom) = win.inner.config.borrow().shortcuts.get(action) {
        return custom.clone();
    }
    SHORTCUTS
        .iter()
        .find(|(a, _, _)| *a == action)
        .map(|(_, _, keys)| keys.iter().map(|k| (*k).to_owned()).collect())
        .unwrap_or_default()
}

pub fn setup(win: &Window, app: &gtk::Application) {
    let w = &win.inner.window;
    let add = |name: &str, f: Box<dyn Fn(&Window)>| {
        let action = gio::SimpleAction::new(name, None);
        let weak = std::rc::Rc::downgrade(&win.inner);
        action.connect_activate(move |_, _| {
            if let Some(inner) = weak.upgrade() {
                f(&Window { inner });
            }
        });
        w.add_action(&action);
    };

    add("open", Box::new(crate::dialogs::open_files));
    add("open-folder", Box::new(crate::dialogs::open_folder));
    add("close", Box::new(|w| w.window().close()));
    add("escape", Box::new(Window::escape));
    add(
        "animation-toggle",
        Box::new(|w| {
            if let Some(a) = w.inner.animator.borrow().as_ref() {
                a.toggle();
            }
        }),
    );
    add(
        "animation-step",
        Box::new(|w| {
            if let Some(a) = w.inner.animator.borrow().as_ref() {
                a.step();
            }
        }),
    );
    add("properties", Box::new(crate::properties::show));
    add("save", Box::new(Window::save));
    add("save-as", Box::new(Window::save_as));
    add("delete", Box::new(Window::delete));
    add("copy", Box::new(Window::copy_image));
    add("copy-path", Box::new(Window::copy_path));
    add("wallpaper", Box::new(Window::set_wallpaper));
    add("open-with", Box::new(Window::open_with));
    add("open-location", Box::new(Window::open_containing_folder));
    add("reload", Box::new(Window::reload));
    add("fit-window", Box::new(Window::fit_window_to_image));
    add("preferences", Box::new(crate::prefs::show));
    add("shortcuts", Box::new(crate::shortcuts::show));
    add("about", Box::new(show_about));
    add(
        "rotate-cw",
        Box::new(|w| w.transform(Orientation::rotate_cw)),
    );
    add(
        "rotate-ccw",
        Box::new(|w| w.transform(Orientation::rotate_ccw)),
    );
    add(
        "flip-horizontal",
        Box::new(|w| w.transform(Orientation::flip_horizontal)),
    );
    add(
        "flip-vertical",
        Box::new(|w| w.transform(Orientation::flip_vertical)),
    );
    add("next", Box::new(|w| w.navigate(1)));
    add("prev", Box::new(|w| w.navigate(-1)));
    add("first", Box::new(Window::first));
    add("last", Box::new(Window::last));
    add("zoom-in", Box::new(|w| w.inner.view.zoom_step(1)));
    add("zoom-out", Box::new(|w| w.inner.view.zoom_step(-1)));
    add("zoom-fit", Box::new(|w| w.inner.view.fit()));
    add("zoom-normal", Box::new(|w| w.inner.view.set_zoom(1.0)));
    add("zoom-2", Box::new(|w| w.inner.view.set_zoom(2.0)));
    add("zoom-3", Box::new(|w| w.inner.view.set_zoom(3.0)));

    // Sorting: a radio action for the key and a toggle for the direction.
    let sort = gio::SimpleAction::new_stateful(
        "sort-by",
        Some(glib::VariantTy::STRING),
        &sort_key_name(win.inner.config.borrow().sort_by.into()).to_variant(),
    );
    let weak = std::rc::Rc::downgrade(&win.inner);
    sort.connect_activate(move |a, param| {
        let (Some(inner), Some(key)) = (weak.upgrade(), param.and_then(glib::Variant::str)) else {
            return;
        };
        a.set_state(&key.to_variant());
        let key = match key {
            "modified" => SortKey::Modified,
            "size" => SortKey::Size,
            _ => SortKey::Name,
        };
        Window { inner }.set_sort(|o| o.key = key);
    });
    w.add_action(&sort);

    let desc = gio::SimpleAction::new_stateful(
        "sort-descending",
        None,
        &win.inner.config.borrow().sort_descending.to_variant(),
    );
    let weak = std::rc::Rc::downgrade(&win.inner);
    desc.connect_activate(move |a, _| {
        let Some(inner) = weak.upgrade() else { return };
        let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        a.set_state(&on.to_variant());
        Window { inner }.set_sort(|o| o.descending = on);
    });
    w.add_action(&desc);

    add_toggle(win, "fullscreen", false, |w, _| w.toggle_fullscreen());
    add_toggle(win, "slideshow", false, Window::set_slideshow);
    let c = win.inner.config.borrow().clone();
    add_toggle(
        win,
        "show-toolbar",
        c.show_toolbar,
        Window::set_toolbar_visible,
    );
    add_toggle(
        win,
        "show-statusbar",
        c.show_statusbar,
        Window::set_statusbar_visible,
    );
    add_toggle(
        win,
        "show-thumbnails",
        c.show_thumbnails,
        Window::set_thumbnails_visible,
    );

    for (action, _, _) in SHORTCUTS {
        let accels = accels_for(win, action);
        for a in &accels {
            if gtk::accelerator_parse(a).is_none() {
                tracing::warn!(action, accel = %a, "ignoring invalid shortcut");
            }
        }
        let refs: Vec<&str> = accels
            .iter()
            .map(String::as_str)
            .filter(|a| gtk::accelerator_parse(*a).is_some())
            .collect();
        app.set_accels_for_action(&format!("win.{action}"), &refs);
    }
}

/// A boolean stateful action. `f` receives the requested new state; it is
/// responsible for the state change where that is asynchronous (fullscreen).
fn add_toggle(win: &Window, name: &str, initial: bool, f: fn(&Window, bool)) {
    let action = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
    let weak = std::rc::Rc::downgrade(&win.inner);
    action.connect_activate(move |a, _| {
        let Some(inner) = weak.upgrade() else { return };
        let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        a.set_state(&on.to_variant());
        f(&Window { inner }, on);
    });
    win.inner.window.add_action(&action);
}

impl Window {
    fn set_sort(&self, f: impl FnOnce(&mut SortOrder)) {
        let mut opts = self.inner.scan_options.get();
        f(&mut opts.order);
        self.inner.scan_options.set(opts);
        self.inner.collection.borrow_mut().set_order(opts.order);
        {
            let mut c = self.inner.config.borrow_mut();
            c.sort_by = opts.order.key.into();
            c.sort_descending = opts.order.descending;
        }
        self.collection_changed();
        self.show_current();
    }
}

fn sort_key_name(k: SortKey) -> &'static str {
    match k {
        SortKey::Name => "name",
        SortKey::Modified => "modified",
        SortKey::Size => "size",
    }
}

/// Sets a boolean action's state without activating it.
pub fn set_action_state(window: &gtk::ApplicationWindow, name: &str, on: bool) {
    if let Some(a) = window
        .lookup_action(name)
        .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
    {
        a.set_state(&on.to_variant());
    }
}

fn show_about(win: &Window) {
    gtk::AboutDialog::builder()
        .transient_for(win.window())
        .modal(true)
        .program_name("rustnior")
        .logo_icon_name(crate::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("A fast, minimal image viewer")
        .license_type(gtk::License::Gpl30)
        .website("https://github.com/czM1K3/rustnior")
        .build()
        .present();
}

pub fn build_header(zoom_label: &gtk::Label) -> gtk::HeaderBar {
    let header = gtk::HeaderBar::new();
    let button = |icon: &str, action: &str, tip: &str| {
        gtk::Button::builder()
            .icon_name(icon)
            .action_name(action)
            .tooltip_text(tip)
            .focusable(false)
            .build()
    };
    header.pack_start(&button(
        "document-open-symbolic",
        "win.open",
        "Open Image (Ctrl+O)",
    ));
    let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    nav.add_css_class("linked");
    nav.append(&button(
        "go-previous-symbolic",
        "win.prev",
        "Previous (Backspace)",
    ));
    nav.append(&button("go-next-symbolic", "win.next", "Next (Space)"));
    header.pack_start(&nav);
    let rotate = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    rotate.add_css_class("linked");
    rotate.append(&button(
        "object-rotate-left-symbolic",
        "win.rotate-ccw",
        "Rotate Anticlockwise (Ctrl+Shift+R)",
    ));
    rotate.append(&button(
        "object-rotate-right-symbolic",
        "win.rotate-cw",
        "Rotate Clockwise (Ctrl+R)",
    ));
    header.pack_start(&rotate);

    let menu = gio::Menu::new();
    let files = gio::Menu::new();
    files.append(Some("Open Image…"), Some("win.open"));
    files.append(Some("Open Folder…"), Some("win.open-folder"));
    files.append(Some("Open With…"), Some("win.open-with"));
    files.append(Some("Open Containing Folder"), Some("win.open-location"));
    menu.append_section(None, &files);
    let edit = gio::Menu::new();
    edit.append(Some("Save"), Some("win.save"));
    edit.append(Some("Save As…"), Some("win.save-as"));
    edit.append(Some("Reload"), Some("win.reload"));
    edit.append(Some("Copy Image"), Some("win.copy"));
    edit.append(Some("Copy Path"), Some("win.copy-path"));
    edit.append(Some("Set as Wallpaper"), Some("win.wallpaper"));
    edit.append(Some("Move to Trash"), Some("win.delete"));
    menu.append_section(None, &edit);
    let sort = gio::Menu::new();
    sort.append(Some("By Name"), Some("win.sort-by::name"));
    sort.append(Some("By Date Modified"), Some("win.sort-by::modified"));
    sort.append(Some("By Size"), Some("win.sort-by::size"));
    sort.append(Some("Descending"), Some("win.sort-descending"));
    menu.append_submenu(Some("Sort Images"), &sort);
    let image = gio::Menu::new();
    image.append(Some("Properties"), Some("win.properties"));
    image.append(Some("Rotate Clockwise"), Some("win.rotate-cw"));
    image.append(Some("Rotate Anticlockwise"), Some("win.rotate-ccw"));
    image.append(Some("Flip Horizontally"), Some("win.flip-horizontal"));
    image.append(Some("Flip Vertically"), Some("win.flip-vertical"));
    menu.append_section(None, &image);
    let view = gio::Menu::new();
    view.append(Some("Toolbar"), Some("win.show-toolbar"));
    view.append(Some("Adjust Window Size"), Some("win.fit-window"));
    view.append(Some("Fullscreen"), Some("win.fullscreen"));
    view.append(Some("Slideshow"), Some("win.slideshow"));
    view.append(Some("Thumbnails"), Some("win.show-thumbnails"));
    view.append(Some("Status Bar"), Some("win.show-statusbar"));
    menu.append_section(None, &view);
    let close = gio::Menu::new();
    close.append(Some("Preferences"), Some("win.preferences"));
    close.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    close.append(Some("About rustnior"), Some("win.about"));
    close.append(Some("Close"), Some("win.close"));
    menu.append_section(None, &close);
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .focusable(false)
        .build();
    header.pack_end(&menu_button);
    header.pack_end(&button(
        "view-fullscreen-symbolic",
        "win.fullscreen",
        "Fullscreen (F11)",
    ));

    let zoom = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    zoom.add_css_class("linked");
    zoom.append(&button("zoom-out-symbolic", "win.zoom-out", "Zoom Out (-)"));
    zoom.append(&button(
        "zoom-fit-best-symbolic",
        "win.zoom-fit",
        "Best Fit (F)",
    ));
    zoom.append(&button(
        "zoom-original-symbolic",
        "win.zoom-normal",
        "Normal Size (1)",
    ));
    zoom.append(&button("zoom-in-symbolic", "win.zoom-in", "Zoom In (+)"));
    header.pack_end(zoom_label);
    header.pack_end(&zoom);
    header
}

/// Right-click menu for the image.
pub fn context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Open With…"), Some("win.open-with"));
    open.append(Some("Open Containing Folder"), Some("win.open-location"));
    menu.append_section(None, &open);
    let copy = gio::Menu::new();
    copy.append(Some("Copy Image"), Some("win.copy"));
    copy.append(Some("Copy Path"), Some("win.copy-path"));
    copy.append(Some("Set as Wallpaper"), Some("win.wallpaper"));
    menu.append_section(None, &copy);
    let image = gio::Menu::new();
    image.append(Some("Rotate Clockwise"), Some("win.rotate-cw"));
    image.append(Some("Rotate Anticlockwise"), Some("win.rotate-ccw"));
    image.append(Some("Flip Horizontally"), Some("win.flip-horizontal"));
    image.append(Some("Flip Vertically"), Some("win.flip-vertical"));
    menu.append_section(None, &image);
    let file = gio::Menu::new();
    file.append(Some("Save"), Some("win.save"));
    file.append(Some("Save As…"), Some("win.save-as"));
    file.append(Some("Move to Trash"), Some("win.delete"));
    menu.append_section(None, &file);
    let props = gio::Menu::new();
    props.append(Some("Toolbar"), Some("win.show-toolbar"));
    props.append(Some("Fullscreen"), Some("win.fullscreen"));
    props.append(Some("Properties"), Some("win.properties"));
    menu.append_section(None, &props);
    menu
}
