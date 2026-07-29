#!/usr/bin/env bash
set -euo pipefail

if [ -z "${TARGET:-}" ] || [ -z "${EXECUTABLE:-}" ]; then
  echo "TARGET and EXECUTABLE are required" >&2
  exit 1
fi

version="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
archive="openapi-to-rust-v${version}-${TARGET}.tar.gz"
dist_dir="${DIST_DIR:-dist}"
package_dir="${PACKAGE_DIR:-target/package-release}"
mkdir -p "$dist_dir" "$package_dir"
cp -f "target/${TARGET}/release/${EXECUTABLE}" "$package_dir/${EXECUTABLE}"
tar -czf "$dist_dir/${archive}" -C "$package_dir" "${EXECUTABLE}"
node scripts/write-checksum.mjs "$dist_dir/${archive}"
