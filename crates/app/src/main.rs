//! rustnior — a fast, minimal image viewer.

mod imageview;
mod texture;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use gtk::{gio, glib, prelude::*};
use rustnior_core::{DecodeRequest, Decoders};

use crate::window::{LoadResult, Window};

pub const APP_ID: &str = "io.github.rustnior.Rustnior";

#[derive(Parser, Debug, Clone, Default)]
#[command(version, about = "A fast, minimal image viewer")]
pub struct Cli {
    /// Start in fullscreen mode
    #[arg(long)]
    pub fullscreen: bool,
    /// Start a slideshow
    #[arg(long)]
    pub slideshow: bool,
    /// Images or folders to open
    pub paths: Vec<PathBuf>,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let decoders = Arc::new(Decoders::default());

    // Start decoding the first image before GTK initialises so the two overlap.
    let early = cli.paths.first().filter(|p| p.is_file()).map(|path| {
        let (tx, rx) = async_channel::bounded::<LoadResult>(1);
        let path = path.clone();
        let decoders = Arc::clone(&decoders);
        std::thread::spawn(move || {
            let result = decoders.decode_path(&path, &DecodeRequest::default());
            // The receiver only disappears if the app is shutting down.
            let _ = tx.send_blocking((path, result));
        });
        rx
    });

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let early = std::cell::RefCell::new(early);
    app.connect_activate(move |app| {
        let win = Window::new(app, Arc::clone(&decoders));
        if let Some(rx) = early.borrow_mut().take() {
            win.receive_early(rx);
        } else if let Some(path) = cli.paths.first() {
            win.open(path.clone());
        }
        win.present();
    });

    // GTK must not parse our arguments; clap already did.
    let status = app.run_with_args::<&str>(&[]);
    if status == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
