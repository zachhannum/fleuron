# CLAUDE.md

Working agreements for implementing fleuron. These are policies, not
suggestions; when something here conflicts with a quick fix, the policy
wins and the quick fix waits for its own PR.

## Project shape

- Workspace: `crates/fleuron` (engine), `crates/fleuron-cli` (binary),
  `crates/fleuron-wasm` (bindings), `crates/fleuron-fixtures` (corpus
  and perf harness, never published).
- Pipeline is one-way: content tree + style tree → box tree → line
  layout → fragmentation → pages → display list / PDF. Downstream never
  reaches back upstream.
- The three invariants (see README): styling enters as CSS; the engine
  never touches the DOM; layout never decodes images.
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
- Display-list and wire-format structures get `insta` snapshots. `.snap`
  files are reviewed like code; `cargo insta review` after intentional
  changes, never blind `--accept`.
- Property and snapshot tests live in the crate's `tests/` integration
  directory; unit tests stay colocated.

## E2E testing

There is exactly one e2e definition in this repo: **fixture book JSON
in → valid PDF out**, invoked through the CLI, living in
`crates/fleuron-cli/tests/`.

- Input: `fixtures/book.json` (checked in, realistic prose — dialogue,
  em-dashes, hyphenation-prone words; never lorem).
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
  in as markdown and read into content trees by the fixtures crate.
  Pride and Prejudice is the gate: ~330 pages, the book scale the
  budgets are written against. The Count of Monte Cristo is four times
  that, and exists to expose superlinearity. No generated prose — it is
  uniform, and uniform text hides the tail cases that make layout slow.
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

## Documentation rules

Applies to code comments and all documentation — internal (CLAUDE.md,
docs/) and external (README).

**DO**

- Keep them short.
- Only write documentation when the WHY is non-obvious.
- Write docs as statements of how things are.

**DO NOT**

- Document what the code or doc already says.
- Document deletions.
- Document changes over time — history lives in git.
- Include links (code references, PRs, issues, error URLs).
- Explain why a rejected or unchosen alternative wasn't taken.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` in the CLI.
- Serialization: serde everywhere; postcard on the WASM wire.
- Public API docs (`///`) on every public item; the display-list types
  are a cross-painter contract and get treated like documentation.
- Benchmarks: criterion, in `crates/fleuron/benches/`, one bench per
  pipeline stage, run over the fixture corpus.
