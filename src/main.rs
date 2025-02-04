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
    // Do not carelessly ignore errors
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
)]
#![forbid(unsafe_code)]

use logcontrol_tracing::{PrettyLogControl1LayerFactory, TracingLogControl1};
use logcontrol_zbus::ConnectionBuilderExt;
use tokio::signal::{
    ctrl_c,
    unix::{signal, SignalKind},
};
use tracing::{error, info, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

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
    info!("Connected to bus, listening for theme changes");

    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        result = ctrl_c() => {
            if let Err(error) = result {
                error!("Ctrl-C failed? {error}");
            } else {
                info!("Received SIGINT");
            }
        }
        _ = sigterm.recv() => {
        }
    }
    connection.graceful_shutdown().await;
    Ok(())
}
