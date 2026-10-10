#!/usr/bin/env bash
# Refresh the embedded snapshot (data/dtc.tsv, data/modules.tsv) from a vehicle-db build.
#
#   scripts/sync_vehicle_db.sh ../vehicle-db      # a checkout where `tools/build.py` has been run
#
# Review the diff before committing: the snapshot is what ships inside the binary and
# what works offline. `canforge db update` can fetch newer data at runtime.
set -euo pipefail
src="${1:?usage: $0 <vehicle-db checkout>}/dist"
[ -f "$src/dtc.tsv" ] && [ -f "$src/manifest.json" ] || { echo "run tools/build.py in $1 first" >&2; exit 1; }
cp "$src/dtc.tsv" "$src/modules.tsv" "$(dirname "$0")/../data/"
head -1 "$src/dtc.tsv"
