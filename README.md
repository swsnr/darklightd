# darklightd

Monitor the desktop appearance settings for light/dark mode, and update application settings accordingly.

Whenever the desktop appearance changes this small daemon updates a settings which do not otherwise apply dark mode automatically:

- Change the legacy Gtk theme to `Adwaita-dark` when dark mode is enabled, and reset it to the default otherwise.
- Change the Helix theme (see below).

## Helix instructions.

To dynamically reconfigure Helix darklightd symlinks

- `~/.config/helix/themes/darklightd-default.toml` for no preference, or
- `~/.config/helix/themes/darklightd-light.toml` for light mode, or
- `~/.config/helix/themes/darklightd-dark.toml` for dark mode

to `~/.config/helix/themes/darklightd-auto.toml` whenever the colour scheme changes.
If the light or dark variants are missing it uses `darklightd-default.toml` instead.
To make use of this dynamically reconfigured theme set `theme = "darklightd-auto" in
`~/.config/helix/config.toml`.

For instance, to make helix follow the GNOME theme with Adwaita create the following
two files:

```toml
# ~/.config/helix/lightdarkd-default.toml
inherits = "adwaita-light"
```

```toml
# ~/.config/helix/lightdarkd-dark.toml
inherits = "adwaita-dark"
```

And then set `theme` in `~/.config/helix/config.toml`:

```toml
theme = "darklightd-auto"
```

## Installation

```console
$ cargo build release
$ run0 install -m755 target/release/lightdarkd /usr/local/bin/lightdarkd
$ run0 install -m644 systemd/lightdarkd.service /usr/local/lib/systemd/user/lightdarkd.service
$ systemctl --user daemon-reload
$ systemctl --user enable --now lightdarkd.service
```

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de>

This Source Code Form is subject to the terms of the Mozilla Public
License, v. 2.0. If a copy of the MPL was not distributed with this
file, You can obtain one at <http://mozilla.org/MPL/2.0/>.
