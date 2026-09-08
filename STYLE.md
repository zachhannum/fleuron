# Prose style

The voice the docs, the README, and the package READMEs are written in.

The rules below come from edits made to finished pages. Each one is a
real before and after, so the guide is a record of what was wanted
rather than a theory of good writing.

## The two that outrank the rest

**Clarity and correctness first.** Every other rule here loses to
being understood, and to being right. A plain sentence that takes an
extra clause beats a short one the reader has to decode.

**Write the ordinary word.** A trade word is fine where it is the
clearest word, and it gets a definition the first time it appears
unless it is already common. A trade word used for flavor gets cut.

Before:

> The sheet is 6x9 inches, with mirrored margins and the folio bottom
> centre.

After:

> A 6x9 inch page, mirrored margins, and page numbers centered at the
> bottom.

`ink`, `paints`, `sets`, `the flow`, `trim`, `leaf`, `measure` and
`sink` are flowery, not technical. `recto` and `verso` stay, because
they are literal CSS values, and the page defines them.

## Sentences

### Say what a thing is for before saying what it is

Before:

> This page is the whole of the CSS fleuron reads.

After:

> Fleuron uses CSS to describe how a book is typeset. This page
> describes all of the CSS rules that fleuron supports and how to use
> them.

### Active voice

The engine does something. Something is not done by the engine.

Before:

> Any subset of CSS not supported by the engine is output as a
> warning.

After:

> The engine warns about anything it does not support.

### One fact per sentence

Two facts welded with `and` or `so` usually want to be two sentences.
Chained clauses are where the register goes wrong.

Before:

> What the margins leave is the content box, and the width of that box
> is the measure: the width every line of prose breaks to.

After:

> What is left inside the margins is the content box. Its width is the
> width that lines of text break to.

### Say what an example demonstrates

Every snippet gets a lead-in naming what it does. `The following
example ...` is the ordinary phrasing and needs no improving on.

Before:

> An author sheet overrides as much of that as it names:

After:

> The following example overrides the default page size, font size,
> text alignment, and hyphenation:

### Stop before the obvious

A consequence the reader works out unaided is a sentence to cut.

Before:

> The letter is sized to fit those lines rather than by `font-size`.
> Its top lines up with the top of the capitals on the first line, and
> its baseline sits on the last. The lines beside it are shortened to
> make room.

After:

> The letter is sized to fit those lines rather than by `font-size`.
> Its top lines up with the top of the capitals on the first line, and
> its baseline sits on the last.

Dropping a fact to keep a paragraph moving is allowed. An intro does
not have to be complete.

### No riddles, no epigrams

State the thing, then show it. A sentence whose job is to be pleasing
gets cut, however true it is.

> A name is a promise the rest of the code has to keep.

### No personification

A file, a tool, or a warning does not say, know, want, or announce
anything. A host, a caller, or a reader is a party to the contract
rather than a tool, and may still want things.

## Words

American spelling: `color`, `centered`, `synthesized`.

ASCII where a reader might type it: `6x9`, not `6×9`. A space in
`11 pt`.

Oxford commas.

No em dashes or en dashes. A period, a comma, a colon, or parentheses
does the job.

One name per concept, used everywhere: in prose, in code comments, in
strings, in headings, and in filenames. A rename is finished when
nothing in the repo still uses the old name.

Name packages and commands the way the reader types them, in
backticks.

A feature that is missing is not supported yet, not refused.

## What does not go in a page

Numbers measured somewhere else. Timings, memory figures, and counts
belong on the page that measures them, and go stale everywhere else.

Sample output that drifts. Either a test checks the number or it stays
out.

The future. No "when it publishes", no "this will change".

History. What a page used to say is in git.

Claims wider than the code. "Nothing panics" is a claim about every
input anyone will ever write.

Implementation detail, for a reader outside the repo. What a command
does and what comes back is theirs. How it is done is not. Test names,
CI jobs, and benchmark harnesses belong in CLAUDE.md. A section whose
heading says it is for someone building the repo is the exception.

## Shape of a page

Headings are labels a reader scans and a search box matches, not lines
of prose. Page titles are sentence case.

A quickstart opens with install, then the one command that produces
output, then what came back.

A section that explains a mechanism ends in a snippet that runs it,
with a lead-in naming what the snippet does. A snippet that is also a
file in the repo is checked against that file by a test, so the page
cannot drift from the code.

A reference table links out rather than repeating a paragraph inline.

Saying what a page covers is fine. So is pointing at another part of
it.

## Links

Docs pages link: to other pages, to source, to the projects the code
depends on. CLAUDE.md's rule against links covers code comments and
internal notes.
