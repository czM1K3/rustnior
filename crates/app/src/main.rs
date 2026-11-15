//! rustnior — a fast, minimal image viewer.

mod actions;
mod animation;
mod dialogs;
mod fileops;
mod fullscreen;
mod imageview;
mod input;
mod properties;
mod texture;
mod thumbstrip;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use gtk::{gio, glib, prelude::*};
use rustnior_core::{DecodeRequest, Decoders};

use crate::window::{EarlyLoad, Window};

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
        let (tx, rx) = async_channel::bounded::<EarlyLoad>(1);
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
        win.present();
        win.open_paths(cli.paths.clone(), early.borrow_mut().take());
        if cli.slideshow {
            win.set_slideshow(true);
        } else if cli.fullscreen {
            win.window().fullscreen();
        }
        #[cfg(debug_assertions)]
        smoke::run_script(&win);
    });

    // GTK must not parse our arguments; clap already did.
    let status = app.run_with_args::<&str>(&[]);
    if status == glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Debug builds only: `RUSTNIOR_SMOKE="next,zoom-in,…"` activates window
/// actions one by one (every `RUSTNIOR_SMOKE_MS`, default 400 ms) so the UI
/// can be exercised headlessly.
#[cfg(debug_assertions)]
mod smoke {
    use gtk::{glib, prelude::*};

    pub fn run_script(win: &crate::window::Window) {
        let Ok(script) = std::env::var("RUSTNIOR_SMOKE") else {
            return;
        };
        let ms = std::env::var("RUSTNIOR_SMOKE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400);
        let mut steps: std::collections::VecDeque<String> =
            script.split(',').map(|s| s.trim().to_owned()).collect();
        let window = win.window().clone();
        glib::timeout_add_local(std::time::Duration::from_millis(ms), move || {
            let Some(step) = steps.pop_front() else {
                return glib::ControlFlow::Break;
            };
            let (name, arg) = match step.split_once("::") {
                Some((n, a)) => (n.to_owned(), Some(a.to_variant())),
                None => (step.clone(), None),
            };
            tracing::info!(%step, "smoke step");
            if let Err(err) =
                WidgetExt::activate_action(&window, &format!("win.{name}"), arg.as_ref())
            {
                tracing::error!(%step, %err, "smoke step failed");
            }
            glib::ControlFlow::Continue
        });
    }
}
