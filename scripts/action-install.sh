#!/usr/bin/env bash
set -euo pipefail

if [ -n "${INPUT_BINARY:-}" ]; then
  if [ ! -x "$INPUT_BINARY" ]; then
    echo "::error::binary is not executable: $INPUT_BINARY"
    exit 1
  fi
  echo "binary=$INPUT_BINARY" >>"$GITHUB_OUTPUT"
  exit 0
fi

version="${INPUT_VERSION:-}"
if [ -z "$version" ]; then
  case "${ACTION_REF:-}" in
    v[0-9]*) version="${ACTION_REF#v}" ;;
    *)
      echo "::error::version is required when the Action is not referenced by a version tag"
      exit 1
      ;;
  esac
fi
version="${version#v}"

runner_arch="$(uname -m)"
case "${RUNNER_OS:-}-${runner_arch}" in
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  macOS-x86_64) target="x86_64-apple-darwin" ;;
  macOS-arm64) target="aarch64-apple-darwin" ;;
  Windows-x86_64) target="x86_64-pc-windows-msvc" ;;
  *)
    echo "::error::unsupported runner: ${RUNNER_OS:-unknown} ${runner_arch}"
    exit 1
    ;;
esac

archive="openapi-to-rust-v${version}-${target}.tar.gz"
repository="${ACTION_REPOSITORY:-gpu-cli/openapi-to-rust}"
base_url="https://github.com/${repository}/releases/download/v${version}"
install_root="${RUNNER_TEMP}/openapi-to-rust-${version}"
mkdir -p "$install_root/bin"
curl --fail --location --silent --show-error "$base_url/$archive" --output "$install_root/$archive"
curl --fail --location --silent --show-error "$base_url/$archive.sha256" --output "$install_root/$archive.sha256"
node "$GITHUB_ACTION_PATH/scripts/verify-download.mjs" "$install_root/$archive" "$install_root/$archive.sha256"
tar -xzf "$install_root/$archive" -C "$install_root/bin"

binary="$install_root/bin/openapi-to-rust"
if [ "${RUNNER_OS:-}" = "Windows" ]; then
  binary="$binary.exe"
fi
chmod +x "$binary"
echo "$install_root/bin" >>"$GITHUB_PATH"
echo "binary=$binary" >>"$GITHUB_OUTPUT"
