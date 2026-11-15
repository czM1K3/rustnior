//! Commands that set the desktop wallpaper on common Linux desktops.
//!
//! The UI runs the first command list that succeeds, falling back to the XDG
//! wallpaper portal and finally to generic X11 setters.

use std::path::Path;

use crate::thumbnail::file_uri;

/// Desktop environments with a known wallpaper mechanism.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Gnome,
    Cinnamon,
    Mate,
    Xfce,
    Kde,
    Lxde,
    Lxqt,
    Sway,
    Hyprland,
    Unknown,
}

impl Desktop {
    /// Detects the desktop from `XDG_CURRENT_DESKTOP` (colon-separated list).
    pub fn detect(xdg_current_desktop: Option<&str>) -> Self {
        let Some(v) = xdg_current_desktop else {
            return Self::Unknown;
        };
        for name in v.split(':').map(str::to_ascii_lowercase) {
            let d = match name.as_str() {
                "gnome" | "unity" | "budgie" | "pantheon" | "gnome-classic" | "gnome-flashback" => {
                    Self::Gnome
                }
                "x-cinnamon" | "cinnamon" => Self::Cinnamon,
                "mate" => Self::Mate,
                "xfce" => Self::Xfce,
                "kde" => Self::Kde,
                "lxde" => Self::Lxde,
                "lxqt" => Self::Lxqt,
                "sway" => Self::Sway,
                "hyprland" => Self::Hyprland,
                _ => continue,
            };
            return d;
        }
        Self::Unknown
    }
}

/// Command lines (program + args) to run, in order, for `desktop`. Empty for
/// unknown desktops, where the caller should try the portal and fallbacks.
pub fn commands(desktop: Desktop, path: &Path) -> Vec<Vec<String>> {
    let p = path.display().to_string();
    let uri = file_uri(path);
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<String>>();
    match desktop {
        Desktop::Gnome => vec![
            s(&[
                "gsettings",
                "set",
                "org.gnome.desktop.background",
                "picture-uri",
                &uri,
            ]),
            s(&[
                "gsettings",
                "set",
                "org.gnome.desktop.background",
                "picture-uri-dark",
                &uri,
            ]),
            s(&[
                "gsettings",
                "set",
                "org.gnome.desktop.background",
                "picture-options",
                "zoom",
            ]),
        ],
        Desktop::Cinnamon => vec![s(&[
            "gsettings",
            "set",
            "org.cinnamon.desktop.background",
            "picture-uri",
            &uri,
        ])],
        Desktop::Mate => vec![s(&[
            "gsettings",
            "set",
            "org.mate.background",
            "picture-filename",
            &p,
        ])],
        Desktop::Xfce => vec![s(&[
            "sh",
            "-c",
            // Every monitor/workspace has its own last-image property.
            "for prop in $(xfconf-query -c xfce4-desktop -l | grep 'last-image$'); do \
             xfconf-query -c xfce4-desktop -p \"$prop\" -s \"$1\"; done",
            "sh",
            &p,
        ])],
        Desktop::Kde => {
            let script = format!(
                "for (const d of desktops()) {{ d.wallpaperPlugin = 'org.kde.image'; \
                 d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; \
                 d.writeConfig('Image', {}); }}",
                js_string(&uri)
            );
            vec![s(&[
                "dbus-send",
                "--session",
                "--dest=org.kde.plasmashell",
                "--type=method_call",
                "/PlasmaShell",
                "org.kde.PlasmaShell.evaluateScript",
                &format!("string:{script}"),
            ])]
        }
        Desktop::Lxde => vec![s(&["pcmanfm", "--set-wallpaper", &p])],
        Desktop::Lxqt => vec![s(&["pcmanfm-qt", "--set-wallpaper", &p])],
        Desktop::Sway => vec![s(&["swaymsg", "output", "*", "bg", &p, "fill"])],
        Desktop::Hyprland => vec![s(&["hyprctl", "hyprpaper", "reload", &format!(",{p}")])],
        Desktop::Unknown => Vec::new(),
    }
}

/// Generic setters tried last (X11 window managers).
pub fn fallback_commands(path: &Path) -> Vec<Vec<String>> {
    let p = path.display().to_string();
    vec![
        vec!["feh".into(), "--bg-fill".into(), p.clone()],
        vec![
            "nitrogen".into(),
            "--set-zoom-fill".into(),
            "--save".into(),
            p,
        ],
    ]
}

/// Quotes a string for JavaScript (KDE's evaluateScript).
fn js_string(v: &str) -> String {
    let mut out = String::from("'");
    for c in v.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_desktops() {
        assert_eq!(Desktop::detect(Some("ubuntu:GNOME")), Desktop::Gnome);
        assert_eq!(Desktop::detect(Some("KDE")), Desktop::Kde);
        assert_eq!(Desktop::detect(Some("X-Cinnamon")), Desktop::Cinnamon);
        assert_eq!(Desktop::detect(Some("weird")), Desktop::Unknown);
        assert_eq!(Desktop::detect(None), Desktop::Unknown);
    }

    #[test]
    fn gnome_sets_both_variants() {
        let c = commands(Desktop::Gnome, Path::new("/p/a b.jpg"));
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].last().map(String::as_str), Some("file:///p/a%20b.jpg"));
        assert_eq!(c[1][3], "picture-uri-dark");
    }

    #[test]
    fn kde_script_is_quoted() {
        let c = commands(Desktop::Kde, Path::new("/x/it's.png"));
        let arg = c[0].last().cloned().unwrap_or_default();
        assert!(
            arg.contains("'file:///x/it's.png'") || arg.contains("it\\'s"),
            "{arg}"
        );
    }

    #[test]
    fn xfce_passes_path_as_argument_not_code() {
        let c = commands(Desktop::Xfce, Path::new("/x/$(rm -rf).png"));
        assert_eq!(c[0].last().map(String::as_str), Some("/x/$(rm -rf).png"));
        assert!(!c[0][2].contains("rm -rf"));
    }
}
