#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
version=2.10.0
checksum=c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
destination="target/sparkle/$version"
archive="target/sparkle/Sparkle-$version.tar.xz"
if [[ -f "$destination/.verified-$checksum" && -f "$destination/Sparkle.framework/Headers/Sparkle.h" ]]; then exit 0; fi
mkdir -p "$destination"
if [[ ! -f "$archive" ]]; then
    curl --fail --location --silent --show-error --retry 2 \
        "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz" \
        -o "$archive.download"
    mv "$archive.download" "$archive"
fi
printf '%s  %s\n' "$checksum" "$archive" | shasum -a 256 -c -
tar -xf "$archive" -C "$destination"
touch "$destination/.verified-$checksum"
