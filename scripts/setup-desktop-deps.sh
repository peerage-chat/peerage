#!/usr/bin/env bash
# Зависимости для десктоп-приложения (Slint, нативный GUI под WSL/Linux).
# Запуск из WSL: bash scripts/setup-desktop-deps.sh
set -euo pipefail
sudo apt-get update -qq
sudo apt-get install -y \
  build-essential pkg-config nasm \
  libxkbcommon0 libxkbcommon-x11-0 \
  libwayland-client0 libwayland-cursor0 libwayland-egl1 \
  libgl1 libegl1 libgles2 \
  libfontconfig1 libfreetype6 \
  libopus-dev libasound2-dev \
  libpipewire-0.3-dev libgbm-dev libdrm-dev libdbus-1-dev
echo "desktop deps OK (Slint + Opus/ALSA + H.264 nasm + захват экрана)"
