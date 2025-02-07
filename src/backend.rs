// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

mod types;

use std::future;

use futures::{StreamExt, TryFutureExt};
use tokio::{sync::watch, task::JoinSet};
use tokio_stream::wrappers::WatchStream;
use tracing::{event, span, Instrument, Level};

pub use types::ColorScheme;
pub mod gtk;

pub fn spawn_backends(color_scheme_rx: &watch::Receiver<ColorScheme>) -> JoinSet<()> {
    let backends_span = span!(Level::INFO, "backends").or_current();
    let mut backends = JoinSet::new();
    backends.spawn(
        WatchStream::from_changes(color_scheme_rx.clone())
            .for_each(|color_scheme| {
                event!(Level::INFO, task.id = %tokio::task::id(), "Color scheme updated to {color_scheme:?}");
                future::ready(())
            })
            .instrument(span!(parent: &backends_span, Level::INFO, "backend.log")),
    );
    backends.spawn(
        WatchStream::from_changes(color_scheme_rx.clone())
            .for_each(|color_scheme| {
                gtk::apply_color_scheme(color_scheme)
                    .inspect_err(move |error| {
                        event!(
                            Level::ERROR,
                            "Failed to apply color scheme {color_scheme:?} to Gtk: {error}"
                        );
                    })
                    .unwrap_or_else(|_| ())
                    .instrument(
                        span!(Level::INFO, "backend.gtk", task.id = %tokio::task::id())
                            .or_current(),
                    )
            })
            .instrument(span!(parent: &backends_span, Level::INFO, "backend.gtk")),
    );

    backends
}
