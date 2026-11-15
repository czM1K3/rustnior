//! Save, delete, clipboard, wallpaper and "open with" operations.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gtk::{gdk, gio, glib, prelude::*};
use rustnior_core::config::ModifyPref;
use rustnior_core::save::{SaveKind, SaveOptions, save_transformed};
use rustnior_core::wallpaper::{self, Desktop};
use rustnior_core::{DecodedImage, Orientation};

use crate::texture::frame_to_texture;
use crate::window::{Window, file_name};

/// Runs `f` on a worker thread and hands its result to `done` on the main loop.
fn run_in_background<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(f());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

impl Window {
    pub fn has_unsaved_changes(&self) -> bool {
        !self.inner.user_orientation.get().is_identity() && self.inner.view.has_image()
    }

    /// The decoded current image, if it is on screen.
    fn current_image(&self) -> Option<(PathBuf, Arc<DecodedImage>)> {
        let path = self.current_path()?;
        if self.inner.shown.borrow().as_ref() != Some(&path) {
            return None;
        }
        let img = self.inner.loader.cached(&path)?;
        Some((path, img))
    }

    /// Calls `then` once unsaved rotations of the current image have been
    /// saved or discarded, according to the "on modified" preference.
    pub fn leave_current(&self, then: impl FnOnce(&Window) + 'static) {
        if !self.has_unsaved_changes() || self.slideshow_running() {
            self.discard_changes();
            then(self);
            return;
        }
        let pref = self.inner.config.borrow().on_modified;
        match pref {
            ModifyPref::Ignore => {
                self.discard_changes();
                then(self);
            }
            ModifyPref::Save => self.save_then(then),
            ModifyPref::Ask => {
                let name = self
                    .current_path()
                    .map(|p| file_name(&p))
                    .unwrap_or_default();
                let dialog = gtk::AlertDialog::builder()
                    .modal(true)
                    .message(format!("Save changes to “{name}”?"))
                    .detail("The image was rotated or flipped.")
                    .buttons(["Cancel", "Discard", "Save"])
                    .cancel_button(0)
                    .default_button(2)
                    .build();
                let this = self.clone();
                dialog.choose(
                    Some(self.window()),
                    gio::Cancellable::NONE,
                    move |res| match res {
                        Ok(1) => {
                            this.discard_changes();
                            then(&this);
                        }
                        Ok(2) => this.save_then(then),
                        _ => {}
                    },
                );
            }
        }
    }

    fn discard_changes(&self) {
        self.inner.user_orientation.set(Orientation::IDENTITY);
    }

    /// Ctrl+S: writes the rotation/flip back to the file.
    pub fn save(&self) {
        if self.has_unsaved_changes() {
            self.save_then(|_| {});
        } else {
            self.inner.toast.show("No changes to save");
        }
    }

    fn save_then(&self, then: impl FnOnce(&Window) + 'static) {
        let Some((path, img)) = self.current_image() else {
            then(self);
            return;
        };
        let user = self.inner.user_orientation.get();
        let opts = self.save_options();
        let this = self.clone();
        let dest = path.clone();
        run_in_background(
            move || save_transformed(&path, &img, user, &path, &opts),
            move |res| match res {
                Ok(kind) => {
                    this.discard_changes();
                    this.after_file_changed(&dest);
                    this.inner.toast.show(match kind {
                        SaveKind::Lossless => "Saved (lossless)",
                        SaveKind::Reencoded => "Saved (re-encoded)",
                    });
                    then(&this);
                }
                Err(err) => this.error("Could not save the image", &err.to_string()),
            },
        );
    }

    /// Ctrl+Shift+S: writes the image as displayed to a new file; the format
    /// follows the extension.
    pub fn save_as(&self) {
        let Some((src, img)) = self.current_image() else {
            return;
        };
        let dialog = gtk::FileDialog::builder()
            .title("Save Image As")
            .modal(true)
            .initial_name(file_name(&src))
            .build();
        if let Some(dir) = src.parent() {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let user = self.inner.user_orientation.get();
        let opts = self.save_options();
        let this = self.clone();
        dialog.save(Some(self.window()), gio::Cancellable::NONE, move |res| {
            let Some(dest) = res.ok().and_then(|f| f.path()) else {
                return;
            };
            let same = dest == src;
            let shown = dest.clone();
            run_in_background(
                move || save_transformed(&src, &img, user, &dest, &opts),
                move |res| match res {
                    Ok(_) if same => {
                        this.discard_changes();
                        this.after_file_changed(&shown);
                        this.inner.toast.show("Saved");
                    }
                    Ok(_) => this
                        .inner
                        .toast
                        .show(&format!("Saved as {}", file_name(&shown))),
                    Err(err) => this.error("Could not save the image", &err.to_string()),
                },
            );
        });
    }

    fn save_options(&self) -> SaveOptions {
        let c = self.inner.config.borrow();
        SaveOptions {
            jpeg_quality: c.jpeg_quality,
            png_compression: c.png_compression,
        }
    }

    /// Re-reads a file that changed on disk and shows it again.
    fn after_file_changed(&self, path: &Path) {
        self.inner.loader.invalidate(path);
        self.inner.strip.invalidate(path);
        self.inner.collection.borrow_mut().refresh(path);
        if self.current_path().as_deref() == Some(path) {
            self.inner.shown.replace(None);
            self.show_current();
        }
    }

    /// Reloads the current file from disk (discarding unsaved changes).
    pub fn reload(&self) {
        if let Some(path) = self.current_path() {
            self.discard_changes();
            self.after_file_changed(&path);
        }
    }

    /// Delete: moves the current file to the trash, asking first if enabled.
    pub fn delete(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        if !self.inner.config.borrow().confirm_delete {
            self.trash(path);
            return;
        }
        let dialog = gtk::AlertDialog::builder()
            .modal(true)
            .message(format!("Move “{}” to the trash?", file_name(&path)))
            .buttons(["Cancel", "Move to Trash"])
            .cancel_button(0)
            .default_button(1)
            .build();
        let this = self.clone();
        dialog.choose(Some(self.window()), gio::Cancellable::NONE, move |res| {
            if res == Ok(1) {
                this.trash(path);
            }
        });
    }

    fn trash(&self, path: PathBuf) {
        if let Err(err) = trash::delete(&path) {
            self.error("Could not move the file to the trash", &err.to_string());
            return;
        }
        self.inner.loader.invalidate(&path);
        self.inner.strip.invalidate(&path);
        self.discard_changes();
        {
            let mut c = self.inner.collection.borrow_mut();
            if c.current().map(|e| e.path.as_path()) == Some(path.as_path()) {
                c.remove_current();
            }
        }
        self.inner.shown.replace(None);
        self.collection_changed();
        self.show_current();
        self.inner
            .toast
            .show(&format!("Moved {} to the trash", file_name(&path)));
    }

    /// Ctrl+C: copies the image as displayed (rotation applied).
    pub fn copy_image(&self) {
        let Some((_, img)) = self.current_image() else {
            return;
        };
        let Some(frame) = img.first_frame() else {
            return;
        };
        let total = img.orientation.then(self.inner.user_orientation.get());
        let texture = if total.is_identity() {
            frame_to_texture(frame)
        } else {
            frame_to_texture(&total.apply_to_frame(frame))
        };
        self.window().clipboard().set_texture(&texture);
        self.inner.toast.show("Image copied");
    }

    /// Ctrl+Shift+C: copies the file, as a path for text fields and as a file
    /// for file managers.
    pub fn copy_path(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        let file = gio::File::for_path(&path);
        let provider = gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_value(&gdk::FileList::from_array(&[file]).to_value()),
            gdk::ContentProvider::for_value(&path.display().to_string().to_value()),
        ]);
        if let Err(err) = self.window().clipboard().set_content(Some(&provider)) {
            tracing::warn!(%err, "clipboard");
        }
        self.inner.toast.show("Path copied");
    }

    /// Ctrl+F8: desktop-specific setter, then the XDG portal, then feh/nitrogen.
    pub fn set_wallpaper(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        let sandboxed = Path::new("/.flatpak-info").exists();
        let desktop = Desktop::detect(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref());
        let cmds = if sandboxed {
            Vec::new()
        } else {
            wallpaper::commands(desktop, &path)
        };
        let this = self.clone();
        run_in_background(
            move || !cmds.is_empty() && cmds.iter().all(|c| run(c)),
            move |ok| {
                if ok {
                    this.inner.toast.show("Wallpaper set");
                } else {
                    this.set_wallpaper_portal(path);
                }
            },
        );
    }

    fn set_wallpaper_portal(&self, path: PathBuf) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let uri = gio::File::for_path(&path).uri();
            let opts = glib::VariantDict::new(None);
            opts.insert("show-preview", false);
            opts.insert("set-on", "both");
            let params =
                glib::Variant::tuple_from_iter(["".to_variant(), uri.to_variant(), opts.end()]);
            let result = match gio::bus_get_future(gio::BusType::Session).await {
                Ok(conn) => conn
                    .call_future(
                        Some("org.freedesktop.portal.Desktop"),
                        "/org/freedesktop/portal/desktop",
                        "org.freedesktop.portal.Wallpaper",
                        "SetWallpaperURI",
                        Some(&params),
                        None,
                        gio::DBusCallFlags::NONE,
                        10_000,
                    )
                    .await
                    .map(|_| ()),
                Err(e) => Err(e),
            };
            match result {
                Ok(()) => this.inner.toast.show("Wallpaper change requested"),
                Err(err) => {
                    tracing::debug!(%err, "wallpaper portal unavailable");
                    let cmds = wallpaper::fallback_commands(&path);
                    let t = this.clone();
                    run_in_background(
                        move || cmds.iter().any(|c| run(c)),
                        move |ok| {
                            if ok {
                                t.inner.toast.show("Wallpaper set");
                            } else {
                                t.error(
                                    "Could not set the wallpaper",
                                    "No supported desktop, portal, feh or nitrogen was found.",
                                );
                            }
                        },
                    );
                }
            }
        });
    }

    /// "Open With…": lets the user pick an application.
    pub fn open_with(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
        launcher.set_always_ask(true);
        let this = self.clone();
        launcher.launch(Some(self.window()), gio::Cancellable::NONE, move |res| {
            if let Err(err) = res
                && !err.matches(gtk::DialogError::Dismissed)
            {
                this.error("Could not open the file", &err.to_string());
            }
        });
    }

    /// Opens the file manager with the current file selected.
    pub fn open_containing_folder(&self) {
        let Some(path) = self.current_path() else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
        let this = self.clone();
        launcher.open_containing_folder(Some(self.window()), gio::Cancellable::NONE, move |res| {
            if let Err(err) = res {
                this.error("Could not open the folder", &err.to_string());
            }
        });
    }

    pub fn error(&self, message: &str, detail: &str) {
        tracing::warn!(message, detail);
        gtk::AlertDialog::builder()
            .modal(true)
            .message(message)
            .detail(detail)
            .build()
            .show(Some(self.window()));
    }
}

fn run(cmd: &[String]) -> bool {
    let Some((prog, args)) = cmd.split_first() else {
        return false;
    };
    std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
