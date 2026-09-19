fn main() {
    let md = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let css = std::env::args()
        .nth(2)
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or_default();
    let (sections, warnings) =
        fleuron_markdown::to_sections(&md, "t.md", &fleuron_markdown::Options::default());
    let book = fleuron_markdown::assemble(Default::default(), sections);
    println!("frontend warnings: {warnings:?}");
    let registry = fleuron::fonts::bundled_registry().unwrap();
    let styles =
        fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("t.css", &css)])
            .compile(&book, &registry);
    println!("style warnings: {:?}", styles.warnings());
    let out =
        fleuron::layout::layout_book(&book, &styles, &registry, &fleuron::images::Assets::none());
    println!("layout warnings: {:?}", out.warnings);
    for page in &out.pages {
        println!("--- page {}", page.number);
        for item in &page.items {
            if let fleuron::pages::DrawItem::Text {
                x, y, text, size, ..
            } = item
            {
                println!("  {x:7.1} {y:7.1} {size:5.1}  {text}");
            }
            if let fleuron::pages::DrawItem::Rect { x, y, w, h, .. } = item {
                println!("  rect {x:7.1} {y:7.1} {w:7.1} {h:7.1}");
            }
        }
    }
}
