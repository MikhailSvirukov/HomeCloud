#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
entrypoint="${script_dir}/../entrypoint.sh"

check_invalid() {
    local value="$1"
    local output
    local status
    set +e
    output="$(STORAGE_NODE_RESTART_DELAY_SECONDS="${value}" \
        STORAGE_NODE_BIN=/bin/true \
        "${entrypoint}" 2>&1)"
    status=$?
    set -e
    if [ "${status}" -ne 64 ]; then
        printf 'entrypoint rejected %q with status %s, expected 64: %s\n' \
            "${value}" "${status}" "${output}" >&2
        exit 1
    fi
}

for value in 0 3601 99999999999999999999 -1 1.5 " 1"; do
    check_invalid "${value}"
done

STORAGE_NODE_RESTART_DELAY_SECONDS=1 STORAGE_NODE_BIN=/bin/true "${entrypoint}" >/dev/null
