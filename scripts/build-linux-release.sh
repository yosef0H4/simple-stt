#!/usr/bin/env bash
set -euo pipefail
# Desktop releases must be installed as native packages, never run from target/.
exec "$(dirname "$0")/package-linux-release.sh" "$@"
