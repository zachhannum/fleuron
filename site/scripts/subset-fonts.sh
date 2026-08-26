#!/usr/bin/env sh
# Regenerates the display faces in src/assets/fonts from the same bytes
# the engine ships. Latin plus the punctuation the site sets and ❦; the
# weight axis survives, so one file covers every display weight.
#
# Needs fonttools with brotli: pip install 'fonttools[woff]'
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out="$root/site/src/assets/fonts"
chars='U+0020-007E,U+00A0,U+2013,U+2014,U+2018,U+2019,U+201C,U+201D,U+2026,U+2766'

subset() {
  pyftsubset "$root/crates/fleuron/fonts/$1.ttf" \
    --unicodes="$chars" \
    --layout-features='kern,liga,calt,onum,tnum' \
    --flavor=woff2 --with-zopfli \
    --output-file="$out/$2.woff2"
}

subset EBGaramond-VF ebgaramond-subset
subset EBGaramond-Italic-VF ebgaramond-italic-subset
