// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::path::PathBuf;

fn user_home() -> PathBuf {
    std::env::var_os("HOME").unwrap().into()
}

/// Return `XDG_CONFIG_HOME`.
pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| user_home().join(".config"), Into::into)
}
