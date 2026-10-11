#!/bin/sh
# Install the tyler-tiler release binary on macOS.
#   curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
set -eu

repo="ndelah/Tyler"
version="${TYLER_VERSION:-latest}"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "tyler-tiler manages windows only on macOS." >&2
  exit 1
fi

case "$(uname -m)" in
  arm64) target="aarch64-apple-darwin" ;;
  x86_64) target="x86_64-apple-darwin" ;;
  *)
    echo "unsupported CPU: $(uname -m)" >&2
    exit 1
    ;;
esac

asset="tyler-tiler-${target}.tar.gz"
if [ "$version" = "latest" ]; then
  url="https://github.com/${repo}/releases/latest/download/${asset}"
else
  url="https://github.com/${repo}/releases/download/${version}/${asset}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ${url}"
if ! curl -fL --retry 3 --retry-delay 2 -o "${tmp}/${asset}" "$url"; then
  echo "Could not download a release binary." >&2
  echo "Releases are built by GitHub Actions when a v* tag is pushed:" >&2
  echo "  https://github.com/${repo}/releases" >&2
  exit 1
fi

tar -xzf "${tmp}/${asset}" -C "$tmp"
if [ ! -f "${tmp}/tyler-tiler" ]; then
  echo "archive did not contain tyler-tiler" >&2
  exit 1
fi

# curl marks the download quarantined. Drop that so Gatekeeper does not
# refuse an ad-hoc signed binary.
xattr -d com.apple.quarantine "${tmp}/tyler-tiler" 2>/dev/null || true

dest="/usr/local/bin"
if [ ! -d "$dest" ] || [ ! -w "$dest" ]; then
  dest="${HOME}/.local/bin"
  mkdir -p "$dest"
fi

cp "${tmp}/tyler-tiler" "${dest}/tyler-tiler"
chmod 755 "${dest}/tyler-tiler"

echo "Installed ${dest}/tyler-tiler"
case ":$PATH:" in
  *":${dest}:"*) ;;
  *)
    echo "Add it to your PATH:"
    echo "  echo 'export PATH=\"${dest}:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac

cat <<EOF

Next:
  1. Open a terminal and run:  tyler-tiler
  2. When macOS asks, allow Accessibility and Input Monitoring
     (System Settings → Privacy & Security → Accessibility, and Input Monitoring).
     Enable the tyler-tiler entry, then run it again.
  3. Stop it with Ctrl-C in that terminal. That puts hidden windows back on screen.

Leave SIP on. tyler-tiler does not need it disabled.
Config, if you want one: ~/.config/tyler/tyler.toml
  tyler-tiler write-config
EOF
