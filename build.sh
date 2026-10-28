#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
exec cargo xtask build --arch "${1:-x86_64}" "${@:2}"
