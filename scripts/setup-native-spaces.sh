#!/bin/sh
# Install/start the native Space backend. Existing yabai configuration is preserved.
set -eu
if [ "$(uname -s)" != Darwin ]; then
  echo "Native Mission Control desktops require macOS." >&2
  exit 1
fi
PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
export PATH
if ! command -v yabai >/dev/null 2>&1; then
  if ! command -v brew >/dev/null 2>&1; then
    echo "Install yabai first: https://github.com/asmvik/yabai/wiki/Installing-yabai-(latest-release)" >&2
    exit 1
  fi
  brew install asmvik/formulae/yabai
fi
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/yabai"
if [ ! -f "$config_dir/yabairc" ] && [ ! -f "$HOME/.yabairc" ]; then
  mkdir -p "$config_dir"
  # No-clobber protects a config created concurrently by the user.
  (set -C; cat > "$config_dir/yabairc" <<'CONFIG'
#!/bin/sh
# Tyler owns tiling; yabai only controls native Mission Control desktops.
PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
export PATH
yabai -m config layout float
CONFIG
  )
  chmod 755 "$config_dir/yabairc"
else
  echo "Keeping your existing yabai configuration. It must use layout float for Tyler."
fi
yabai --start-service
echo "Enable yabai in System Settings > Privacy & Security > Accessibility."
echo "Then run: tyler-tiler check-spaces"
echo "Create additional desktops in Mission Control before using Fn + number."
