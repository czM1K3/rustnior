//! Light/dark theme: explicit choice, or the desktop's preference read from
//! the XDG settings portal (and followed live).

use std::cell::RefCell;

use gtk::{gio, glib, prelude::*};
use rustnior_core::config::ThemePref;

thread_local! {
    /// Keeps the portal proxy (and its signal handler) alive.
    static PORTAL: RefCell<Option<gio::DBusProxy>> = const { RefCell::new(None) };
    static CSS_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const TRANSLUCENT_CLASS: &str = "rustnior-translucent";

/// The window normally paints the theme background under everything; a
/// translucent window drops it so only the image view's own (translucent)
/// background colour is drawn.
///
/// On-screen controls sit on a blurred copy of the image (`blur::BlurBin`),
/// so they get a lighter tint and a radius matching `blur::CORNER_RADIUS`.
const CSS: &str = "
window.rustnior-translucent { background: none; }
blurbin > .osd { border-radius: 12px; background-color: rgba(0, 0, 0, 0.45); }
";

/// Adds the app's style sheet to the default display (once).
pub fn install_css() {
    if CSS_INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    // Above user priority: `~/.config/gtk-4.0/gtk.css` (often a symlinked
    // theme) styles `.background` there and would otherwise keep the window
    // opaque. The rule only applies once the user opts in.
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}

/// Lets the desktop show through the window behind the image. Needs a
/// compositor; without one the uncovered area is black.
pub fn set_translucent(window: &impl IsA<gtk::Widget>, on: bool) {
    let window = window.as_ref();
    if on {
        window.add_css_class(TRANSLUCENT_CLASS);
    } else {
        window.remove_css_class(TRANSLUCENT_CLASS);
    }
}

const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// `GtkInterfaceColorScheme` values (GTK ≥ 4.20).
const SCHEME_DEFAULT: i32 = 1;
const SCHEME_DARK: i32 = 2;
const SCHEME_LIGHT: i32 = 3;

/// Sets `gtk-interface-color-scheme` when the running GTK has it (≥ 4.20,
/// which also follows the desktop by itself). We build against 4.14, so the
/// property is set by name. Returns false on older GTK.
fn set_interface_color_scheme(settings: &gtk::Settings, value: i32) -> bool {
    let name = "gtk-interface-color-scheme";
    let Some(pspec) = settings.find_property(name) else {
        return false;
    };
    let Some(v) = glib::EnumClass::with_type(pspec.value_type()).and_then(|c| c.to_value(value))
    else {
        return false;
    };
    settings.set_property_from_value(name, &v);
    true
}

pub fn apply(pref: ThemePref) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let scheme = match pref {
        ThemePref::System => SCHEME_DEFAULT,
        ThemePref::Light => SCHEME_LIGHT,
        ThemePref::Dark => SCHEME_DARK,
    };
    if set_interface_color_scheme(&settings, scheme) {
        // A global `gtk-application-prefer-dark-theme=true` (settings.ini)
        // still forces dark, so clear it for an explicit Light choice. It is
        // only touched then, because GTK ≥ 4.20 warns that it is deprecated.
        if pref == ThemePref::Light && settings.is_gtk_application_prefer_dark_theme() {
            settings.set_gtk_application_prefer_dark_theme(false);
        }
        tracing::debug!(
            ?pref,
            scheme = ?settings.property_value("gtk-interface-color-scheme"),
            prefer_dark = settings.is_gtk_application_prefer_dark_theme(),
            theme = ?settings.gtk_theme_name(),
            "theme applied"
        );
        return;
    }
    match pref {
        ThemePref::Light => settings.set_gtk_application_prefer_dark_theme(false),
        ThemePref::Dark => settings.set_gtk_application_prefer_dark_theme(true),
        ThemePref::System => follow_system(settings),
    }
}

/// Portal value: 0 = no preference, 1 = dark, 2 = light.
fn set_scheme(settings: &gtk::Settings, value: u32) {
    match value {
        1 => settings.set_gtk_application_prefer_dark_theme(true),
        2 => settings.set_gtk_application_prefer_dark_theme(false),
        _ => {} // Keep whatever the user's GTK settings say.
    }
}

/// Unwraps nested variants (`v`, `(v)`) down to a `u32`.
fn as_u32(v: &glib::Variant) -> Option<u32> {
    if let Some(u) = v.get::<u32>() {
        return Some(u);
    }
    if v.is_type(glib::VariantTy::VARIANT)
        && let Some(inner) = v.as_variant()
    {
        return as_u32(&inner);
    }
    (v.n_children() == 1)
        .then(|| v.child_value(0))
        .and_then(|c| as_u32(&c))
}

fn follow_system(settings: gtk::Settings) {
    if PORTAL.with(|p| p.borrow().is_some()) {
        // Already subscribed; just re-read the current value.
        if let Some(proxy) = PORTAL.with(|p| p.borrow().clone()) {
            read(proxy, settings);
        }
        return;
    }
    glib::spawn_future_local(async move {
        let proxy = match gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )
        .await
        {
            Ok(p) => p,
            Err(err) => {
                tracing::debug!(%err, "settings portal unavailable");
                return;
            }
        };
        let s = settings.clone();
        proxy.connect_local("g-signal", false, move |args| {
            let name = args.get(2).and_then(|v| v.get::<String>().ok());
            let params = args.get(3).and_then(|v| v.get::<glib::Variant>().ok());
            if name.as_deref() == Some("SettingChanged")
                && let Some(p) = params
                && p.n_children() == 3
                && p.child_value(0).str() == Some(NAMESPACE)
                && p.child_value(1).str() == Some(KEY)
                && let Some(v) = as_u32(&p.child_value(2))
            {
                set_scheme(&s, v);
            }
            None
        });
        read(proxy.clone(), settings);
        PORTAL.with(|p| p.replace(Some(proxy)));
    });
}

fn read(proxy: gio::DBusProxy, settings: gtk::Settings) {
    glib::spawn_future_local(async move {
        let args = (NAMESPACE, KEY).to_variant();
        // ReadOne is the current API; Read is the deprecated one older portals have.
        let result = match proxy
            .call_future("ReadOne", Some(&args), gio::DBusCallFlags::NONE, 2000)
            .await
        {
            Ok(v) => Ok(v),
            Err(_) => {
                proxy
                    .call_future("Read", Some(&args), gio::DBusCallFlags::NONE, 2000)
                    .await
            }
        };
        match result.as_ref().ok().and_then(as_u32) {
            Some(v) => set_scheme(&settings, v),
            None => tracing::debug!("no color-scheme from portal"),
        }
    });
}
