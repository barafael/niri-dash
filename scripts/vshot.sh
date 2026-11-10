#!/bin/bash
# Run a niri-dash binary (the GPUI one or the Slint one) on a private virtual KWin, on the mock
# system, and take screenshots. Never touches the desktop session: its own dbus session, its own
# compositor, its own XDG directories.
#
# usage: scripts/vshot.sh BIN OUTDIR STEP...
#
#   BIN     target/debug/niri-dash (GPUI) or slint/target/debug/niri-dash-slint (Slint)
#   OUTDIR  where the pictures (NAME.png) and the logs go; made if missing
#   STEP    done in order:
#             shot=NAME      a screenshot of the whole virtual screen, OUTDIR/NAME.png
#             cmd=ARGS       run `BIN ARGS` (show, hide, toggle, edit, say --to x "hi", ...)
#             app=WIDGET     start `BIN app WIDGET` in the background (a window of its own)
#             stress=ARGS    start `BIN stress ARGS` in the background (an on-screen stress run;
#                            use with NODAEMON=1), its output in OUTDIR/stress.log
#             sleep=N        wait N seconds
#   With no steps: wait 5 seconds, take OUTDIR/shot.png.
#
# environment:
#   CONFIG=file.kdl   the config.kdl to start from (default: the program writes its default)
#   SIZE=1600x900     the virtual screen
#   MOCK=1            the mock system (made-up data); MOCK=0 reads the real one (don't)
#   KEEP=1            leave the temporary home behind (its path is printed)
#   NODAEMON=1        don't start the daemon (for `stress=`, which is a dashboard of its own)
#
# The daemon is started with `daemon --show`; its log is OUTDIR/dash.log, KWin's OUTDIR/kwin.log.
# The first screenshot is taken only after the dashboard has had 5 seconds to come up.
set -u
BIN=$(realpath "$1")
OUT=$(realpath -m "$2")
shift 2
STEPS=("$@")
[ ${#STEPS[@]} -eq 0 ] && STEPS=(sleep=5 shot=shot)
mkdir -p "$OUT"
H=$(mktemp -d /tmp/vshot.XXXXXX)
mkdir -p "$H"/runtime "$H"/config/niri-dash "$H"/state "$H"/data "$H"/cache
chmod 700 "$H"/runtime
[ -n "${CONFIG:-}" ] && cp "$CONFIG" "$H"/config/niri-dash/config.kdl
SIZE=${SIZE:-1600x900}
S=vshot-$$
export XDG_RUNTIME_DIR=$H/runtime XDG_CONFIG_HOME=$H/config XDG_STATE_HOME=$H/state
export XDG_DATA_HOME=$H/data XDG_CACHE_HOME=$H/cache HOME=$H
unset WAYLAND_DISPLAY DISPLAY NIRI_SOCKET
export VSHOT_BIN=$BIN VSHOT_OUT=$OUT VSHOT_SOCKET=$S VSHOT_SIZE=$SIZE VSHOT_HOME=$H
export VSHOT_MOCK=${MOCK:-1} VSHOT_NODAEMON=${NODAEMON:-0}
export VSHOT_STEPS=$(printf '%s\n' "${STEPS[@]}")

dbus-run-session -- bash -c '
  W=${VSHOT_SIZE%x*}; Hh=${VSHOT_SIZE#*x}
  kwin_wayland --virtual --no-lockscreen --socket "$VSHOT_SOCKET" --width "$W" --height "$Hh" \
    >"$VSHOT_OUT/kwin.log" 2>&1 &
  KW=$!
  # However this ends (an error, a timeout, Ctrl-C), the compositor and what ran on it go with it.
  trap '"'"'kill $KW $(jobs -p) 2>/dev/null'"'"' EXIT
  for i in $(seq 60); do [ -S "$XDG_RUNTIME_DIR/$VSHOT_SOCKET" ] && break; sleep 0.2; done
  export WAYLAND_DISPLAY=$VSHOT_SOCKET
  [ "$VSHOT_MOCK" = 1 ] && export NIRI_DASH_MOCK=1
  DP=
  if [ "$VSHOT_NODAEMON" != 1 ]; then
    RUST_LOG=${RUST_LOG:-info} timeout 600 "$VSHOT_BIN" daemon --show >"$VSHOT_OUT/dash.log" 2>&1 &
    DP=$!
  else
    : >"$VSHOT_OUT/dash.log"
  fi
  while IFS= read -r step; do
    case "$step" in
      shot=*)  timeout 30 spectacle -b -f -n -o "$VSHOT_OUT/${step#shot=}.png" >>"$VSHOT_OUT/spectacle.log" 2>&1 ;;
      cmd=*)   eval "timeout 10 \"\$VSHOT_BIN\" ${step#cmd=}" >>"$VSHOT_OUT/cmd.log" 2>&1 ;;
      app=*)   timeout 120 "$VSHOT_BIN" app "${step#app=}" >>"$VSHOT_OUT/app.log" 2>&1 & ;;
      stress=*) eval "timeout 300 \"\$VSHOT_BIN\" stress ${step#stress=}" >>"$VSHOT_OUT/stress.log" 2>&1 & ;;
      sleep=*) sleep "${step#sleep=}" ;;
      *)       echo "vshot: unknown step $step" >&2 ;;
    esac
  done <<<"$VSHOT_STEPS"
  timeout 5 "$VSHOT_BIN" quit >/dev/null 2>&1
  sleep 1
  kill $KW 2>/dev/null
  # What was started in the background (apps, a stress run) goes with the compositor.
  kill $(jobs -p) 2>/dev/null
  [ -n "$DP" ] && wait $DP 2>/dev/null
' >"$OUT/session.log" 2>&1
grep -iE "panic|error" "$OUT/dash.log" | grep -v zbus | head -5
ls "$OUT"/*.png 2>/dev/null
[ "${KEEP:-0}" = 1 ] && echo "home: $H" || rm -rf "$H"
