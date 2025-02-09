# darklightd

Monitor the desktop appearance settings for light/dark mode, and update application settings accordingly.

Whenever the desktop appearance changes this small daemon updates a settings which do not otherwise apply dark mode automatically:

- Change the legacy Gtk theme to `Adwaita-dark` when dark mode is enabled, and reset it to the default otherwise.

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
