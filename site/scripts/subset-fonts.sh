#!/usr/bin/env sh
# Regenerates the faces in src/assets/fonts. The book face is subset
# from the same bytes the engine ships; the display and chrome faces
# come from site/fonts. Latin plus the punctuation the site sets and
# ❦.
#
# The display face is pinned to the optical size and width the site
# sets it at, which is the difference between twenty-eight kilobytes
# and a hundred. The weight axis survives, so one file covers the
# display and the brand.
#
# Needs fonttools with brotli: pip install 'fonttools[woff]' brotli
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out="$root/site/src/assets/fonts"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chars='U+0020-007E,U+00A0,U+00A9,U+00B7,U+00D7,U+2013,U+2014,U+2018,U+2019,U+201C,U+201D,U+2026,U+2039,U+203A,U+2190,U+2192,U+2699,U+2766'

subset() {
  pyftsubset "$1" \
    --unicodes="$chars" \
    --layout-features='kern,liga,calt,onum,tnum' \
    --flavor=woff2 --with-zopfli \
    --output-file="$out/$2.woff2"
}

subset "$root/crates/fleuron/fonts/EBGaramond-VF.ttf" ebgaramond-subset
subset "$root/crates/fleuron/fonts/EBGaramond-Italic-VF.ttf" ebgaramond-italic-subset

fonttools varLib.instancer -q -o "$work/bricolage.ttf" \
  "$root/site/fonts/BricolageGrotesque-VF.ttf" opsz=96 wdth=100
subset "$work/bricolage.ttf" bricolage-subset

# Only the letter the landing headline opens on, which is the
# difference between three kilobytes and a hundred.
pyftsubset "$root/site/fonts/Fleuron-Mixed.otf" \
  --unicodes='U+0041' \
  --layout-features='' \
  --flavor=woff2 --with-zopfli \
  --output-file="$out/fleuron-mixed-subset.woff2"

subset "$root/site/fonts/SpaceMono-Regular.ttf" spacemono-subset
subset "$root/site/fonts/SpaceMono-Bold.ttf" spacemono-bold-subset
