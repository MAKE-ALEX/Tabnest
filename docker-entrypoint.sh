#!/bin/sh
set -eu

DATA_DIR="${DATA_DIR:-/data}"
mkdir -p "$DATA_DIR/favicons"

# The container starts as root only to make a bind-mounted ./data writable,
# then immediately drops to the unprivileged tabnest user.
if [ "$(id -u)" = "0" ]; then
    chown -R 10001:10001 "$DATA_DIR" 2>/dev/null || true
    exec setpriv --reuid=10001 --regid=10001 --clear-groups /usr/local/bin/tabnest "$@"
fi

exec /usr/local/bin/tabnest "$@"
