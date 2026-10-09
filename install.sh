#!/bin/sh
# Install flows-contribute from the GitHub releases into ~/.local/bin, after a SHA-256 check.
set -eu

REPO="yungifez/flows-contribute"
VERSION="${FLOWS_CONTRIBUTE_VERSION:-latest}"
DEST="${FLOWS_CONTRIBUTE_BIN:-$HOME/.local/bin}"

die() { printf 'Problem: %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
  Linux) os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) die "This script supports macOS and Linux. On Windows, download the .exe from https://github.com/$REPO/releases" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) die "Your processor ($(uname -m)) is not supported." ;;
esac
asset="flows-contribute-$arch-$os"

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading $asset ..."
curl -fsSL "$base/$asset" -o "$tmp/$asset" || die "The download failed."
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || die "The checksum download failed."

expected=$(awk -v name="$asset" '$2 == name || $2 == "*"name { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || die "SHA256SUMS has no entry for $asset."
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || die "The checksum does not match. The file was not installed."

mkdir -p "$DEST"
mv "$tmp/$asset" "$DEST/flows-contribute"
chmod 755 "$DEST/flows-contribute"
echo "Installed $DEST/flows-contribute"
case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "Add $DEST to your PATH, or run it as $DEST/flows-contribute" ;;
esac
echo "Next, run: flows-contribute login"
