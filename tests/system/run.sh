#!/bin/sh
# Runs the system tests: niri-dash's real system (`live()`) against mocks of NetworkManager,
# BlueZ and UPower, in a container that has nothing else on it. They turn radios off and on, so
# they run here and nowhere else.
#
#     tests/system/run.sh                 all of them
#     tests/system/run.sh wifi            the ones with "wifi" in their name
#
# Needs podman or docker (CONTAINER_ENGINE picks one) and cargo.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
engine=${CONTAINER_ENGINE:-$(command -v podman || command -v docker || true)}
[ -n "$engine" ] || { echo "run.sh: no podman or docker" >&2; exit 1; }

cd "$root"
# The test binary is built here, on the host, and mounted into the container: building inside would
# compile the whole workspace again. The image has the same glibc as the development machine.
built=$(cargo test --locked -p niri-dash-system --test live --no-run 2>&1 | tee /dev/stderr)
binary=$(printf '%s\n' "$built" | sed -n 's/.*Executable tests\/live.rs (\(.*\))/\1/p' | tail -1)
[ -x "$root/$binary" ] || { echo "run.sh: couldn't find the test binary" >&2; exit 1; }

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
cp "$root/$binary" "$staging/live"
chmod 755 "$staging"

"$engine" build -q -t niri-dash-system-tests -f "$here/Containerfile" "$here" >/dev/null
exec "$engine" run --rm -e NIRI_DASH_SYSTEM_ONLY="${1:-}" -v "$staging:/test:ro,Z" \
    niri-dash-system-tests /test/live
