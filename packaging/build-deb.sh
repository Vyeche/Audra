#!/bin/sh
# Builds Audra's Debian packages for Pop!_OS and Ubuntu into target/debian/
# (or $CARGO_TARGET_DIR/debian/), with nothing but dpkg-deb:
#
#   audra_<version>_<arch>.deb               the mixer app
#   snd-usb-babyface-pro-dkms_<version>_all.deb
#                                            the kernel driver, as source that
#                                            DKMS builds on install and again
#                                            for every new kernel
#
# Install both with:  sudo apt install ./target/debian/*.deb
#
# Usage: packaging/build-deb.sh [--no-build]   (--no-build: package the
# release binary already built)
set -eu

cd "$(dirname "$0")/.."
TARGET=${CARGO_TARGET_DIR:-target}
OUT="$TARGET/debian"
STAGE="$OUT/stage"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -n 1)
DRIVER_VERSION=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' driver/tools/kernel/dkms.conf)
ARCH=$(dpkg --print-architecture)
# Who built it: DEB_MAINTAINER, else git's name and e-mail, else a plain
# "local build" (a fresh machine has no git identity, and the script quit
# silently there: the clean-build check, 2026-11-23).
MAINTAINER=${DEB_MAINTAINER:-"$(git config user.name 2>/dev/null || echo "Local build") <$(git config user.email 2>/dev/null || echo "nobody@localhost")>"}

if [ "${1:-}" != "--no-build" ]; then
    # The builder's home and checkout paths stay out of the binary (panic
    # messages embed source paths; a public package showed the builder's
    # user name in /home/<name>/.cargo/...).
    RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/build --remap-path-prefix=$(pwd)=/audra" \
        cargo build --release -p audra
fi
BIN="$TARGET/release/audra"
[ -x "$BIN" ] || { echo "No release binary at $BIN" >&2; exit 1; }

rm -rf "$STAGE"
mkdir -p "$OUT"

# Writes DEBIAN/control for package root $1 from the template $2, filling
# in the version, architecture, maintainer and installed size.
control() {
    root=$1
    template=$2
    version=$3
    arch=$4
    size=$(du -sk --exclude=DEBIAN "$root" | cut -f1)
    mkdir -p "$root/DEBIAN"
    sed -e "s|@VERSION@|$version|" -e "s|@ARCH@|$arch|" \
        -e "s|@MAINTAINER@|$MAINTAINER|" -e "s|@SIZE@|$size|" \
        "$template" > "$root/DEBIAN/control"
}

# ---- The app -----------------------------------------------------------
APP="$STAGE/audra"
install -Dm755 "$BIN" "$APP/usr/bin/audra"
strip --strip-unneeded "$APP/usr/bin/audra"
install -Dm644 crates/app/assets/audra.desktop "$APP/usr/share/applications/audra.desktop"
install -Dm644 crates/app/assets/audra.svg "$APP/usr/share/icons/hicolor/scalable/apps/audra.svg"
install -Dm644 crates/app/assets/audra-128.png "$APP/usr/share/icons/hicolor/128x128/apps/audra.png"
install -Dm644 crates/app/assets/audra-256.png "$APP/usr/share/icons/hicolor/256x256/apps/audra.png"
install -Dm644 packaging/audra.copyright "$APP/usr/share/doc/audra/copyright"
control "$APP" packaging/audra.control "$VERSION" "$ARCH"
dpkg-deb --root-owner-group --build "$APP" "$OUT/audra_${VERSION}_${ARCH}.deb"

# ---- The driver (DKMS) -------------------------------------------------
DRV="$STAGE/snd-usb-babyface-pro-dkms"
SRC="$DRV/usr/src/snd-usb-babyface-pro-$DRIVER_VERSION"
for f in babyfacepro.c babyfacepro-ctl.c babyfacepro.h Makefile dkms.conf; do
    install -Dm644 "driver/tools/kernel/$f" "$SRC/$f"
done
install -Dm644 packaging/driver/copyright "$DRV/usr/share/doc/snd-usb-babyface-pro-dkms/copyright"
control "$DRV" packaging/driver/control "$DRIVER_VERSION" all
for script in postinst prerm; do
    sed "s|@VERSION@|$DRIVER_VERSION|" "packaging/driver/$script" > "$DRV/DEBIAN/$script"
    chmod 755 "$DRV/DEBIAN/$script"
done
dpkg-deb --root-owner-group --build "$DRV" "$OUT/snd-usb-babyface-pro-dkms_${DRIVER_VERSION}_all.deb"

rm -rf "$STAGE"
echo
echo "Built:"
ls -1 "$OUT"/*.deb
echo
# apt needs a path with a slash to take a file, not a package name.
case "$OUT" in /*) DIR="$OUT" ;; *) DIR="./$OUT" ;; esac
echo "Install with:  sudo apt install $DIR/audra_${VERSION}_${ARCH}.deb $DIR/snd-usb-babyface-pro-dkms_${DRIVER_VERSION}_all.deb"
