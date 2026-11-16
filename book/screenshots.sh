#!/bin/sh
# Regenerates the manual's screenshots in book/src/images/.
#
#   book/screenshots.sh [path/to/niri-dash]
#
# Every picture is rendered offscreen by `niri-dash snapshot --mock`: made-up data, the shipped
# default layout (assets/config.kdl) and a US English locale, in a throwaway home directory, so
# your own config isn't touched. The theme is the one below (the Hack font has to be installed;
# without it the pictures come out in the default font).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
bin=${1:-$root/target/release/niri-dash}
out=$here/src/images

theme='theme colorscheme="gruvbox-dark" style="sharp" { font-family "Hack"; }'

home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
mkdir -p "$home/config/niri-dash" "$out"

# A shipped config as the dashboard's, with the pictures' theme in place of its own.
config() {
    sed "s|^theme .*|$theme|" "$root/assets/$1" > "$home/config/niri-dash/config.kdl"
}
config config.kdl

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

# The keyboard: `f` labels every widget; a label goes into that widget, and the tips along the
# bottom list its keys (`f`, `n`: the notifications); `e` expands it, with the tips under it
# (`f`, `c`, `a`, `e`: the calendar).
snap keys-labels --events "key:f"
snap keys-widget --events "key:f;key:n;wait:300"
snap keys-expanded --events "key:f;key:c;key:a;key:e;wait:500"

# A widget as an app of its own: the window is the widget, with its keys under it.
"$bin" snapshot --mock --width 400 --height 460 --scale 1 --app sleep-timer "$out/app-sleep-timer.png"

# The portrait layout (assets/config-portrait.kdl), on a screen taller than wide.
config config-portrait.kdl
width=1080
height=1920
snap portrait

# The second tutorial, "Your own layout": the clicks of its text, replayed from the default layout,
# one picture per step. The widget list is longer than a screen, so these use a tall one and keep
# its top left. Needs python3 with Pillow, for the crops. The clicks are places on the screen, so
# they are for this theme and font: with another, look where things are and set them anew.
remove="down:316,253;up:316,253;wait:500"
add="down:1606,2356;up:1606,2356;wait:600"
drag="down:1420,150;move:1000,300;move:300,330;move:49,320;up:49,320;wait:700"
shape="down:80,252;up:80,252;wait:600"
resize="down:262,385;move:300,385;move:346,385;up:346,385;wait:700"
pin="down:288,252;up:288,252;wait:500"

own() {
    name=$1
    events=$2
    crop=$3
    config config.kdl
    "$bin" snapshot --mock --width 1920 --height 2400 --scale 1 --edit --events "$events" "$out/$name.png"
    python3 -c 'import sys; from PIL import Image
path, box = sys.argv[1], tuple(map(int, sys.argv[2].split(",")))
Image.open(path).crop(box).save(path)' "$out/$name.png" "$crop"
}

grid=0,0,1560,900
own own-1-removed "$remove" $grid
own own-2-list "$remove" 1570,2215,1910,2372
own own-3-added "$remove;$add" $grid
own own-4-moved "$remove;$add;$drag" $grid
own own-5-rectangle "$remove;$add;$drag;$shape" $grid
own own-6-resized "$remove;$add;$drag;$shape;$resize" $grid
own own-7-pinned "$remove;$add;$drag;$shape;$resize;$pin" $grid
