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
use logcontrol_zbus::ConnectionBuilderExt;
use monitor::spawn_color_scheme_monitor;
use tokio::{signal, sync::watch, task};
use tracing::{error, info, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

mod backend;
mod monitor;
mod portal;

use backend::{spawn_backends, ColorScheme};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        TracingLogControl1::new_auto(PrettyLogControl1LayerFactory, default_level)?;
    let subscriber = Registry::default().with(env_filter).with(control_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    let connection = zbus::connection::Builder::session()?
        .serve_log_control(logcontrol_zbus::LogControl1::new(control))?
        .name("de.swsnr.darklightd")?
        .build()
        .await?;
    info!("Connected to bus");

    let (color_scheme_tx, color_scheme_rx) = watch::channel(ColorScheme::NoPreference);

    let mut backends = spawn_backends(&color_scheme_rx);
    let mut monitor_handle = spawn_color_scheme_monitor(connection.clone(), color_scheme_tx);
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

    let mut failed_tasks: Vec<(task::Id, Box<dyn std::error::Error>)> =
        Vec::with_capacity(backends.len() + 1);

    tokio::select! {
        result = signal::ctrl_c() => {
            if let Err(error) = result {
                error!("Ctrl-C failed? {error}");
            } else {
                info!("Received SIGINT");
            }
            monitor_handle.abort();
        }
        _ = sigterm.recv() => {
            monitor_handle.abort();
        }
        result = &mut monitor_handle => {
            match result {
                Err(error) if error.is_panic() => failed_tasks.push((error.id(), error.into())),
                Ok(Err(error)) => failed_tasks.push((monitor_handle.id(), error.into())),
                _ => {}
            }
        }
        result = backends.join_next() => {
            if let Some(Err(error)) = result {
                failed_tasks.push((error.id(), error.into()));
            }
            // If a backend failed abort monitoring; this will close the channel
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
        for (id, error) in &failed_tasks {
            error!(task.id = %id, "Task {id} failed to join: {error}");
        }
        Err(std::io::Error::new(
            ErrorKind::Other,
            format!("{} tasks failed", failed_tasks.len()),
        )
        .into())
    }
}
