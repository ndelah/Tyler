#!/bin/sh
# Install Rust if needed, build tyler-tiler, and copy it to ~/.local/bin.
#   curl -fsSL https://raw.githubusercontent.com/ndelah/Tyler/main/scripts/install.sh | sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
  echo "tyler-tiler manages windows only on macOS." >&2
  exit 1
fi

if ! command -v git >/dev/null 2>&1; then
  echo "git is required. Install the Xcode command line tools, then run this again:" >&2
  echo "  xcode-select --install" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Installing Rust (rustup)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi

if [ -f "${HOME}/.cargo/env" ]; then
  # rustup's env file is shell syntax. It is not on PATH until this shell sources it.
  . "${HOME}/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is still not available after installing rustup." >&2
  exit 1
fi

src="${TYLER_SRC:-${HOME}/.local/share/tyler/src}"
repo="https://github.com/ndelah/Tyler.git"

if [ -d "${src}/.git" ]; then
  git -C "$src" pull --ff-only origin main
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
    echo "Add it to your PATH, then open a new terminal:"
    echo "  echo 'export PATH=\"${dest}:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac
