#!/bin/zsh
set -eu
cd "${0:A:h}/.."
python3 tools/camera.py github-setup
printf '\nPress Return to close.\n'
read -r
