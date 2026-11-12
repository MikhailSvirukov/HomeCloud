#!/bin/sh
# Storage-node process supervisor used as the container's PID 1.
#
# The full contract is documented in RUNTIME_CONTRACT.md. This script stays
# POSIX sh so the image does not need bash.

set -eu

DEFAULT_RESTART_DELAY_SECONDS=1
DEFAULT_STORAGE_NODE_BIN=/usr/local/bin/storage-node
EXIT_INVALID_DELAY=64

# Positional arguments replace the supervisor entirely, which keeps
# `docker run <image> <command> ...` usable for diagnostics.
if [ "$#" -gt 0 ]; then
    exec "$@"
fi

storage_node_bin="${STORAGE_NODE_BIN:-$DEFAULT_STORAGE_NODE_BIN}"
restart_delay="${STORAGE_NODE_RESTART_DELAY_SECONDS:-$DEFAULT_RESTART_DELAY_SECONDS}"

case "$restart_delay" in
    '' | *[!0-9]*)
        printf '%s: STORAGE_NODE_RESTART_DELAY_SECONDS must be a non-negative integer, got "%s"\n' \
            "$0" "$restart_delay" >&2
        exit "$EXIT_INVALID_DELAY"
        ;;
esac

if [ "${#restart_delay}" -gt 4 ] || [ "$restart_delay" -lt 1 ] || [ "$restart_delay" -gt 3600 ]; then
    printf '%s: STORAGE_NODE_RESTART_DELAY_SECONDS must be in 1..=3600, got "%s"\n' \
        "$0" "$restart_delay" >&2
    exit "$EXIT_INVALID_DELAY"
fi

child_pid=
sleep_pid=
shutdown=0
shutdown_signal=TERM
shutdown_status=0

# Record the first termination signal, then forward it to the current child.
# Later signals keep forwarding but do not change the exit status.
trigger_shutdown() {
    trap_status=$1
    trap_signal=$2
    if [ "$shutdown" -eq 0 ]; then
        shutdown=1
        shutdown_signal=$trap_signal
        shutdown_status=$trap_status
    fi
    if [ -n "$child_pid" ]; then
        kill "-$trap_signal" "$child_pid" 2>/dev/null || true
    fi
    if [ -n "$sleep_pid" ]; then
        kill "-$trap_signal" "$sleep_pid" 2>/dev/null || true
    fi
}

trap 'trigger_shutdown 143 TERM' TERM
trap 'trigger_shutdown 130 INT' INT

while :; do
    # A signal received outside a child wait (for example during the restart
    # delay) ends the supervisor without starting another child.
    if [ "$shutdown" -eq 1 ]; then
        exit "$shutdown_status"
    fi

    "$storage_node_bin" &
    child_pid=$!

    # Close the race where a signal arrived between the loop check and the
    # child pid assignment.
    if [ "$shutdown" -eq 1 ]; then
        kill "-$shutdown_signal" "$child_pid" 2>/dev/null || true
    fi

    status=0
    wait "$child_pid" || status=$?

    # A trapped signal can interrupt `wait` while the child is still alive, so
    # keep waiting until it has actually exited.
    while [ "$shutdown" -eq 1 ] && kill -0 "$child_pid" 2>/dev/null; do
        status=0
        wait "$child_pid" || status=$?
    done

    child_pid=

    if [ "$shutdown" -eq 1 ]; then
        exit "$shutdown_status"
    fi

    # A clean child exit ends the container; only failures are restarted.
    if [ "$status" -eq 0 ]; then
        exit 0
    fi

    printf '%s: storage node exited with status %s; restarting in %s second(s)\n' \
        "$0" "$status" "$restart_delay" >&2
    sleep "$restart_delay" &
    sleep_pid=$!
    status=0
    wait "$sleep_pid" || status=$?
    sleep_pid=
    if [ "$shutdown" -eq 1 ]; then
        exit "$shutdown_status"
    fi
    if [ "$status" -ne 0 ]; then
        printf "%s: restart delay failed with status %s\n" "$0" "$status" >&2
        exit 1
    fi
done
