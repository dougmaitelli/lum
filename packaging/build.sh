#!/usr/bin/env bash
# Package the already-built native release binary, without installing anything.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -sm) == 'Linux x86_64' ]] || { echo 'Packaging targets x86-64 Linux.' >&2; exit 1; }
export VERSION
pkgid=$(cargo pkgid --locked -p lum)
VERSION=${pkgid##*#}
VERSION=${VERSION##*@}
readonly name="lum-${VERSION}-linux-x86_64"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name" "$stage/out"
install -m755 target/release/lum "$stage/$name/"
install -m644 packaging/{INSTALL.md,lum.env,lum.service} "$stage/$name/"
tar czf "$stage/out/$name.tar.gz" -C "$stage" "$name"
for format in deb rpm; do
    nfpm package --config packaging/nfpm.yaml --packager "$format" --target "$stage/out/"
done
(cd "$stage/out" && sha256sum *.deb *.rpm *.tar.gz > SHA256SUMS)
mkdir -p dist
cp "$stage/out/"* dist/
