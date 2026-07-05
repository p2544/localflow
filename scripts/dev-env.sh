#!/usr/bin/env bash
# Source this to get a working Linux dev environment for LocalFlow without
# root: userspace micromamba env supplies cmake, pkg-config, alsa-lib, and
# clang builtin headers (for llama-cpp bindgen).
#
#   source scripts/dev-env.sh          # sets PATH/PKG_CONFIG_PATH/LIBCLANG_PATH
#   cargo test -p localflow-core --no-default-features   # fast tier
#   cargo build -p localflow-core --release              # full engines
#
# The Tauri GUI shell additionally needs system webkit2gtk (root):
#   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
#        libayatana-appindicator3-dev libxdo-dev

LF_MAMBA_PREFIX="${LF_MAMBA_PREFIX:-$HOME/.localflow-dev/mamba}"

if [ ! -d "$LF_MAMBA_PREFIX/envs/lf" ]; then
  echo "Creating dev env at $LF_MAMBA_PREFIX (one-time)…"
  mkdir -p "$LF_MAMBA_PREFIX/bin"
  if [ ! -x "$LF_MAMBA_PREFIX/bin/micromamba" ]; then
    curl -Ls https://micro.mamba.pm/api/micromamba/linux-64/latest \
      | tar -xj -C "$LF_MAMBA_PREFIX" bin/micromamba
  fi
  MAMBA_ROOT_PREFIX="$LF_MAMBA_PREFIX" "$LF_MAMBA_PREFIX/bin/micromamba" \
    create -y -n lf -c conda-forge cmake pkg-config alsa-lib clangdev
fi

LF_ENV="$LF_MAMBA_PREFIX/envs/lf"
export PATH="$LF_ENV/bin:$PATH"
export PKG_CONFIG_PATH="$LF_ENV/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LIBCLANG_PATH="$LF_ENV/lib"
echo "LocalFlow dev env ready (cmake $(cmake --version | head -c22 | tail -c6), $LF_ENV)"
