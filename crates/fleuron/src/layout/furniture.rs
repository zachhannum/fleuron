//! The page margin boxes: a folio, a running head, and the band
//! each of them sits in.

use std::collections::BTreeMap;

use crate::lines::ParagraphStyle;
use crate::pages::{Page, Side};
use crate::style::{Align, Band, Content, MarginBox, MarginBoxStyle, PageStyle};

use super::Paginator;
use super::flow::PageInfo;

/// The running strings in force, by name.
pub(super) type Strings = BTreeMap<String, String>;

/// What one page's furniture resolves its content against: the folio
/// the page counted to, and the running strings it opened with.
struct Furniture<'a> {
    folio: u32,
    strings: &'a Strings,
}

/// The folio each page prints: one past the page before it, or the
/// number a page restarts the count at.
pub(crate) fn folios(infos: &[PageInfo]) -> Vec<u32> {
    let mut folio = 0;
    infos
        .iter()
        .map(|info| {
            folio = info.reset.unwrap_or(folio + 1);
            folio
        })
        .collect()
}

impl Paginator<'_> {
    /// Settles numbering and side once the whole flow is assembled,
    /// then paints each page's margin boxes: a folio's digits are not
    /// known until the pages before it are.
    ///
    /// Idempotent. What an earlier paint left is discarded first, so
    /// a session that only changed its furniture repaints in place.
    ///
    /// The folio counts pages, and `counter-reset: page` restarts it
    /// where a section asked; the side counts leaves, and nothing
    /// restarts that — recto and verso are where a page falls in the
    /// sheet, not what is printed on it.
    ///
    /// A page inserted to square the sheet paints no furniture. A
    /// blank leaf is blank: a page whose only content would be a
    /// running head does not get one.
    pub(crate) fn paint(&self, pages: &mut [Page], infos: &[PageInfo]) {
        let numbers = folios(infos);
        for (((index, page), info), folio) in pages.iter_mut().enumerate().zip(infos).zip(numbers) {
            // Furniture is appended after the page's own content, so
            // dropping the tail is all a repaint has to undo.
            page.items.truncate(info.content_items);
            page.number = folio;
            page.side = Side::of_number(index as u32 + 1);
            if info.slot.blank {
                continue;
            }
            let master = self.styles.page(info.slot.query(page.side));
            for which in MarginBox::ALL {
                let Some(box_style) = master.margin_box(which) else {
                    continue;
                };
                let Some((band, align)) = which.band() else {
                    continue;
                };
                let furniture = Furniture {
                    folio,
                    strings: &info.strings,
                };
                self.paint_margin_box(page, master, box_style, band, align, furniture);
            }
        }
    }

    /// Paints one page margin box. Its content is a line like any
    /// other — shaped, measured, placed on the band's baseline — so
    /// furniture and prose paint through the same path.
    fn paint_margin_box(
        &self,
        page: &mut Page,
        master: &PageStyle,
        box_style: &MarginBoxStyle,
        band: Band,
        align: Align,
        furniture: Furniture<'_>,
    ) {
        let text = match &box_style.content {
            Content::None | Content::Pieces(_) => return,
            Content::Counter(counter) => counter.format(furniture.folio),
            Content::String(name) => furniture.strings.get(name).cloned().unwrap_or_default(),
            Content::Text(text) => text.clone(),
        };
        if text.is_empty() {
            return;
        }
        let style = box_style.style.paragraph();
        let Some(line) = self.line_of(&text, style) else {
            return;
        };
        let (band_top, _) = margin_band(master, band, style);
        let baseline = band_top + line.box_.baseline;
        let text_width = self.line_width(&line);
        let x = match align {
            // Centred on the trim, not on the content box: a folio
            // belongs on the page's axis, and mirrored margins put
            // the content box off it.
            Align::Center => (master.geometry.width - text_width) / 2.0,
            Align::Start => master.geometry.margin.left,
            Align::End => master.geometry.width - master.geometry.margin.right - text_width,
        };
        page.items.append(&mut self.text_items(&line, x, baseline));
    }
}

/// The band one margin box's line sits in: `(top, height)` in page
/// coordinates, one line tall, centred in the margin it lives in.
pub fn margin_band(master: &PageStyle, band: Band, style: ParagraphStyle) -> (f32, f32) {
    let (start, margin) = match band {
        Band::Top => (0.0, master.geometry.margin.top),
        Band::Bottom => (
            master.geometry.height - master.geometry.margin.bottom,
            master.geometry.margin.bottom,
        ),
    };
    let height = style.size * style.line_height;
    (start + margin / 2.0 - height / 2.0, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::testing::{
        chapter, folio, master, opens_a_chapter, paginate, run_width_pt, ua,
    };
    use crate::pages::{DrawItem, Side};
    use crate::style::{MarginBox, Situation};

    /// Folios are correct and sequential — every page that shows a
    /// folio shows its own number, and the folios read in page order
    /// with no repeats or gaps among body pages.
    #[test]
    fn folios_are_correct_and_sequential() {
        let pages = paginate(vec![chapter("Chapter One", 24), chapter("Chapter Two", 24)]);
        assert!(
            pages.len() > 4,
            "expected a multi-page book, got {}",
            pages.len()
        );
        let mut numbered = Vec::new();
        for page in &pages {
            if let Some((_, digits)) = folio(page) {
                assert_eq!(
                    digits,
                    page.number.to_string(),
                    "page {} shows folio {digits}",
                    page.number
                );
                numbered.push(page.number);
            }
        }
        assert!(numbered.len() >= 2, "expected folios on the body pages");
        assert!(
            numbered.windows(2).all(|w| w[1] > w[0]),
            "folios out of order: {numbered:?}"
        );
    }

    /// The folio is suppressed on chapter opens, because
    /// `@page chapter:first` says so. A chapter's first page counts —
    /// the next folio is one past it — but shows nothing; inserted
    /// blank versos are equally blind, by `@page :blank`.
    #[test]
    fn folios_are_suppressed_on_chapter_opens() {
        let pages = paginate(vec![chapter("Chapter One", 14), chapter("Chapter Two", 14)]);
        let opens: Vec<u32> = pages
            .iter()
            .filter(|p| opens_a_chapter(p))
            .map(|p| p.number)
            .collect();
        assert_eq!(opens.len(), 2, "two chapters, two opening pages");
        for page in &pages {
            let blind = opens_a_chapter(page) || page.items.is_empty();
            assert_eq!(
                folio(page).is_some(),
                !blind,
                "page {}: folio presence wrong (opens chapter: {}, blank: {})",
                page.number,
                opens_a_chapter(page),
                page.items.is_empty()
            );
        }
        // Counted, not shown: the page after an open shows its own
        // number, one past the blind one.
        for open in opens {
            if let Some(next) = pages.get(open as usize) {
                assert_eq!(folio(next).map(|(_, d)| d), Some((open + 1).to_string()));
            }
        }
    }

    /// The folio baseline sits in the bottom margin box — strictly
    /// below the content area, inside the margin band — and is
    /// centred on the trim, not on the content box (whose mirrored
    /// margins are off-centre).
    #[test]
    fn folio_baseline_sits_in_the_margin_box() {
        let pages = paginate(vec![chapter("Chapter One", 20)]);
        let mut checked = 0;
        for page in &pages {
            let master = master(Situation::Body(page.side));
            let geometry = master.geometry;
            let folio_style = master
                .margin_box(MarginBox::BottomCenter)
                .expect("body pages have a folio")
                .style
                .paragraph();
            let (band_top, band_height) = margin_band(master, Band::Bottom, folio_style);
            let (_, content_top) = geometry.content_origin();
            let content_bottom = content_top + geometry.content_size().1;
            let Some((
                DrawItem::Text {
                    x, y, glyphs, size, ..
                },
                _,
            )) = folio(page)
            else {
                continue;
            };
            assert!(
                *y > content_bottom,
                "page {}: folio baseline {y} is inside the content area (bottom {content_bottom})",
                page.number
            );
            assert!(
                *y >= band_top && *y <= band_top + band_height,
                "page {}: folio baseline {y} outside the margin box [{band_top}, {}]",
                page.number,
                band_top + band_height
            );
            assert!(
                *y + geometry.margin.bottom / 4.0 < geometry.height,
                "page {}: folio baseline {y} runs off the trim",
                page.number
            );
            let width = run_width_pt(glyphs, *size);
            assert!(
                (x + width / 2.0 - geometry.width / 2.0).abs() < 1e-3,
                "page {}: folio centered at {}, trim center {}",
                page.number,
                x + width / 2.0,
                geometry.width / 2.0
            );
            checked += 1;
        }
        assert!(checked >= 2, "expected folios to check");
    }

    /// The running-head slot is reserved geometry in the top margin
    /// and stays empty: the built-in sheet generates no top margin
    /// box, so nothing paints above the content box.
    #[test]
    fn running_head_slot_is_reserved_and_empty() {
        let master = master(Situation::Body(Side::Recto));
        let head = margin_band(master, Band::Top, ua().root().paragraph());
        let (_, content_top) = master.geometry.content_origin();
        assert!(head.0 > 0.0);
        assert!(
            head.0 + head.1 <= content_top,
            "running head overlaps the content box"
        );
        assert!(master.margin_box(MarginBox::TopCenter).is_none());
        for page in paginate(vec![chapter("Chapter One", 14)]) {
            for item in &page.items {
                if let DrawItem::Text { y, .. } = item {
                    assert!(
                        *y >= content_top,
                        "page {}: something painted in the running-head slot at {y}",
                        page.number
                    );
                }
            }
        }
    }
}
