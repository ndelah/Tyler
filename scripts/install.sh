#!/bin/sh
# Install Rust if needed, build tyler-tiler from a fixed checkout, and copy it to ~/.local/bin.
# The build does not use the caller's current directory.
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

src="${TYLER_SRC:-${HOME}/.local/share/tyler}"
repo="https://github.com/ndelah/Tyler.git"

# Earlier versions of this script checked out into ~/.local/share/tyler/src.
if [ ! -d "${src}/.git" ] && [ -d "${src}/src/.git" ]; then
  src="${src}/src"
fi

if [ -d "${src}/.git" ]; then
  git -C "$src" pull --ff-only origin main
else
  mkdir -p "$(dirname "$src")"
  git clone "$repo" "$src"
fi

cd "$src"
cargo build --release

built="${src}/target/release/tyler-tiler"
if [ ! -f "$built" ]; then
  echo "build did not produce ${built}" >&2
  exit 1
fi

dest="${HOME}/.local/bin"
mkdir -p "$dest"
cp "$built" "${dest}/tyler-tiler"
chmod 755 "${dest}/tyler-tiler"

echo "Installed ${dest}/tyler-tiler"
case ":${PATH}:" in
  *":${dest}:"*) ;;
  *)
    echo "Add it to your PATH, then open a new terminal:"
    echo "  echo 'export PATH=\"${dest}:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac

echo "Tutorial: https://github.com/ndelah/Tyler/blob/main/docs/tutorial.md"
echo "SUPER is Control+Option+Command. Existing config files are preserved; see the tutorial to migrate."
echo "For Accessibility, authorize your terminal app if tyler-tiler is not listed."
