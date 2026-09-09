#!/bin/zsh
set -eu
cd "${0:A:h}/.."
python3 tools/camera.py wifi-setup
printf '\nPress Return to close.\n'
read -r
