//! File and folder choosers.

use gtk::{gio, prelude::*};

use crate::window::Window;

fn image_filters() -> gio::ListStore {
    let images = gtk::FileFilter::new();
    images.set_name(Some("Images"));
    images.add_mime_type("image/*");
    for ext in rustnior_core::decode::SUPPORTED_EXTENSIONS {
        images.add_suffix(ext);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some("All Files"));
    all.add_pattern("*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&images);
    filters.append(&all);
    filters
}

fn initial_folder(win: &Window) -> Option<gio::File> {
    win.current_path()
        .and_then(|p| p.parent().map(gio::File::for_path))
}

/// "Open Image…": one file browses its folder, several limit browsing to them.
pub fn open_files(win: &Window) {
    let dialog = gtk::FileDialog::builder()
        .title("Open Images")
        .modal(true)
        .filters(&image_filters())
        .build();
    if let Some(dir) = initial_folder(win) {
        dialog.set_initial_folder(Some(&dir));
    }
    let w = win.clone();
    dialog.open_multiple(Some(win.window()), gio::Cancellable::NONE, move |res| {
        let Ok(list) = res else { return };
        let paths: Vec<_> = list
            .iter::<gio::File>()
            .filter_map(|f| f.ok().and_then(|f| f.path()))
            .collect();
        w.open_paths(paths, None);
    });
}

pub fn open_folder(win: &Window) {
    let dialog = gtk::FileDialog::builder()
        .title("Open Folder")
        .modal(true)
        .build();
    if let Some(dir) = initial_folder(win) {
        dialog.set_initial_folder(Some(&dir));
    }
    let w = win.clone();
    dialog.select_folder(Some(win.window()), gio::Cancellable::NONE, move |res| {
        if let Some(path) = res.ok().and_then(|f| f.path()) {
            w.open_paths(vec![path], None);
        }
    });
}
