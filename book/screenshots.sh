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

# A 1920x1080 screen at scale 1: big enough for the default grid with the edit sidebar open.
width=1920
height=1080

snap() {
    name=$1
    shift
    "$bin" snapshot --mock --width "$width" --height "$height" --scale 1 "$@" "$out/$name.png"
}

snap dashboard
snap edit-mode --edit
snap edit-colors --edit --tab colors
snap edit-style --edit --tab style
snap edit-arrange --edit --tab arrange
snap widget-settings --edit --configure 0
snap edit-arranged --edit --tab arrange --arrange

# The same layout in other color schemes and styles (the README shows these).
snap style-catppuccin-soft --colorscheme catppuccin-mocha --style soft
snap style-gruvbox-sharp --colorscheme gruvbox-light --style sharp
snap style-nord-compact --colorscheme nord --style compact
snap style-rose-pine-dawn --colorscheme rose-pine-dawn --style default

# The keyboard: `f` labels every widget; `f`, `c`, `a`, `e` goes into the calendar and expands it.
snap keys-labels --events "key:f"
snap keys-expanded --events "key:f;key:c;key:a;key:e"

# The portrait layout (assets/config-portrait.kdl), on a screen taller than wide.
cp "$root/assets/config-portrait.kdl" "$home/config/niri-dash/config.kdl"
width=1080
height=1920
snap portrait

# The second tutorial, "Your own layout": the clicks of its text, replayed from the default layout,
# one picture per step. The widget list is longer than a screen, so these use a tall one and keep
# its top left. Needs python3 with Pillow, for the crops.
remove="down:316,253;up:316,253;wait:500"
add="down:1600,2269;up:1600,2269;wait:600"
drag="down:1440,170;move:1000,300;move:300,340;move:66,342;up:66,342;wait:700"
shape="down:79,252;up:79,252;wait:600"
resize="down:260,381;move:300,381;move:345,381;up:345,381;wait:700"
pin="down:284,252;up:284,252;wait:500"

own() {
    name=$1
    events=$2
    crop=$3
    cp "$root/assets/config.kdl" "$home/config/niri-dash/config.kdl"
    "$bin" snapshot --mock --width 1920 --height 2400 --scale 1 --edit --events "$events" "$out/$name.png"
    python3 -c 'import sys; from PIL import Image
path, box = sys.argv[1], tuple(map(int, sys.argv[2].split(",")))
Image.open(path).crop(box).save(path)' "$out/$name.png" "$crop"
}

grid=0,0,1560,900
own own-1-removed "$remove" $grid
own own-2-list "$remove" 1570,2200,1910,2340
own own-3-added "$remove;$add" $grid
own own-4-moved "$remove;$add;$drag" $grid
own own-5-rectangle "$remove;$add;$drag;$shape" $grid
own own-6-resized "$remove;$add;$drag;$shape;$resize" $grid
own own-7-pinned "$remove;$add;$drag;$shape;$resize;$pin" $grid
