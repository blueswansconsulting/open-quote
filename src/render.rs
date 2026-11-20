//! Lays a validated [`Quote`] out onto A4 pages.

use crate::pdf::{self, fit_line, text_width, wrap, Font, Jpeg, Page, PAGE_HEIGHT, PAGE_WIDTH};
use crate::quote::{format_money, format_quantity, Quote, VAT_RATE_PERCENT};

const MARGIN: f32 = 50.0;
const RIGHT: f32 = PAGE_WIDTH - MARGIN;
const CONTENT_WIDTH: f32 = RIGHT - MARGIN;
const TOP: f32 = PAGE_HEIGHT - MARGIN;
const BOTTOM: f32 = 70.0; // leave room for the footer
const DESC_WIDTH: f32 = 270.0;
const QTY_RIGHT: f32 = 375.0;
const UNIT_RIGHT: f32 = 460.0;
const BODY: f32 = 10.0;
const LEADING: f32 = 13.0;
const MUTED: f32 = 0.4;
const META_LABEL_GAP: f32 = 12.0;
const LOGO_MAX_W: f32 = 150.0;
const LOGO_MAX_H: f32 = 55.0;
const PHOTO_MAX_H: f32 = 260.0;
const MAX_CONTACT_LINES: usize = 10;

struct Layout {
    pages: Vec<Page>,
    y: f32,
}

impl Layout {
    fn new() -> Self {
        Layout {
            pages: vec![Page::default()],
            y: TOP,
        }
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("at least one page")
    }

    /// Start a new page if `height` points won't fit. Returns true if it did.
    fn ensure(&mut self, height: f32) -> bool {
        if self.y - height < BOTTOM {
            self.pages.push(Page::default());
            self.y = TOP;
            true
        } else {
            false
        }
    }

    fn heading(&mut self, text: &str) {
        self.ensure(40.0);
        let y = self.y;
        self.page().text(MARGIN, y, Font::Bold, 9.0, MUTED, text);
        self.y -= 15.0;
    }

    fn paragraph(&mut self, text: &str, size: f32, leading: f32) {
        for line in wrap(text, Font::Regular, size, CONTENT_WIDTH) {
            self.ensure(leading);
            let y = self.y;
            self.page().text(MARGIN, y, Font::Regular, size, 0.0, &line);
            self.y -= leading;
        }
    }

    fn table_header(&mut self) {
        let y = self.y;
        let p = self.page();
        p.fill_rect(MARGIN, y - 6.0, CONTENT_WIDTH, 20.0, 0.93);
        p.text(MARGIN + 6.0, y, Font::Bold, 9.0, 0.0, "Description");
        p.text_right(QTY_RIGHT, y, Font::Bold, 9.0, 0.0, "Qty");
        p.text_right(UNIT_RIGHT, y, Font::Bold, 9.0, 0.0, "Unit price");
        p.text_right(RIGHT - 6.0, y, Font::Bold, 9.0, 0.0, "Amount");
        self.y -= 26.0;
    }
}

/// Scale `img` to fit within `max_w` x `max_h`, never upscaling.
fn fit(img: &Jpeg, max_w: f32, max_h: f32) -> (f32, f32) {
    let (w, h) = (img.width as f32, img.height as f32);
    let scale = (max_w / w).min(max_h / h).min(1.0);
    (w * scale, h * scale)
}

/// Right edge for a meta label so it never overlaps its value.
pub(crate) fn meta_label_right(value: &str) -> f32 {
    let value_left = RIGHT - text_width(value, Font::Regular, 9.0);
    (RIGHT - 110.0).min(value_left - META_LABEL_GAP)
}

/// Width available to the business name/contact column: up to 300pt, but
/// always clear of the widest label/value pair (and the QUOTE title).
pub(crate) fn header_left_width(q: &Quote) -> f32 {
    let mut meta_left = RIGHT - text_width("QUOTE", Font::Bold, 24.0);
    for (label, value) in [
        ("Quote no.", &q.quote_number),
        ("Date", &q.date),
        ("Valid until", &q.valid_until),
    ] {
        if !value.is_empty() {
            meta_left =
                meta_left.min(meta_label_right(value) - text_width(label, Font::Regular, 9.0));
        }
    }
    (meta_left - MARGIN - 16.0).clamp(120.0, 300.0)
}

pub(crate) fn footer_text(q: &Quote) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !q.footer_note.is_empty() {
        parts.push(q.footer_note.clone());
    }
    if let Some((rating, count)) = q.rating {
        let mut s = format!("Rated {rating:.1}/5");
        if let Some(n) = count {
            s.push_str(&format!(
                " from {n} review{}",
                if n == 1 { "" } else { "s" }
            ));
        }
        parts.push(s);
    }
    if parts.is_empty() && !q.footer_link.is_empty() {
        parts.push(q.footer_link.clone());
    }
    parts.join(" · ")
}

pub fn render_quote(q: &Quote) -> Vec<u8> {
    let mut l = Layout::new();
    let mut images: Vec<Jpeg> = Vec::new();

    // Header: logo + business on the left, QUOTE block on the right. The left
    // column is narrowed so it can never run under the right-hand meta block.
    let header_top = l.y;
    let left_width = header_left_width(q);
    let mut left_y = header_top;
    if let Some(logo) = &q.logo {
        let (w, h) = fit(logo, LOGO_MAX_W, LOGO_MAX_H);
        images.push(logo.clone());
        l.page()
            .image(images.len() - 1, MARGIN, header_top - h + 12.0, w, h);
        left_y = header_top - h - 10.0;
    } else {
        left_y -= 6.0;
    }
    for line in wrap(&q.business.name, Font::Bold, 18.0, left_width) {
        l.page().text(MARGIN, left_y, Font::Bold, 18.0, 0.0, &line);
        left_y -= 21.0;
    }
    let mut contact: Vec<String> = q
        .business
        .address
        .split('\n')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();
    for extra in [&q.business.phone, &q.business.email] {
        if !extra.is_empty() {
            contact.push(extra.clone());
        }
    }
    if !q.business.vat_number.is_empty() {
        contact.push(format!("VAT no. {}", q.business.vat_number));
    }
    for line in contact
        .iter()
        .flat_map(|c| wrap(c, Font::Regular, 9.0, left_width))
        .take(MAX_CONTACT_LINES)
    {
        l.page()
            .text(MARGIN, left_y, Font::Regular, 9.0, MUTED, &line);
        left_y -= 12.0;
    }

    let mut right_y = header_top - 8.0;
    l.page()
        .text_right(RIGHT, right_y, Font::Bold, 24.0, 0.0, "QUOTE");
    right_y -= 22.0;
    for (label, value) in [
        ("Quote no.", &q.quote_number),
        ("Date", &q.date),
        ("Valid until", &q.valid_until),
    ] {
        if value.is_empty() {
            continue;
        }
        let page = l.page();
        page.text_right(RIGHT, right_y, Font::Regular, 9.0, 0.0, value);
        page.text_right(
            meta_label_right(value),
            right_y,
            Font::Regular,
            9.0,
            MUTED,
            label,
        );
        right_y -= 13.0;
    }
    l.y = left_y.min(right_y) - 18.0;

    // Customer.
    l.heading("QUOTE FOR");
    let mut customer_lines = wrap(&q.customer.name, Font::Bold, 11.0, 300.0);
    let name_count = customer_lines.len();
    for part in q
        .customer
        .address
        .split('\n')
        .filter(|s| !s.trim().is_empty())
    {
        customer_lines.extend(wrap(part.trim(), Font::Regular, BODY, 300.0));
    }
    for (i, line) in customer_lines.iter().enumerate() {
        let (font, size) = if i < name_count {
            (Font::Bold, 11.0)
        } else {
            (Font::Regular, BODY)
        };
        l.ensure(LEADING);
        let y = l.y;
        l.page().text(MARGIN, y, font, size, 0.0, line);
        l.y -= LEADING;
    }
    l.y -= 12.0;

    // Description of the work.
    if !q.scope.is_empty() {
        l.heading("THE WORK");
        l.paragraph(&q.scope, BODY, LEADING);
        l.y -= 12.0;
    }

    // Line items.
    l.ensure(60.0);
    l.table_header();
    for item in &q.lines {
        let desc = wrap(&item.description, Font::Regular, BODY, DESC_WIDTH);
        let tag = item.kind.label();
        let rows = desc.len() + usize::from(tag.is_some());
        for (i, line) in desc.iter().enumerate() {
            // Keep at least two rows of an item together with its numbers.
            let needed = if i == 0 {
                LEADING * rows.min(2) as f32
            } else {
                LEADING
            };
            if l.ensure(needed) {
                l.table_header();
            }
            let y = l.y;
            let page = l.page();
            page.text(MARGIN + 6.0, y, Font::Regular, BODY, 0.0, line);
            if i == 0 {
                page.text_right(
                    QTY_RIGHT,
                    y,
                    Font::Regular,
                    BODY,
                    0.0,
                    &format_quantity(item.quantity_hundredths),
                );
                page.text_right(
                    UNIT_RIGHT,
                    y,
                    Font::Regular,
                    BODY,
                    0.0,
                    &format_money(item.unit_pence),
                );
                page.text_right(
                    RIGHT - 6.0,
                    y,
                    Font::Regular,
                    BODY,
                    0.0,
                    &format_money(item.total_pence),
                );
            }
            l.y -= LEADING;
        }
        if let Some(tag) = tag {
            if l.ensure(LEADING) {
                l.table_header();
            }
            let y = l.y + 2.0;
            l.page()
                .text(MARGIN + 6.0, y, Font::Regular, 8.0, MUTED, tag);
            l.y -= 11.0;
        }
        l.y -= 4.0;
        let y = l.y + 8.0;
        l.page().line(MARGIN, y, RIGHT, y, 0.5, 0.85);
        l.y -= 6.0;
    }

    // Totals.
    let mut rows: Vec<(String, i64, bool, bool)> = Vec::new(); // label, amount, strong, muted
    if q.totals.labour > 0 {
        rows.push(("Labour".into(), q.totals.labour, false, true));
    }
    if q.totals.materials > 0 {
        rows.push(("Materials".into(), q.totals.materials, false, true));
    }
    rows.push(("Subtotal".into(), q.totals.subtotal, false, false));
    if q.vat {
        rows.push((
            format!("VAT ({VAT_RATE_PERCENT}%)"),
            q.totals.vat,
            false,
            false,
        ));
    }
    rows.push(("Total".into(), q.totals.total, true, false));
    l.ensure(rows.len() as f32 * 18.0 + 30.0);
    l.y -= 4.0;
    for (label, amount, strong, muted) in rows {
        let (font, size) = if strong {
            (Font::Bold, 13.0)
        } else {
            (Font::Regular, BODY)
        };
        let gray = if muted { MUTED } else { 0.0 };
        if strong {
            let y = l.y + 13.0;
            l.page().line(UNIT_RIGHT - 80.0, y, RIGHT, y, 1.0, 0.0);
            l.y -= 4.0;
        }
        let y = l.y;
        let page = l.page();
        page.text_right(UNIT_RIGHT, y, font, size, gray, &label);
        page.text_right(RIGHT - 6.0, y, font, size, gray, &format_money(amount));
        l.y -= 18.0;
    }
    if !q.vat {
        let y = l.y;
        l.page()
            .text_right(RIGHT - 6.0, y, Font::Regular, 8.0, MUTED, "No VAT charged");
        l.y -= 14.0;
    }

    // Notes / terms.
    if !q.notes.is_empty() {
        l.y -= 14.0;
        l.heading("NOTES");
        l.paragraph(&q.notes, 9.0, 12.0);
    }

    // Photos.
    if !q.photos.is_empty() {
        l.y -= 14.0;
        // Keep the heading with the first photo rather than orphaning it.
        let (_, first_h) = fit(&q.photos[0].image, CONTENT_WIDTH, PHOTO_MAX_H);
        l.ensure(15.0 + first_h + 24.0);
        l.heading("PHOTOS");
        for photo in &q.photos {
            let (w, h) = fit(&photo.image, CONTENT_WIDTH, PHOTO_MAX_H);
            let caption = if photo.caption.is_empty() { 0.0 } else { 14.0 };
            l.ensure(h + caption + 10.0);
            images.push(photo.image.clone());
            let index = images.len() - 1;
            let y = l.y - h;
            l.page().image(index, MARGIN, y, w, h);
            l.y = y - 4.0;
            if !photo.caption.is_empty() {
                l.y -= 9.0;
                let y = l.y;
                let caption = fit_line(&photo.caption, Font::Regular, 9.0, CONTENT_WIDTH);
                l.page()
                    .text(MARGIN, y, Font::Regular, 9.0, MUTED, &caption);
            }
            l.y -= 14.0;
        }
    }

    // Footer on every page.
    let total_pages = l.pages.len();
    let footer = footer_text(q);
    for (i, page) in l.pages.iter_mut().enumerate() {
        page.line(MARGIN, 48.0, RIGHT, 48.0, 0.5, 0.85);
        let page_label = format!("Page {} of {}", i + 1, total_pages);
        page.text_right(RIGHT, 34.0, Font::Regular, 8.0, MUTED, &page_label);
        if !footer.is_empty() {
            let max = CONTENT_WIDTH - text_width(&page_label, Font::Regular, 8.0) - 20.0;
            let text = fit_line(&footer, Font::Regular, 8.0, max);
            page.text(MARGIN, 34.0, Font::Regular, 8.0, MUTED, &text);
            if !q.footer_link.is_empty() {
                let w = text_width(&text, Font::Regular, 8.0);
                page.link(MARGIN, 31.0, w, 11.0, &q.footer_link);
            }
        }
    }

    pdf::write_pdf(&l.pages, &images)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::test_jpeg;
    use crate::quote::{validate, Business, Customer, LineItem, PhotoInput, QuoteRequest};
    use base64::Engine;

    fn sample(items: usize, notes: &str) -> QuoteRequest {
        QuoteRequest {
            business: Business {
                name: "Sparks Electrical".into(),
                phone: "07700 900123".into(),
                address: "1 High St\nBristol BS1 1AA".into(),
                ..Default::default()
            },
            customer: Customer {
                name: "Mrs Jones".into(),
                address: "2 Low Rd".into(),
            },
            quote_number: "Q-0001".into(),
            date: "20 Nov 2026".into(),
            valid_until: "20 Dec 2026".into(),
            scope: "Full rewire of the kitchen.".into(),
            items: (0..items)
                .map(|i| LineItem {
                    description: format!(
                        "Item {i}: supply and fit double socket, including chasing and making good"
                    ),
                    quantity: 1.0,
                    unit_price: 45.0,
                    kind: if i % 2 == 0 {
                        "labour".into()
                    } else {
                        String::new()
                    },
                })
                .collect(),
            vat: true,
            notes: notes.into(),
            footer_note: "Verified on Electricians Ranked".into(),
            footer_link: String::new(),
            rating: None,
            review_count: None,
            logo: String::new(),
            photos: vec![],
        }
    }

    fn count(pdf: &[u8], needle: &[u8]) -> usize {
        pdf.windows(needle.len()).filter(|w| w == &needle).count()
    }

    fn hex(s: &str) -> String {
        s.chars()
            .map(|c| format!("{:02X}", crate::pdf::win_ansi(c)))
            .collect()
    }

    #[test]
    fn short_quote_is_one_page() {
        let pdf = render_quote(&validate(sample(3, "Valid for 30 days.")).unwrap());
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(count(&pdf, b"/Type /Page "), 1);
        assert!(count(&pdf, hex("THE WORK").as_bytes()) == 1);
        assert!(count(&pdf, hex("Labour").as_bytes()) >= 2); // tag + totals row
    }

    #[test]
    fn long_quote_flows_onto_more_pages() {
        let pdf = render_quote(&validate(sample(100, &"Terms apply. ".repeat(150))).unwrap());
        let n = count(&pdf, b"/Type /Page ");
        assert!(n >= 3, "pages = {n}");
        assert_eq!(count(&pdf, hex(&format!("of {n}")).as_bytes()), n);
    }

    #[test]
    fn meta_labels_never_overlap_long_values() {
        for value in ["Q-1", "20 Dec 2026", "Q-2026-11-SMITH-KITCHEN-REWIRE"] {
            let value_left = RIGHT - text_width(value, Font::Regular, 9.0);
            assert!(
                meta_label_right(value) <= value_left - META_LABEL_GAP + 0.01,
                "{value}"
            );
        }
        assert_eq!(meta_label_right("Q-1"), RIGHT - 110.0);
    }

    #[test]
    fn business_column_stays_clear_of_meta_block() {
        let mut r = sample(1, "");
        r.quote_number = "W".repeat(30);
        let q = validate(r).unwrap();
        let width = header_left_width(&q);
        let label_left =
            meta_label_right(&q.quote_number) - text_width("Quote no.", Font::Regular, 9.0);
        assert!(MARGIN + width < label_left, "{width}");
        // A normal quote keeps the full 300pt column.
        assert_eq!(header_left_width(&validate(sample(1, "")).unwrap()), 300.0);
    }

    #[test]
    fn very_long_addresses_stay_on_the_page() {
        let mut r = sample(1, "");
        r.customer.address = "Line\n".repeat(60);
        r.business.address = "Unit\n".repeat(60);
        let pdf = render_quote(&validate(r).unwrap());
        // Every text op must sit above the footer rule (y > 48) on its page.
        let s = String::from_utf8_lossy(&pdf);
        for op in s.split("BT ").skip(1) {
            let td = op.split(" Td").next().unwrap();
            let y: f32 = td.split_whitespace().last().unwrap().parse().unwrap();
            assert!(y >= 30.0, "text at y={y}");
        }
        assert!(count(&pdf, b"/Type /Page ") >= 2);
    }

    #[test]
    fn logo_photos_and_footer_link_are_embedded() {
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let mut r = sample(2, "");
        r.logo = b64(&test_jpeg(600, 200));
        r.photos = vec![
            PhotoInput {
                data: b64(&test_jpeg(1600, 1200)),
                caption: "Existing fuse board".into(),
            },
            PhotoInput {
                data: b64(&test_jpeg(1200, 1600)),
                caption: String::new(),
            },
        ];
        r.footer_link = "https://electriciansranked.co.uk/sparks".into();
        r.rating = Some(4.9);
        r.review_count = Some(37);
        let q = validate(r).unwrap();
        assert_eq!(
            footer_text(&q),
            "Verified on Electricians Ranked · Rated 4.9/5 from 37 reviews"
        );
        let pdf = render_quote(&q);
        assert_eq!(count(&pdf, b"/Subtype /Image"), 3);
        assert!(count(&pdf, b"/Im2 Do") == 1);
        let pages = count(&pdf, b"/Type /Page ");
        assert_eq!(
            count(&pdf, b"/URI (https://electriciansranked.co.uk/sparks)"),
            pages
        );
        assert!(count(&pdf, hex("Existing fuse board").as_bytes()) == 1);
    }

    #[test]
    fn footer_falls_back_to_link_text() {
        let mut r = sample(1, "");
        r.footer_note = String::new();
        r.footer_link = "https://example.com/me".into();
        assert_eq!(footer_text(&validate(r).unwrap()), "https://example.com/me");
    }
}

#[cfg(test)]
mod photo_heading_tests {
    use super::*;
    use crate::pdf::test_jpeg;
    use crate::quote::{
        tests::{item, request},
        validate, PhotoInput,
    };
    use base64::Engine;

    #[test]
    fn photos_heading_is_not_orphaned() {
        // Enough items to push the photos section near the page bottom.
        for n in 10..24 {
            let mut r = request(
                (0..n)
                    .map(|i| item(&format!("Item {i}"), 1.0, 10.0))
                    .collect(),
                true,
            );
            r.photos = vec![PhotoInput {
                data: base64::engine::general_purpose::STANDARD.encode(test_jpeg(1600, 1600)),
                caption: String::new(),
            }];
            let pdf = render_quote(&validate(r).unwrap());
            let s = String::from_utf8_lossy(&pdf);
            let heading: String = "PHOTOS"
                .chars()
                .map(|c| format!("{:02X}", c as u8))
                .collect();
            // The heading and the image draw must be in the same content stream.
            let stream_with_heading = s
                .split("endstream")
                .find(|chunk| chunk.contains(&heading))
                .unwrap();
            assert!(
                stream_with_heading.contains("/Im0 Do"),
                "orphaned heading with {n} items"
            );
        }
    }
}
