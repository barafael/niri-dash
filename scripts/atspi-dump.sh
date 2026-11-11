#!/bin/bash
# Print what a screen reader is given by a niri-dash binary (the GPUI one or the Slint one): its
# AT-SPI tree, read by libatspi. Runs the binary on the mock system on a private virtual KWin and
# a private session bus with its own accessibility bus. Never touches the desktop session: its
# own dbus session, compositor, runtime directory and XDG directories (like vshot.sh).
#
# usage: scripts/atspi-dump.sh BIN APP_PREFIX [CONFIG.kdl]
#
#   BIN         target/gpui/debug/niri-dash-gpui (GPUI), or target/debug/niri-dash (Slint)
#   APP_PREFIX  how the application is called on the bus (start of its name): "niri-dash"
#   CONFIG      the config.kdl to show (default: one widget, KIND=wifi, 8x7 cells)
#
# environment: KIND=widget for the default config, WAIT=seconds to let it come up (default 40),
#   SIZE=WxH of the virtual screen (default 1600x900), APPLY="show edit ..." `BIN` commands to
#   run, one word each, before dumping; KEYS="f w j" keys to press first (GPUI binary: `f w`
#   enters the Wi-Fi widget, `j` moves its cursor); VERBOSE=1 to show the bus's and portals' noise.
set -u
BIN=$(realpath "$1")
PREFIX=$2
CONFIG=${3:-}
HERE=$(dirname "$(realpath "$0")")
H=$(mktemp -d /tmp/atspi-dump.XXXXXX)
mkdir -p "$H"/runtime "$H"/config/niri-dash "$H"/state "$H"/data "$H"/cache
chmod 700 "$H"/runtime
if [ -n "$CONFIG" ]; then
  cp "$CONFIG" "$H"/config/niri-dash/config.kdl
else
  printf 'layout {\n    %s col=0 row=0 width=8 height=7\n}\n' "${KIND:-wifi}" >"$H"/config/niri-dash/config.kdl
fi
export XDG_RUNTIME_DIR=$H/runtime XDG_CONFIG_HOME=$H/config XDG_STATE_HOME=$H/state
export XDG_DATA_HOME=$H/data XDG_CACHE_HOME=$H/cache HOME=$H
export ATSPI_DBUS_IMPLEMENTATION=dbus-daemon GSETTINGS_BACKEND=memory LC_ALL=C
unset WAYLAND_DISPLAY DISPLAY NIRI_SOCKET AT_SPI_BUS_ADDRESS
export DUMP_BIN=$BIN DUMP_PREFIX=$PREFIX DUMP_HOME=$H DUMP_WAIT=${WAIT:-40}
export DUMP_SIZE=${SIZE:-1600x900} DUMP_PY=$HERE/atspi_dump.py DUMP_KEY=$HERE/atspi_key.py DUMP_APPLY=${APPLY:-} DUMP_KEYS=${KEYS:-}

dbus-run-session -- bash -c '
  W=${DUMP_SIZE%x*}; Hh=${DUMP_SIZE#*x}
  kwin_wayland --virtual --no-lockscreen --no-global-shortcuts --socket dump-$$ --width "$W" --height "$Hh" \
    >"$DUMP_HOME/kwin.log" 2>&1 &
  KW=$!
  # However this ends (an error, a timeout, Ctrl-C), the compositor and the dashboard go with it.
  trap '"'"'kill $KW $DP 2>/dev/null'"'"' EXIT
  for i in $(seq 60); do [ -S "$XDG_RUNTIME_DIR/dump-$$" ] && break; sleep 0.2; done
  export WAYLAND_DISPLAY=dump-$$ NIRI_DASH_MOCK=1
  # A desktop turns accessibility on when a screen reader starts.
  gdbus call --session --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "<true>" >/dev/null
  export NIRI_DASH_TEST_KEYS=1
  RUST_LOG=${RUST_LOG:-warn} timeout 300 "$DUMP_BIN" daemon --show >"$DUMP_HOME/dash.log" 2>&1 &
  DP=$!
  sleep 4
  if [ -n "$DUMP_APPLY" ]; then
    for c in $DUMP_APPLY; do NIRI_DASH_TEST_KEYS=1 timeout 10 "$DUMP_BIN" $c >>"$DUMP_HOME/cmd.log" 2>&1; sleep 0.5; done
  fi
  # Keys, as the control socket takes them (the GPUI binary only: `f w` enters the Wi-Fi widget).
  for k in $DUMP_KEYS; do
    python3 "$DUMP_KEY" "$XDG_RUNTIME_DIR/niri-dash-$WAYLAND_DISPLAY.sock" "$k"
    sleep 0.4
  done
  python3 "$DUMP_PY" "$DUMP_PREFIX" "$DUMP_WAIT"
  timeout 5 "$DUMP_BIN" quit >/dev/null 2>&1
  kill $KW 2>/dev/null
  wait $DP 2>/dev/null
' 2>"$H"/session.log
status=$?
[ "${VERBOSE:-0}" = 1 ] && cat "$H"/session.log >&2
[ "$status" != 0 ] && { echo "--- dashboard log ---" >&2; tail -20 "$H"/dash.log >&2; }
rm -rf "$H"
exit $status
