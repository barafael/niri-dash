#!/bin/sh
# Regenerates the manual's screenshots in book/src/images/.
#
#   book/screenshots.sh [path/to/niri-dash]
#
# Every picture is rendered offscreen by `niri-dash snapshot --mock`: made-up data, the shipped
# default layout (assets/config.kdl) and a US English locale, in a throwaway home directory, so
# the result doesn't depend on whose machine it runs on and your own config isn't touched.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
bin=${1:-$root/target/release/niri-dash}
out=$here/src/images

home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
mkdir -p "$home/config/niri-dash" "$out"
cp "$root/assets/config.kdl" "$home/config/niri-dash/config.kdl"

export NIRI_DASH_HOME=$home
export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8

snap() {
    name=$1
    shift
    # A 1920x1080 screen at scale 1: big enough for the default grid with the edit sidebar open.
    "$bin" snapshot --mock --width 1920 --height 1080 --scale 1 "$@" "$out/$name.png"
}

snap dashboard
snap edit-mode --edit
snap edit-colors --edit --tab colors
snap edit-style --edit --tab style
snap edit-arrange --edit --tab arrange
snap widget-settings --edit --configure 0

# The keyboard: `f` labels every widget; `f`, `c`, `a`, `e` goes into the calendar and expands it.
snap keys-labels --events "key:f"
snap keys-expanded --events "key:f;key:c;key:a;key:e"
