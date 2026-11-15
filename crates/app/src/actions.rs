//! Window actions, their keyboard accelerators and the header bar.

use gtk::{gio, glib, prelude::*};
use rustnior_core::{SortKey, SortOrder};

use crate::window::Window;

/// (action, accelerators). Mirrors Viewnior's bindings plus common extras.
const ACCELS: &[(&str, &[&str])] = &[
    ("win.open", &["<Control>o"]),
    ("win.open-folder", &["<Control>f"]),
    ("win.close", &["<Control>w", "Escape", "q"]),
    (
        "win.next",
        &["space", "Page_Down", "<Alt>Right", "KP_Page_Down"],
    ),
    (
        "win.prev",
        &["BackSpace", "Page_Up", "<Alt>Left", "KP_Page_Up"],
    ),
    ("win.first", &["Home", "<Alt>Home", "KP_Home"]),
    ("win.last", &["End", "<Alt>End", "KP_End"]),
    (
        "win.zoom-in",
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
        "win.zoom-out",
        &[
            "minus",
            "KP_Subtract",
            "<Control>minus",
            "<Control>KP_Subtract",
        ],
    ),
    ("win.zoom-fit", &["f", "0", "KP_0"]),
    ("win.zoom-normal", &["1", "KP_1", "<Control>0"]),
    ("win.zoom-2", &["2", "KP_2"]),
    ("win.zoom-3", &["3", "KP_3"]),
];

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
        &"name".to_variant(),
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

    let desc = gio::SimpleAction::new_stateful("sort-descending", None, &false.to_variant());
    let weak = std::rc::Rc::downgrade(&win.inner);
    desc.connect_activate(move |a, _| {
        let Some(inner) = weak.upgrade() else { return };
        let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        a.set_state(&on.to_variant());
        Window { inner }.set_sort(|o| o.descending = on);
    });
    w.add_action(&desc);

    let status = gio::PropertyAction::new("show-statusbar", &win.inner.status.root, "visible");
    w.add_action(&status);

    for (action, accels) in ACCELS {
        app.set_accels_for_action(action, accels);
    }
}

impl Window {
    fn set_sort(&self, f: impl FnOnce(&mut SortOrder)) {
        let mut opts = self.inner.scan_options.get();
        f(&mut opts.order);
        self.inner.scan_options.set(opts);
        self.inner.collection.borrow_mut().set_order(opts.order);
        self.show_current();
    }
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

    let menu = gio::Menu::new();
    let files = gio::Menu::new();
    files.append(Some("Open Image…"), Some("win.open"));
    files.append(Some("Open Folder…"), Some("win.open-folder"));
    menu.append_section(None, &files);
    let sort = gio::Menu::new();
    sort.append(Some("By Name"), Some("win.sort-by::name"));
    sort.append(Some("By Date Modified"), Some("win.sort-by::modified"));
    sort.append(Some("By Size"), Some("win.sort-by::size"));
    sort.append(Some("Descending"), Some("win.sort-descending"));
    menu.append_submenu(Some("Sort Images"), &sort);
    let view = gio::Menu::new();
    view.append(Some("Status Bar"), Some("win.show-statusbar"));
    menu.append_section(None, &view);
    let close = gio::Menu::new();
    close.append(Some("Close"), Some("win.close"));
    menu.append_section(None, &close);
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Menu")
        .focusable(false)
        .build();
    header.pack_end(&menu_button);

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
