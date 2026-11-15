//! rustnior — a fast, minimal image viewer.

mod actions;
mod animation;
mod dialogs;
mod fileops;
mod fullscreen;
mod imageview;
mod input;
mod prefs;
mod properties;
mod settings;
mod shortcuts;
mod texture;
mod theme;
mod thumbstrip;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use gtk::{gio, glib, prelude::*};
use rustnior_core::decode::PreviewSink;
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

/// Process start, for `RUSTNIOR_TIMING=1` startup measurements.
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() -> ExitCode {
    START.get_or_init(std::time::Instant::now);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    timing::mark("args parsed");
    let decoders = Arc::new(Decoders::default());
    let config = settings::load();
    timing::mark("config loaded");

    // Start decoding the first image before GTK initialises so the two overlap.
    // A quick preview is sent first for large JPEGs (with libjpeg-turbo).
    let early = cli.paths.first().filter(|p| p.is_file()).map(|path| {
        let (tx, rx) = async_channel::unbounded::<EarlyLoad>();
        let path = path.clone();
        let decoders = Arc::clone(&decoders);
        let target = (config.window.monitor_width, config.window.monitor_height);
        std::thread::spawn(move || {
            let preview_tx = tx.clone();
            let req = DecodeRequest {
                target_size: Some(target),
                preview: Some(PreviewSink(Arc::new(move |img| {
                    let _ = preview_tx.send_blocking(EarlyLoad::Preview(img));
                }))),
                ..DecodeRequest::default()
            };
            let result = decoders.decode_path(&path, &req);
            // The receiver only disappears if the app is shutting down.
            let _ = tx.send_blocking(EarlyLoad::Full(result));
        });
        rx
    });

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let early = std::cell::RefCell::new(early);
    app.connect_startup(|_| {
        gtk::Window::set_default_icon_name(APP_ID);
        timing::mark("gtk startup");
    });
    app.connect_activate(move |app| {
        timing::mark("activate");
        let win = Window::new(app, Arc::clone(&decoders), config.clone());
        timing::mark("window built");
        if std::env::var_os("RUSTNIOR_TIMING").is_some() {
            timing::report_first_frames(&win);
        }
        win.present();
        timing::mark("presented");
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

/// Prints when the first frame and the first image frame were painted.
mod timing {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gtk::prelude::*;

    /// Prints a startup milestone when `RUSTNIOR_TIMING` is set.
    pub fn mark(what: &str) {
        if std::env::var_os("RUSTNIOR_TIMING").is_some() {
            let ms = crate::START
                .get()
                .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
            eprintln!("timing: {what} at {ms:.1} ms");
        }
    }

    pub fn report_first_frames(win: &crate::window::Window) {
        let view = win.inner.view.clone();
        win.window().connect_realize(move |w| {
            let Some(clock) = w.frame_clock() else { return };
            let first = Cell::new(true);
            let view = view.clone();
            let id: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> = Rc::default();
            let id2 = Rc::clone(&id);
            let handler = clock.connect_after_paint(move |clock| {
                let ms = crate::START
                    .get()
                    .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
                if first.replace(false) {
                    eprintln!("timing: first frame painted at {ms:.1} ms");
                }
                if view.has_image() {
                    eprintln!("timing: first image frame painted at {ms:.1} ms");
                    if let Some(h) = id2.take() {
                        clock.disconnect(h);
                    }
                }
            });
            id.replace(Some(handler));
        });
    }
}
