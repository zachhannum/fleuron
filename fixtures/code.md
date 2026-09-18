---
title: The Careful Build
author: E. Marsh
language: en
---

## Reading the manifest

A manifest names the parts of a build and nothing else. It leaves the
order of the parts to the build, and the choice of machine to the
caller. The build reads the manifest once, at the start, and holds it
for the rest of the run. A field the build does not know is a warning
rather than an error, so a manifest written for a later version still
builds.

The name of a part is the key. The `source` field is a path, relative
to the manifest, and `needs` is a list of the parts that come first. A
part with no `needs` is a leaf, and the build starts at the leaves.

```toml
[part.reader]
source = "src/reader"
needs = []

[part.writer]
source = "src/writer"
needs = ["reader"]

[part.driver]
source = "src/driver"
needs = ["reader", "writer"]
```

The build resolves the manifest into an order and prints it. The
following order is what the manifest above resolves to, one part per
line, with the depth of each part shown by its indentation:

    reader
      writer
        driver

A cycle between two parts is an error, and the message names both of
them. The build stops before it makes anything, so a cycle costs one
read of the manifest and nothing else.

## The build script

The script below is the whole of the build. It reads the manifest,
walks the order, and runs one command per part. It keeps the output of
each part in a directory of its own, so a part that fails leaves the
parts before it in place. Run it with the path to the manifest as its
only argument.

```sh
#!/bin/sh
set -eu

manifest=${1:-build.toml}
out=${OUT:-./out}
log=$out/build.log

if [ ! -f "$manifest" ]; then
    echo "no manifest at $manifest" >&2
    exit 2
fi

mkdir -p "$out"
: > "$log"

resolve() {
    # One part per line, deepest last. The reader prints the order
    # and this reads it back.
    manifest-reader --order "$manifest"
}

make_part() {
    name=$1
    source=$2
    dir=$out/$name

    if [ -d "$dir" ] && [ -f "$dir/.done" ]; then
        echo "$name: up to date" | tee -a "$log"
        return 0
    fi

    rm -rf "$dir"
    mkdir -p "$dir"

    echo "$name: building" | tee -a "$log"
    if compile --source "$source" --out "$dir" >>"$log" 2>&1; then
        touch "$dir/.done"
        echo "$name: ok" | tee -a "$log"
    else
        echo "$name: failed, see $log" >&2
        return 1
    fi
}

failed=0
resolve | while read -r name source; do
    make_part "$name" "$source" || failed=1
done

if [ "$failed" -ne 0 ]; then
    echo "the build stopped at a part that failed" >&2
    exit 1
fi

echo "all parts built into $out" | tee -a "$log"
```

The script writes one line per part, and the same line goes to the log.
A part that is already made prints `up to date` and is left alone, so a
second run of the script over the same manifest makes nothing.
