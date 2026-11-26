#!/bin/sh
# Installa quello che serve a Lagotto su ogni distribuzione, per le verifiche.
#   ci/deps.sh fedora|ubuntu|debian|arch|opensuse
set -eu
distro=$1
case $distro in
fedora)
    dnf -y -q install rust cargo gcc pkgconf-pkg-config fontconfig-devel libxkbcommon-devel \
        git python3 sleuthkit testdisk dosfstools mtools e2fsprogs ImageMagick perl-Image-ExifTool \
        util-linux xorg-x11-server-Xvfb xdotool desktop-file-utils appstream polkit
    ;;
ubuntu|debian)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq build-essential pkg-config curl git ca-certificates python3 \
        libfontconfig1-dev libxkbcommon-dev sleuthkit testdisk dosfstools mtools e2fsprogs \
        imagemagick libimage-exiftool-perl util-linux xvfb xdotool desktop-file-utils appstream
    # Il Rust delle distribuzioni è troppo vecchio per Slint.
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    ;;
arch)
    pacman -Syu --noconfirm --needed base-devel rust fontconfig libxkbcommon git python \
        sleuthkit testdisk dosfstools mtools e2fsprogs imagemagick perl-image-exiftool \
        util-linux xorg-server-xvfb xdotool desktop-file-utils appstream
    ;;
opensuse)
    zypper -n -q ref
    zypper -n -q in --force-resolution gawk
    zypper -n -q in --force-resolution rust cargo gcc pkgconf fontconfig-devel libxkbcommon-devel git python3 \
        sleuthkit testdisk dosfstools mtools e2fsprogs ImageMagick exiftool util-linux xvfb-run xdotool \
        desktop-file-utils AppStream
    ;;
*)
    echo "unknown distribution: $distro" >&2
    exit 2
    ;;
esac
