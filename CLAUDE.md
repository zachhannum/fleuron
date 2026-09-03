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
  `fleuron-react`, a wrapper over it that holds no engine logic;
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
  vocabulary cannot hold warns with line and column; prose is never
  dropped.
- The three invariants (see README): styling enters as CSS; the engine
  never touches the DOM; layout never decodes images.
- "Not in the subset" in `docs/css-subset.mdx` says what the engine
  does not support yet, not what it refuses. A property listed there
  is a candidate for an issue, not a closed door.
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
  the numbers have held still. The memory ceiling does fail today:
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

`.github/workflows/release.yml` is the only thing that publishes. A
`v*` tag runs it, and so does a manual dispatch, which raises the
version, cuts the tag and carries on with it. Either way it runs the
wasm workflow again with the tag held against every version the
repository carries, then puts both tarballs and the module on a GitHub
release and both packages on the registry.

npm authenticates the workflow over OIDC, so no npm token exists.
Trusted publishing is configured per package on npmjs.com against
`release.yml`, which is why the publish steps stay in that file rather
than moving into the workflow it calls.

The one secret is `RELEASE_TOKEN`, which a dispatched release checks out
under because it raises the version on `main` and `main` is protected
against the bot. The tag goes up under `GITHUB_TOKEN` instead, since a
tag pushed under the other token would start a second release run.

`scripts/version.mjs` is where a version is read and where it is
bumped, because the number lives in the workspace, both packages, the
peer range between them, the constant the package reports itself by and
the lockfiles that mirror all of it.

## Documentation rules

Applies to code comments and all documentation — internal (CLAUDE.md,
docs/) and external (README).

**DO**

- Keep them short.
- Only write documentation when the WHY is non-obvious.
- Write docs as statements of how things are.
- Run the `humanizer` skill over prose before it lands: comments,
  doc comments, docs/, README, PR and issue bodies.

**DO NOT**

- Document what the code or doc already says.
- Document deletions.
- Document changes over time — history lives in git.
- Include links (code references, PRs, issues, error URLs).
- Explain why a rejected or unchosen alternative wasn't taken.

Run it against its own rules. Nothing in this repo is a writing
sample to match, so nothing overrides them, the em dash rule
included.

`STYLE.md` is the voice the docs, the README and the npm READMEs are
written in.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` in the CLI.
- Serialization: serde everywhere; postcard on the WASM wire.
- Public API docs (`///`) on every public item; the display-structure
  types are a cross-painter contract and get treated like documentation.
- Benchmarks: criterion, in `crates/fleuron/benches/`, one bench per
  pipeline stage, run over the fixture corpus.
