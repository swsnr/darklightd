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

use futures::{future, StreamExt, TryFutureExt};
use logcontrol_tracing::{PrettyLogControl1LayerFactory, TracingLogControl1};
use logcontrol_zbus::ConnectionBuilderExt;
use tokio::{
    signal::{
        ctrl_c,
        unix::{signal, SignalKind},
    },
    sync::watch,
    task::{Id, JoinSet},
};
use tokio_stream::wrappers::WatchStream;
use tracing::{debug, error, info, info_span, Instrument, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

mod backend;
mod portal;

use backend::{gtk, ColorScheme};

async fn receive_color_scheme_changes(
    settings: portal::SettingsProxy<'_>,
    sender: watch::Sender<ColorScheme>,
) -> zbus::Result<()> {
    let mut changed_stream = settings.receive_setting_changed().await?;
    while let Some(change) = changed_stream.next().await {
        let args = change.args()?;
        if *args.namespace() == "org.freedesktop.appearance" && *args.key() == "color-scheme" {
            let raw_value = u32::try_from(args.value())?;
            let color_scheme = ColorScheme::from(raw_value);
            debug!("org.freedesktop.appearance color-scheme changed to {raw_value} parsed as {color_scheme:?}");
            if *sender.borrow() != color_scheme && sender.send(color_scheme).is_err() {
                // If no one's listening anymore just stop receiving changes
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn monitor_color_scheme_changes(
    connection: zbus::Connection,
    sender: watch::Sender<ColorScheme>,
) -> Result<(), zbus::Error> {
    let settings = portal::SettingsProxy::builder(&connection)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    info!("Connected to settings portal, reading current color scheme from org.freedesktop.appearance color-scheme");
    let reply = settings
        .read_one("org.freedesktop.appearance", "color-scheme")
        .await?;
    let color_scheme = u32::try_from(reply)?.into();
    // We deliberately send the initial value to make the current scheme apply
    if sender.send(color_scheme).is_ok() {
        info!("Watching for color scheme changes");
        receive_color_scheme_changes(settings, sender).await
    } else {
        Ok(())
    }
}

fn spawn_color_scheme_monitor(
    connection: zbus::Connection,
    sender: watch::Sender<ColorScheme>,
) -> tokio::task::JoinHandle<zbus::Result<()>> {
    tokio::spawn(async move {
        monitor_color_scheme_changes(connection, sender)
            .instrument(info_span!("settings-watcher", task.id = %tokio::task::id()).or_current())
            .await
    })
}

fn spawn_watchers(color_scheme_rx: &watch::Receiver<ColorScheme>) -> JoinSet<()> {
    let watcher_span = info_span!("watchers").or_current();
    let mut watchers = JoinSet::new();
    watchers.spawn(
        WatchStream::from_changes(color_scheme_rx.clone())
            .for_each(|color_scheme| {
                info!(task.id = %tokio::task::id(), "Color scheme updated to {color_scheme:?}");
                future::ready(())
            })
            .instrument(info_span!(parent: &watcher_span, "watcher.log")),
    );
    watchers.spawn(
        WatchStream::from_changes(color_scheme_rx.clone())
            .for_each(|color_scheme| {
                gtk::apply_color_scheme(color_scheme)
                    .inspect_err(move |error| {
                        error!("Failed to apply color scheme {color_scheme:?} to Gtk: {error}");
                    })
                    .unwrap_or_else(|_| ())
                    .instrument(
                        info_span!("watcher.gtk", task.id = %tokio::task::id()).or_current(),
                    )
            })
            .instrument(info_span!(parent: &watcher_span, "watcher.gtk")),
    );

    watchers
}

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

    let mut watchers = spawn_watchers(&color_scheme_rx);
    let mut monitor_handle = spawn_color_scheme_monitor(connection.clone(), color_scheme_tx);
    let mut sigterm = signal(SignalKind::terminate())?;

    let mut failed_tasks: Vec<(Id, Box<dyn std::error::Error>)> =
        Vec::with_capacity(watchers.len() + 1);

    tokio::select! {
        result = ctrl_c() => {
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
        result = watchers.join_next() => {
            if let Some(Err(error)) = result {
                failed_tasks.push((error.id(), error.into()));
            }
            // If a watcher failed abort monitoring; this will close the channel
            // and thus nominally stop all ongoing watchers.  We do not abort
            // watchers, because we'd like those that are still running to properly
            // finish applying the last colour scheme change.
            monitor_handle.abort();
        }
    }

    connection.graceful_shutdown().await;
    // Wait until applying the last scheme change is finished
    while let Some(result) = watchers.join_next().await {
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
