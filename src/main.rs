//! Cache Deck: a desktop client for Redis and Valkey.

// Windows release builds: don't open a console window next to the app. The price is that
// `--help` and error messages print nothing there; debug builds keep the console.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod commands;
mod config;
mod platform;
mod theme;
mod worker;

use eframe::egui;

fn parse_args() -> Result<app::Options, String> {
    let mut opts = app::Options {
        screenshot: None,
        connect: None,
        select: None,
        tab: None,
        light: false,
        run: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => {
                opts.screenshot = Some(args.next().ok_or("--screenshot needs a file name")?.into());
            }
            "--connect" => {
                let v = args.next().ok_or("--connect needs host:port")?;
                let (h, p) = v.rsplit_once(':').unwrap_or((v.as_str(), "6379"));
                let port = p.parse().map_err(|_| format!("bad port in {v}"))?;
                opts.connect = Some((h.to_string(), port));
            }
            "--select" => opts.select = Some(args.next().ok_or("--select needs a key")?),
            "--tab" => opts.tab = Some(args.next().ok_or("--tab needs keys, commands or info")?),
            "--light" => opts.light = true,
            "--run" => opts.run.push(args.next().ok_or("--run needs a command")?),
            "-h" | "--help" => {
                println!(
                    "Cache Deck {}\n\nUsage: cache-deck [options]\n  --screenshot <file.png>  draw ~120 frames, save the window as PNG, exit\n  --connect <host:port>    connect right away (not saved)\n  --select <key>           open this key after connecting\n  --tab <keys|commands|info>\n  --light                  start in the light theme\n  --run <command>          run a command in the Commands tab after connecting",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => return Err(format!("Unknown option: {other}")),
        }
    }
    Ok(opts)
}

fn main() -> eframe::Result {
    // Secure (TLS) connections need a crypto backend chosen before the first one is opened;
    // without it the connecting thread panics. An error here only means one is already set.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let shot = opts.screenshot.is_some();

    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon/png/icon-256.png")).expect("app icon");
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Cache Deck")
        .with_app_id("cache-deck")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([900.0, 560.0])
        .with_icon(icon);

    let mut native = eframe::NativeOptions::default();
    if shot {
        // Open quietly: never steal focus from what the person is doing.
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
        #[cfg(target_os = "macos")]
        {
            native.event_loop_builder = Some(Box::new(|builder| {
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                builder.with_activate_ignoring_other_apps(false);
            }));
        }
    }
    native.viewport = viewport;

    // The Mac menu (About / Hide / Quit) takes the process name; show the real app name.
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSProcessInfo, NSString};
        NSProcessInfo::processInfo().setProcessName(&NSString::from_str("Cache Deck"));
    }

    eframe::run_native(
        "Cache Deck",
        native,
        Box::new(move |cc| Ok(Box::new(app::CacheDeck::new(cc, opts)))),
    )
}
