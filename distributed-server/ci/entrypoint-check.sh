#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
entrypoint="${script_dir}/../entrypoint.sh"

for value in 0 3601 99999999999999999999 -1 1.5 " 1"; do
    if STORAGE_NODE_RESTART_DELAY_SECONDS="${value}" \
        STORAGE_NODE_BIN=/bin/true \
        "${entrypoint}" >/dev/null 2>&1; then
        printf 'entrypoint accepted invalid restart delay %q\n' "${value}" >&2
        exit 1
    fi
done

STORAGE_NODE_RESTART_DELAY_SECONDS=1 STORAGE_NODE_BIN=/bin/true "${entrypoint}" >/dev/null
