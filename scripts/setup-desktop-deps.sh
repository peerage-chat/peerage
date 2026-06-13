#!/usr/bin/env bash
# Зависимости для десктоп-приложения (Slint, нативный GUI под WSL/Linux).
# Запуск из WSL: bash scripts/setup-desktop-deps.sh
set -euo pipefail
sudo apt-get update -qq
sudo apt-get install -y \
  build-essential pkg-config \
  libxkbcommon0 libxkbcommon-x11-0 \
  libwayland-client0 libwayland-cursor0 libwayland-egl1 \
  libgl1 libegl1 libgles2 \
  libfontconfig1 libfreetype6
echo "desktop (Slint) deps OK"
