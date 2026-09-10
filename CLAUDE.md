# CLAUDE.md

Working agreements for implementing fleuron. These are policies, not
suggestions; when something here conflicts with a quick fix, the policy
wins and the quick fix waits for its own PR.

## Project shape

- Workspace: `crates/fleuron` (engine), `crates/fleuron-markdown`
  (frontend), `crates/fleuron-cli` (binary), `crates/fleuron-wasm`
  (bindings), `crates/fleuron-fixtures` (corpus and perf harness, never
  published).
- Outside the workspace: `crates/fleuron-wasm/npm` is the npm package
  `fleuron`, the TypeScript beside the module; `packages/react` is
  `fleuron-react`, a wrapper over it that contains no engine logic;
  `examples/` is written the way a consumer writes it, and the browser
  run drives it rather than a page of its own.
- Pipeline is one-way: markdown → content tree + style tree → box tree →
  line layout → fragmentation → pages → display structure / PDF.
  Downstream never reaches back upstream.
- Markdown is the way in. The content tree stays public for a host with
  a structured source of its own, but the docs lead with markdown and it
  is not advertised as a peer.
- The mapping from markdown to content tree lives in
  `docs/reference/markdown.mdx` and is implemented once. A construct the
  vocabulary cannot express warns with line and column; prose is never
  dropped.
- The three invariants (see README): styling enters as CSS; the engine
  never touches the DOM; layout never decodes images.
- "Not in the subset" in `docs/css-subset.mdx` says what the engine
  does not support yet, not what it refuses. A property listed there
  is a candidate for an issue, not a closed door.
- The tables on that page are rendered from the parser's own tables.
  After changing what the parser accepts, regenerate them with
  `FLEURON_UPDATE_DOCS=1 cargo test -p fleuron --test css_subset`.
- Work is tracked in GitHub issues, grouped by the v0.1 epic (#13). An
  issue's acceptance checkboxes are its definition of done.

## Unit testing

- Tests are colocated: `#[cfg(test)]` modules inside the file under
  test. No separate `tests/` directory inside `crates/fleuron`.
- One test per acceptance checkbox on the issue being implemented. When
  you check a box, there is a test that proves it.
- Layout invariants get **property tests** (`proptest`), not golden
  files: no line exceeds the measure; baselines are monotonically
  increasing down a page; `layout()` is deterministic (two runs,
  byte-identical output); page count stable across runs.
- The display structure and the wire format get `insta` snapshots.
  `.snap` files are reviewed like code; `cargo insta review` after
  intentional changes, never blind `--accept`.
- Property and snapshot tests live in the crate's `tests/` integration
  directory; unit tests stay colocated.

## E2E testing

There is exactly one e2e definition in this repo: **fixture book
markdown in → valid PDF out**, invoked through the CLI, living in
`crates/fleuron-cli/tests/`.

- Input: `fixtures/gulliver-excerpt.md`, checked in: realistic prose,
  dialogue, em-dashes, hyphenation-prone words; never lorem.
- CI validates the output three ways: `qpdf --check` (structure),
  `pdftotext` round-trip (word count preserved, hyphenation off for the
  test config), page-count assertions.
- Any pipeline stage that does not extend the e2e path is not done. If
  you added a stage and didn't wire it into the fixture run, finish that
  first.
- Perf is not e2e. Criterion benches report numbers; they do not gate
  PRs until #12's harness has a stable baseline. After that, a
  regression > 20% on the 300-page bench fails CI.

## Implementing changes

- Commit as you go. Do not wait until the feature is done to commit
  everything; a commit is a small testable chunk.

## Perf harness

- The corpus is two public-domain books in `fixtures/corpus/`, checked
  in as markdown and read into content trees through the shipped
  frontend, so the measured path is the shipped path. Pride and
  Prejudice is the gate: ~330 pages, the book scale the budgets are
  written against. The Count of Monte Cristo is four times that, and
  exists to expose superlinearity. No generated prose — it is uniform,
  and uniform text hides the tail cases that make layout slow.
- Budgets live in `fleuron_fixtures::gate::budget` as absolute
  ceilings, not comparisons against a stored baseline.
  `cargo run --release -p fleuron-fixtures --bin perf-gate` checks
  them; CI runs the same binary natively and under wasmtime.
- Timing verdicts warn rather than fail — a shared runner's clock is a
  trend, not a regression — and `--strict` is the switch to throw once
  the numbers have settled. The memory ceiling does fail today:
  allocation counts are identical on every machine.
- A stage that gets a bench gets a seam it can be timed through. If
  timing a stage separately means reaching into a private method, the
  seam is missing, not the bench.

## PR creation and CI

- One issue per branch: `feat/<issue>-slug`, `chore/<issue>-slug`,
  `fix/<issue>-slug`.
- PR description references the issue with `Closes #N`. The issue's
  acceptance checkboxes must all be checked before review is requested.
- PR and issue bodies are unwrapped: one line per paragraph and per
  list item, blank lines between them. GitHub renders a single newline
  as a line break, so prose wrapped to the width used for code comes
  out as a ragged column.
- Before pushing, run the CI mirror locally and make it green:
  `cargo fmt --all --check && cargo clippy --workspace --all-targets
  -- -D warnings && cargo test --workspace`. Pushing red and letting CI
  find it wastes a cycle; CI is verification, not development.
- After opening the PR, watch it to green (`gh run watch`) before
  handing it to review.
- **Claude does not merge.** CI green is the floor, not the finish line;
  a human reviews and merges every PR, including Claude's.
- Never force-push `main`. History rewrites on feature branches are fine
  while the PR is open.
- No Co-Authored-By trailers on commits.
- Keep PRs scoped to their issue, but a small fix noticed on the way
  may ride along rather than wait for a branch of its own.

## CI scaffolding

`.github/workflows/ci.yml` runs on every PR and push to main:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` (unit + property + snapshot)
4. e2e job: build CLI, run fixture book, validate with `qpdf` +
   `pdftotext` (tools installed via `apt` in the job)
5. `cargo-deny` advisories check — no merged dependency with an open
   RUSTSEC advisory (rustybuzz taught us why this job exists)
6. wasm32 build check: `cargo build -p fleuron-wasm
   --target wasm32-unknown-unknown` — the bindings must never silently
   rot while only native gets exercised
7. perf job: book-scale invariants in release, then `perf-gate` against
   the budgets natively and under wasmtime, reported to the run summary

## Releases

`.github/workflows/release.yml` is the only thing that publishes, and a
`v*` tag is the only thing that runs it: the wasm workflow again with
the tag checked against every version in the repository, then both
tarballs and the module on a GitHub release and both packages on the
registry. `cut-release.yml` is the dispatch that raises the version and
cuts that tag.

npm authenticates the workflow over OIDC, so no npm token exists.
Trusted publishing is configured per package on npmjs.com against
`release.yml`, which is why the publish steps stay in that file rather
than moving into the workflow it calls.

`cut-release.yml` raises the version on `main`, which is protected, and
a personal repository can exempt neither `github-actions[bot]` nor
GitHub's Actions app from that. So the release owns a GitHub App, whose
installation is on the branch ruleset's bypass list. It mints a token
from the `RELEASE_APP_ID` variable and the `RELEASE_APP_PRIVATE_KEY`
secret, an app id being nobody's secret. The tag goes up under the same
token, which is what starts the run that publishes.

`scripts/version.mjs` is where a version is read and where it is
bumped, because the number lives in the workspace, both packages, the
peer range between them, the constant the package reports itself by and
the lockfiles that mirror all of it.

## Documentation rules

Editing `docs/`, `README.md`, `crates/fleuron-wasm/npm/README.md` or
`packages/react/README.md` means reading `STYLE.md` first, whole, in
the same turn as the edit. Matching the prose already on the page is
not a substitute: a page can be wrong, and the guide carries rules no
paragraph on the page happens to exercise. Read the draft against it
again before committing.

`STYLE.md` is the voice those pages are written in: Simplified
English, in the spirit of ASD-STE100. Short sentences, active voice,
simple tenses, and one word for one meaning. Its before-and-after
pairs come from edits made to finished pages, so they are the samples
to match.

Four to carry into a draft, which do not replace reading the file:

- No personification. A property, a file or a warning does not say,
  know, want or announce anything. `wrap-flow` does not say which
  side of the image the text wraps on.
- No implementation detail. What a thing does belongs to the reader.
  How the engine does it does not. When a decode happens, what a
  stage caches, and what a band is are all off the page.
- `ink`, `paints`, `sets`, `the flow`, `trim`, `leaf`, `measure` and
  `sink` are flowery, not technical.
- No em dashes, no semicolons, no contractions, and no `should`,
  `may`, `might` or `could`.

**Code comments**

- Keep them short.
- Only write one when the WHY is non-obvious.
- Do not restate what the code says.
- No links (code references, PRs, issues, error URLs).
- Do not explain why an unchosen alternative was not taken.
- A comment published as documentation, such as the ones in
  `crates/fleuron/src/style/ua.css`, follows STYLE.md as well.

**Documentation**

- Say what a page covers and what an example demonstrates. A docs
  page is allowed to describe itself; a code comment is not.
- Write docs as statements of how things are.
- Do not document deletions or changes over time. History lives in
  git.

Both: no em dashes.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` in the CLI.
- Serialization: serde everywhere; postcard on the WASM wire.
- Public API docs (`///`) on every public item; the display-structure
  types are a cross-painter contract and get treated like documentation.
- Benchmarks: criterion, in `crates/fleuron/benches/`, one bench per
  pipeline stage, run over the fixture corpus.
