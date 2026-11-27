#!/bin/sh
# Costruisce il pacchetto RPM di Lagotto (il pacchetto principale).
# Uso: scripts/build-rpm.sh   → dist/lagotto-*.rpm
# Serve: cargo, rpm-build.
set -eu
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cargo build --locked --release
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# Prima della prima uscita (0.0.0) il numero dice data e commit.
if [ "$version" = "0.0.0" ]; then
    release="0.$(date +%Y%m%d%H%M).git$(git rev-parse --short HEAD 2>/dev/null || echo local)"
else
    release=1
fi
stage=$(mktemp -d)
top=$(mktemp -d)
trap 'rm -rf "$stage" "$top"' EXIT
install -Dm755 target/release/lagotto "$stage/usr/bin/lagotto"
install -Dm755 target/release/lagotto-gui "$stage/usr/bin/lagotto-gui"
install -Dm755 target/release/lagotto-helper "$stage/usr/libexec/lagotto-helper"
install -Dm644 data/io.github.outlook791.Lagotto.desktop "$stage/usr/share/applications/io.github.outlook791.Lagotto.desktop"
install -Dm644 data/io.github.outlook791.Lagotto.svg "$stage/usr/share/icons/hicolor/scalable/apps/io.github.outlook791.Lagotto.svg"
install -Dm644 data/io.github.outlook791.Lagotto.metainfo.xml "$stage/usr/share/metainfo/io.github.outlook791.Lagotto.metainfo.xml"
install -Dm644 data/io.github.outlook791.Lagotto.policy "$stage/usr/share/polkit-1/actions/io.github.outlook791.Lagotto.policy"
install -Dm644 packaging/lagotto.repo "$stage/etc/yum.repos.d/lagotto.repo"
install -Dm644 LICENSE "$stage/LICENSE"
mkdir -p "$top/SOURCES" dist
tar -C "$stage" -cf "$top/SOURCES/lagotto-staging.tar" .
rpmbuild -bb --quiet \
    --define "_topdir $top" \
    --define "lagotto_version $version" \
    --define "lagotto_release $release" \
    packaging/lagotto.spec
find "$top/RPMS" -name '*.rpm' -exec cp {} dist/ \;
ls dist/*.rpm
