//! The perf gate: run the corpus, print the stages, check the budgets.
//!
//! ```text
//! perf-gate [--runs N] [--gate-only] [--strict]
//! ```
//!
//! Warns by default and exits zero: the budgets are young and a CI
//! runner is a noisy machine, so a blown budget is news before it is a
//! verdict. `--strict` turns the same output into a failing exit code,
//! which is the switch to throw once the numbers have held still.
//!
//! Runs the same under wasi as natively, so the worker's budget is
//! measured rather than extrapolated.

use std::process::ExitCode;

use fleuron_fixtures::gate::{self, Target};
use fleuron_fixtures::{Corpus, alloc, registry};

/// The gate is the one place the tracker belongs: a binary imposes a
/// global allocator on nothing but itself.
#[global_allocator]
static ALLOCATOR: alloc::Tracking = alloc::Tracking;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("usage: perf-gate [--runs N] [--gate-only] [--strict]");
        return ExitCode::from(2);
    }
    let strict = args.iter().any(|a| a == "--strict");
    let runs = flag_value(&args, "--runs")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3);
    let books: &[Corpus] = if args.iter().any(|a| a == "--gate-only") {
        &[Corpus::GATE]
    } else {
        &Corpus::ALL
    };

    let target = Target::current();
    println!("fleuron perf gate — {}, best of {runs}", target.name());
    if !alloc::installed() {
        eprintln!("perf-gate: allocation tracking is not live; memory is unmeasured");
        return ExitCode::FAILURE;
    }

    let mut over = 0;
    for corpus in books {
        let report = gate::measure(*corpus, registry(), runs);
        println!("\n{report}");
        // Only the gate book carries budgets. The big book is there to
        // show the curve, and a book four times the size failing a
        // book-scale ceiling would say nothing.
        if *corpus != Corpus::GATE {
            continue;
        }
        println!();
        for check in report.checks(target) {
            println!("  {check}");
            if !check.passed() {
                over += 1;
            }
        }
    }

    if over == 0 {
        println!("\nall budgets met");
        return ExitCode::SUCCESS;
    }
    println!("\n{over} budget(s) over ceiling");
    if strict {
        ExitCode::FAILURE
    } else {
        println!("warning only: pass --strict to fail on this");
        ExitCode::SUCCESS
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
