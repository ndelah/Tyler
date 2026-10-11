#!/bin/sh
# Clone tyler-tiler, build a release binary, and copy it onto PATH.
# Rust 1.85 or newer must already be installed (https://rustup.rs).
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "tyler-tiler manages windows only on macOS." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1 || ! command -v git >/dev/null 2>&1; then
  echo "git and cargo are required. Install Rust 1.85 or newer first:" >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  exit 1
fi

src="${TYLER_SRC:-${HOME}/.local/share/tyler/src}"
repo="https://github.com/ndelah/Tyler.git"

if [ -d "${src}/.git" ]; then
  git -C "$src" pull --ff-only
else
  mkdir -p "$(dirname "$src")"
  git clone "$repo" "$src"
fi

cargo build --release --manifest-path "${src}/Cargo.toml"

dest="${HOME}/.local/bin"
mkdir -p "$dest"
cp "${src}/target/release/tyler-tiler" "${dest}/tyler-tiler"
chmod 755 "${dest}/tyler-tiler"

echo "Installed ${dest}/tyler-tiler"
case ":${PATH}:" in
  *":${dest}:"*) ;;
  *)
    echo "Add it to your PATH:"
    echo "  echo 'export PATH=\"${dest}:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac
