# Prose style

The voice the docs, the README, and the package READMEs are written
in.

The voice is Simplified English, in the spirit of ASD-STE100: short
sentences, active voice, simple tenses, and one word for one meaning.
The rules below come from edits made to finished pages. Each one is a
real before and after, so the guide is a record of what was wanted
rather than a theory of good writing.

## The two that outrank the rest

### Clarity and correctness first

Every other rule here loses to being understood, and to being right. A
plain sentence that takes an extra clause beats a short one the reader
has to decode.

### Write the ordinary word

A trade word is fine where it is the clearest word. It gets a
definition of under ten words the first time it appears, unless it is
already common. A trade word used for flavor gets cut.

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

### Active voice, and name the actor

The engine does something. Something is not done by the engine. Where
a sentence has no actor, the actor is the engine, the host, or you.

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

### Twenty-five words, and twenty in a procedure

A sentence that explains something runs to 25 words at most. A
sentence that tells the reader what to do runs to 20, and carries one
instruction. Count the words in the three longest sentences of a
draft, and split what is over.

Before:

> An inset left at `auto` lets the opposite one place the image, and
> where both are `auto` the image sits at the left or top edge of the
> content box.

After:

> An inset of `auto` lets the opposite inset place the image. Where
> both insets of an axis are `auto`, the image sits at the left or the
> top edge of the content box.

### Simple tenses

Write the simple present, the simple past, and the simple future. No
present perfect: "has completed" is "completed". An "-ing" verb after
a comma is a second sentence.

Before:

> An image can be placed against the page rather than among the
> paragraphs, with the text wrapping around it.

After:

> You can put an image against the page rather than among the
> paragraphs. The text then wraps around it.

### can, will, must

These three modals carry every meaning a page here needs. A "should"
that states a requirement is "must". A "should" that recommends is cut,
or stated as the fact behind it. "may", "might" and "could" are "can".

Before:

> the stretches prose may be set in

After:

> the stretches the prose can be set in

### Condition before command

The condition comes first, and a comma divides it from the command: "If
the build fails, read the log." A limit belongs with the step it
limits rather than in a note after it.

### Say what an example demonstrates

Every snippet gets a lead-in naming what it does. `The following
example ...` is the ordinary phrasing and needs no improving on.

Before:

> An author sheet overrides as much of that as it names:

After:

> The following example overrides the default page size, font size,
> text alignment, and hyphenation:

### Break up a paragraph of code spans

A run of inline code inside prose is a wall. A flat vocabulary goes in
a block set apart from the text. A list of things with a second column
of information goes in a table.

Before:

> A compound selector is `<element>` (`p`), `*` (`section > *`),
> `.<class>` (`p.epigraph`) or `#<id>` (`#frontispiece`), optionally
> followed by any of these pseudo-classes: `:first-child`,
> `:last-child`, `:only-child`, `:nth-child()`, and eleven more.

After:

> A compound selector is one of these, optionally followed by any of
> the pseudo-classes below.
>
> | compound | example |
> |---|---|
> | `<element>` | `p` |
> | `.<class>` | `p.epigraph` |
>
> ```
> :first-child :last-child :only-child :nth-child()
> ```

Four or so code spans across a few sentences is ordinary prose. A
sentence that is mostly code spans is the problem.

### Inline code does not wrap

A code span carries a keyline, so a span broken over two lines is two
boxes with open ends. Spans do not break, which means a table column
cannot hold anything wide. Value syntax goes in a block under the
table rather than in a column of it.

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

### State the fact, not its importance

Cut the words that carry no fact: simply, seamlessly, robust,
powerful, comprehensive, leverage, crucial, "in order to", and "it is
worth noting". A page does not tell the reader that something matters.
It says what the thing does.

### No personification

A file, a tool, or a warning does not say, know, want, or announce
anything. A host, a caller, or a reader is a party to the contract
rather than a tool, and can still want things.

## Words

American spelling: `color`, `centered`, `synthesized`.

ASCII where a reader might type it: `6x9`, not `6×9`. A space in
`11 pt`.

Oxford commas.

No em dashes, no en dashes, and no semicolons. A period, a comma, a
colon, or parentheses does the job.

No contractions. Keep the articles, and keep "that".

One name per concept, used everywhere: in prose, in code comments, in
strings, in headings, and in filenames. A rename is finished when
nothing in the repo still uses the old name. `make sure that` covers
check, verify, confirm, validate, and ensure. `configuration` covers
config, settings, and options.

A noun chain runs to three words. Break a longer one with a
preposition: "the timeout value for the connection pool".

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
of prose. A heading runs to two sentences at most. Page titles are
sentence case.

Bold is neither emphasis nor a lead-in. A heading does that work.

A vertical list holds three or more parallel items or steps. The
lead-in ends in a colon, each item starts uppercase, and an item
carries one instruction. Fewer than three items is a sentence.

A warning names the command or the condition first, then the risk. "Do
not run this against production. The command deletes rows."

A quickstart opens with install, then the one command that produces
output, then what came back.

A section that explains a mechanism ends in a snippet that runs it,
with a lead-in naming what the snippet does. Where a snippet is also a
file in the repo, a test compares the two. The page cannot drift from
the code.

A reference table links out rather than repeating a paragraph inline.

Saying what a page covers is fine. So is pointing at another part of
it.

## Links

Docs pages link: to other pages, to source, to the projects the code
depends on. CLAUDE.md's rule against links covers code comments and
internal notes.

## Before you publish

Count the words in the three longest sentences, and split what is over
the limit. Then read the draft for the habits this guide removes: a
contraction, "has been", "should", "may", a semicolon, an em dash, an
"-ing" verb after a comma, bold as emphasis, and check, verify or
config where one word covers all three.
