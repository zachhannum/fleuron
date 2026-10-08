# The fixture book's images

`plate.jpg` is Plate I of the 1726 first edition of *Travels into
Several Remote Nations of the World*, the map that faces page 1 of
Part I. Printed by Benjamin Motte, and public domain by age. The scan
is Wikimedia Commons' `Map of Lilliput - Gulliver's Travels 1726
edition.png`, converted to greyscale JPEG and resized to 250px, with
a JFIF density of 100dpi so that its intrinsic size is a plate-sized
180pt rather than its pixel count.

`fleuron.png` and `fleuron.webp` are the ❦ of EB Garamond, the face
the engine bundles, rendered at 128px on a transparent ground with a
`pHYs` of 300dpi. The two are the same pixels in the two raster
formats the PDF writer decodes.

`tailpiece.svg` is a swelled rule with a lozenge at its center, drawn
for this repository. Its root element gives it a size of 2in by
0.25in.

Between them they cover what the writer does with an image: a JPEG
embedded as it arrived, a raster format whose alpha channel becomes a
soft mask, and an SVG drawn as paths.

The ornament's alpha channel does a second job. `fixtures/styled.css`
wraps the text around the shape that channel covers, so the ornament
is what drives `shape-outside: auto` through the fixture run. The map
covers the other case. It has no alpha channel at all, so the text
goes around its box.
