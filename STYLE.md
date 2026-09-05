# Prose style

The voice the docs, the README and the package READMEs are written in.
CLAUDE.md's documentation rules still apply. This says what the voice is
once they are satisfied.

## Voice

### Subject first, then verb

No inverted openers, no clause that delays the subject.

Before:

> Reach for a cache, and the second run costs nothing.

After:

> The second run reads from the cache.

### No epigrams

A sentence whose job is to be pleasing gets cut, however true it is.

> A name is a promise the rest of the code has to keep.

### No personification

A file, a tool or a refusal does not say, know, want or announce
anything.

Before:

> The parser says where a section begins. The warning says which face
> was wanted.

After:

> The parser sets where a section begins. The warning names the face
> that was asked for.

A host, a caller or a reader is a party to the contract rather than a
tool, and may still want things.

### No riddles

State the thing, then show it. Do not describe a shape and leave the
reader to find it.

Before:

> Four steps, in one direction.

After:

> The program below does four things: reads a source, compiles styling
> against it, lays it out, and writes the file.

### Stop at the fact

A sentence that explains, justifies or admires the fact before it ends
at the fact instead.

Before:

> The two calls are separate because they have different lifetimes. One
> parses once and serves many documents.

After:

> The two calls are separate because they have different lifetimes.

### Plain beats clever, even when clever is shorter

"can be used to" is fine. Compression is not the goal.

## Words

Use the ordinary word. A trade word belongs where it is the API's own
term, not in explanation.

One name per concept, used everywhere: in prose, in code comments, in
strings, in headings, and in filenames. A rename is finished when
nothing in the repo still uses the old name.

Name packages and commands the way the reader types them, in backticks.

A feature that is missing is not supported yet, not refused. Nothing
frames a gap as a decision against it.

## What does not go in a page

Numbers measured somewhere else. Timings, memory figures and counts
belong on the page that measures them, and go stale everywhere else.

Sample output that drifts. Either a test checks the number or it stays
out.

Anything another page already says. One page owns a fact and the rest
link to it.

The future. No "when it publishes", no "this will change".

History. What a page used to say is in git.

Claims wider than the code. "Nothing panics" is a claim about every
input anyone will ever write.

The reader's possessions. Describe what the code does, not what the
reader owns: "hands the url to your loader" is "loads the url".

Implementation detail, for a reader outside the repo. What a command
does and what comes back is theirs. How it is done is not. Test names,
CI jobs and benchmark harnesses belong in CLAUDE.md. A section whose
heading says it is for someone building the repo is the exception.

## Shape of a page

Headings are labels a reader scans and a search box matches, not lines
of prose. Page titles are sentence case.

A quickstart opens with install, then the one command that produces
output, then what came back.

A section that explains a mechanism ends in a snippet that runs it. A
snippet that is also a file in the repo is checked against that file by
a test, so the page cannot drift from the code.

A reference table links out rather than repeating a paragraph inline.
A detail stays on the page that owns it.

## Links

Docs pages link: to other pages, to source, to the projects the code
depends on. CLAUDE.md's rule against links covers code comments and
internal notes.
