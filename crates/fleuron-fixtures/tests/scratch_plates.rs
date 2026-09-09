use std::time::Instant;

use fleuron::layout::{Fragment, Paginator};
use fleuron_fixtures::{Corpus, plates, registry};

#[test]
fn scratch() {
    for css in [
        "img { position: absolute; top: 0; right: 0; margin-left: 12pt; wrap-flow: start }",
        "img { position: absolute; top: 0; right: 0; margin-left: 12pt; wrap-flow: auto }",
        "img { position: static }",
    ] {
        one(css);
    }
}

fn one(css: &str) {
    println!("== {css}");
    let bare = Corpus::GATE.book();
    let book = plates::plated(&bare);
    let styles =
        fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("gate.css", css)])
            .compile(&book, registry());
    let assets = plates::assets(&book);
    let paginator = Paginator::with_assets(registry(), &styles, &assets);
    let start = Instant::now();
    let flows: Vec<Vec<Fragment>> = book
        .sections
        .iter()
        .map(|section| paginator.section_fragments(section))
        .collect();
    println!("section fragments {:?}", start.elapsed());
    let start = Instant::now();
    let pages = paginator.flow(&book, &flows);
    println!(
        "flow {:?}, {} pages, {} rebreaks",
        start.elapsed(),
        pages.len(),
        paginator.rebreaks()
    );
}
