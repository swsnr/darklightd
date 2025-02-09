// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, clippy::all, clippy::pedantic,
    // Guard against left-over debugging output
    clippy::dbg_macro,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::unimplemented,
    clippy::use_debug,
    clippy::todo,
    clippy::undocumented_unsafe_blocks,
    // We must use tokio's APIs to exit the app.
    clippy::exit,
)]
#![forbid(unsafe_code)]

use std::io::ErrorKind;

use logcontrol_tracing::{PrettyLogControl1LayerFactory, TracingLogControl1};
use logcontrol_zbus::{logcontrol::LogControl1, ConnectionBuilderExt};
use monitor::spawn_color_scheme_monitor;
use tokio::{signal, sync::watch, task};
use tracing::{event, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

mod backend;
mod monitor;
mod portal;

use backend::{spawn_backends, ColorScheme};

/// Setup logging.
///
/// Set up logging to log to journald directly if the process runs under systemd.
///
/// When running interactively set up pretty-formatted console logging with a
/// standard `$RUST_LOG` environment filter.  If the env filter is active default
/// to [`Level::INFO`] in release builds, and [`Level::DEBUG`] level in debug
/// builds, i.e. under `cfg!(debug_assertions)`.
///
/// In either case, wrap a [`LogControl1`] layer around the logging setup and
/// return it, for exporting over D-Bus to change log level and log target
/// dynamically at runtime.
fn setup_logging() -> impl LogControl1 {
    // Setup env filter for convenient log control on console
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().ok();
    // If an env filter is set with $RUST_LOG use the lowest level as default for the control part,
    // to make sure the env filter takes precedence initially.
    let default_level = if env_filter.is_some() {
        Level::TRACE
    } else if cfg!(debug_assertions) {
        // In debug builds, e.g. local testing, log more by default
        Level::DEBUG
    } else {
        Level::INFO
    };
    let (control, control_layer) =
        TracingLogControl1::new_auto(PrettyLogControl1LayerFactory, default_level).unwrap();
    let subscriber = Registry::default().with(env_filter).with(control_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
    control
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_control = setup_logging();

    event!(
        Level::INFO,
        "{} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let connection = zbus::connection::Builder::session()?
        .serve_log_control(logcontrol_zbus::LogControl1::new(log_control))?
        .name("de.swsnr.darklightd")?
        .build()
        .await?;
    event!(Level::INFO, "Connected to bus");

    let (color_scheme_tx, color_scheme_rx) = watch::channel(ColorScheme::NoPreference);

    let mut backends = spawn_backends(&color_scheme_rx);
    let mut monitor_handle = spawn_color_scheme_monitor(connection.clone(), color_scheme_tx);
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

    let mut failed_tasks: Vec<(task::Id, Box<dyn std::error::Error>)> =
        Vec::with_capacity(backends.len() + 1);

    tokio::select! {
        result = signal::ctrl_c() => {
            if let Err(error) = result {
                event!(Level::ERROR, "Ctrl-C failed? {error}");
            } else {
                event!(Level::INFO, "Received SIGINT");
            }
            monitor_handle.abort();
        }
        _ = sigterm.recv() => {
            monitor_handle.abort();
        }
        result = &mut monitor_handle => {
            match result {
                // Track if the monitor task panicked or returned an error resulted
                Err(error) if error.is_panic() => failed_tasks.push((error.id(), error.into())),
                Ok(Err(error)) => failed_tasks.push((monitor_handle.id(), error.into())),
                _ => {}
            }
        }
        result = backends.join_next() => {
            if let Some(Err(error)) = result {
                // Track if the backend panicked or was aborted; we do not abort
                // backends so a backend being aborted is an error.
                failed_tasks.push((error.id(), error.into()));
            }
            // Abort monitoring if a backend failed; this will close the channel
            // and thus nominally stop all ongoing backends.  We do not abort
            // backends, because we'd like those that are still running to properly
            // finish applying the last colour scheme change.
            monitor_handle.abort();
        }
    }

    connection.graceful_shutdown().await;

    // Wait until applying the last scheme change is finished
    while let Some(result) = backends.join_next().await {
        if let Err(error) = result {
            if error.is_panic() {
                failed_tasks.push((error.id(), error.into()));
            }
        }
    }

    if failed_tasks.is_empty() {
        Ok(())
    } else {
        // Log all failed tasks and
        for (id, error) in &failed_tasks {
            event!(Level::ERROR, task.id = %id, "Task {id} failed to join: {error}");
        }
        Err(std::io::Error::new(
            ErrorKind::Other,
            format!("{} tasks failed", failed_tasks.len()),
        )
        .into())
    }
}
