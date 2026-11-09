# niri-dash, the Slint variant

This directory is niri-dash as it was written on [Slint](https://slint.dev), before the dashboard
moved to GPUI (the repository's root crate, see [`../README.md`](../README.md)). It is kept for
comparison with the GPUI version and because it is the only variant that builds for a browser (the
[web version](https://barafael.github.io/niri-dash/)). It is not released.

It is a workspace of its own (it patches Slint's software renderer, which only a workspace root can),
with its own `target/`. It shares `../crates/system` with the GPUI crate, and `../book` and
`../testbox` with the rest of the repository. Build and run it from here:

```sh
cargo run                                  # niri-dash-slint, the daemon
NIRI_DASH_MOCK=1 cargo run -- daemon --show
cargo test
trunk serve                                # the web version (rustup target add wasm32-unknown-unknown)
cargo run -- snapshot --mock --edit out.png    # a picture, drawn without a display
cargo run -- stress                            # the keyboard stress run
```

`../scripts/vshot.sh` runs this program or the GPUI one on a private virtual compositor and takes
screenshots, and `../book/screenshots.sh BIN OUTDIR` renders the manual's pictures with either.
[`../PORTING.md`](../PORTING.md) says where each part of this version is in the GPUI one.

## License

MIT or Apache-2.0 for the code in this repository, and Slint under its [Royalty-free Desktop,
Mobile, and Web Applications
License](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md)
(or the GPL-3.0). That license asks for attribution: the "Made with Slint" badge on the web version
and the About widget. Anything redistributed from this directory keeps them. The patched copy of
Slint's software renderer in `patches/i-slint-renderer-software` keeps Slint's own licenses, in its
`LICENSES` directory.
