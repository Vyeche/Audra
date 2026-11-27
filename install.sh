#!/bin/sh
# Installs Audra from this checkout, the newest code, in one step:
#
#   ./install.sh
#
# It builds the packages (packaging/build-deb.sh), then installs Audra,
# replacing the copy that's there even when the version number is the same.
# The driver package goes on only when no Babyface driver is installed yet;
# a driver that's already there (from the package or set up by hand) is kept.
# apt asks for your password.
set -eu

cd "$(dirname "$0")"
packaging/build-deb.sh

OUT="${CARGO_TARGET_DIR:-target}/debian"
case "$OUT" in /*) ;; *) OUT="./$OUT" ;; esac
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -n 1)
DRIVER_VERSION=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' driver/tools/kernel/dkms.conf)
ARCH=$(dpkg --print-architecture)

set -- "$OUT/audra_${VERSION}_${ARCH}.deb"
if dkms status -m snd-usb-babyface-pro 2>/dev/null | grep -q .; then
    echo "The Babyface driver is already installed: keeping it."
else
    set -- "$@" "$OUT/snd-usb-babyface-pro-dkms_${DRIVER_VERSION}_all.deb"
fi

echo
echo "Installing: $*"
sudo apt install --reinstall "$@"
echo
echo "Done. Close Audra if it's open and start it again from the app launcher."
