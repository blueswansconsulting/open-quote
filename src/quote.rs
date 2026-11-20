//! Quote data model, validation and money maths.
//!
//! All money is held in whole pence (`i64`) and quantities in hundredths, so
//! totals never suffer floating-point drift. Rounding is half-up.

use base64::Engine;
use serde::Deserialize;

use crate::pdf::{Jpeg, Paper};
use crate::tax::{self, TaxAmount, TaxInput, TaxSetup};

pub const MAX_ITEMS: usize = 100;
pub const MAX_PHOTOS: usize = 3;
pub const MAX_LOGO_BYTES: usize = 300 * 1024;
pub const MAX_PHOTO_BYTES: usize = 1024 * 1024;
const MAX_QUANTITY: f64 = 100_000.0;
const MAX_UNIT_PRICE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Business {
    pub name: String,
    pub phone: String,
    pub email: String,
    pub address: String,
    pub vat_number: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Customer {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineItem {
    pub description: String,
    pub quantity: f64,
    /// Price per unit in pounds, e.g. 45.5 for £45.50.
    pub unit_price: f64,
    /// Optional: "labour" or "materials".
    #[serde(default)]
    pub kind: String,
    /// Whether tax applies to this line (default yes).
    #[serde(default = "yes")]
    pub taxable: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PhotoInput {
    /// Base64 JPEG, optionally as a `data:image/jpeg;base64,` URL.
    pub data: String,
    pub caption: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuoteRequest {
    #[serde(default)]
    pub business: Business,
    #[serde(default)]
    pub customer: Customer,
    #[serde(default)]
    pub quote_number: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub valid_until: String,
    /// Free-text description of the job as a whole.
    #[serde(default)]
    pub scope: String,
    pub items: Vec<LineItem>,
    /// Legacy UK switch: VAT at 20% when `tax` is absent.
    #[serde(default)]
    pub vat: bool,
    /// Tax lines for any country. Overrides `vat` when present.
    #[serde(default)]
    pub tax: Option<TaxInput>,
    /// Currency symbol, e.g. "£", "$", "€", "R" (default "£").
    #[serde(default)]
    pub currency: String,
    /// "a4" (default) or "letter".
    #[serde(default)]
    pub paper: String,
    /// "Quote", "Quotation" or "Estimate" (any short word; default "Quote").
    #[serde(default)]
    pub title: String,
    /// Label for the registration number, e.g. "VAT no.", "ABN", "GST no.".
    #[serde(default)]
    pub reg_label: String,
    #[serde(default)]
    pub notes: String,
    /// Trust line, e.g. "Verified on Electricians Ranked".
    #[serde(default)]
    pub footer_note: String,
    /// Makes the footer clickable, e.g. the trade's listing page.
    #[serde(default)]
    pub footer_link: String,
    #[serde(default)]
    pub rating: Option<f64>,
    #[serde(default)]
    pub review_count: Option<u32>,
    #[serde(default)]
    pub logo: String,
    #[serde(default)]
    pub photos: Vec<PhotoInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Unspecified,
    Labour,
    Materials,
}

impl ItemKind {
    pub fn label(self) -> Option<&'static str> {
        match self {
            ItemKind::Unspecified => None,
            ItemKind::Labour => Some("Labour"),
            ItemKind::Materials => Some("Materials"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PricedLine {
    pub description: String,
    pub kind: ItemKind,
    pub quantity_hundredths: i64,
    pub unit_pence: i64,
    pub total_pence: i64,
    pub taxable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub labour: i64,
    pub materials: i64,
    pub subtotal: i64,
    /// Sum of the taxable lines.
    pub taxable: i64,
    /// Sum of all tax lines.
    pub tax: i64,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct Photo {
    pub image: Jpeg,
    pub caption: String,
}

/// A validated quote, ready to render.
#[derive(Debug, Clone)]
pub struct Quote {
    pub business: Business,
    pub customer: Customer,
    pub quote_number: String,
    pub date: String,
    pub valid_until: String,
    pub scope: String,
    pub tax: TaxSetup,
    pub taxes: Vec<TaxAmount>,
    pub currency: String,
    pub paper: Paper,
    pub title: String,
    pub reg_label: String,
    pub notes: String,
    pub footer_note: String,
    pub footer_link: String,
    pub rating: Option<(f64, Option<u32>)>,
    pub logo: Option<Jpeg>,
    pub photos: Vec<Photo>,
    pub lines: Vec<PricedLine>,
    pub totals: Totals,
}

/// Round a non-negative amount to the nearest hundredth, half-up.
fn to_hundredths(value: f64) -> i64 {
    (value * 100.0 + 1e-9).round() as i64
}

/// `quantity_hundredths * unit_pence / 100`, rounded half-up.
pub fn line_total(quantity_hundredths: i64, unit_pence: i64) -> i64 {
    (quantity_hundredths * unit_pence + 50) / 100
}

pub fn totals(lines: &[PricedLine], setup: &TaxSetup) -> (Totals, Vec<TaxAmount>) {
    let sum_of = |kind| {
        lines
            .iter()
            .filter(|l| l.kind == kind)
            .map(|l| l.total_pence)
            .sum()
    };
    let subtotal: i64 = lines.iter().map(|l| l.total_pence).sum();
    let taxable: i64 = lines
        .iter()
        .filter(|l| l.taxable)
        .map(|l| l.total_pence)
        .sum();
    let (taxes, total) = tax::compute(subtotal, taxable, setup);
    let totals = Totals {
        labour: sum_of(ItemKind::Labour),
        materials: sum_of(ItemKind::Materials),
        subtotal,
        taxable,
        tax: taxes.iter().map(|t| t.amount).sum(),
        total,
    };
    (totals, taxes)
}

/// Turn tabs into spaces, remove other control characters, and keep
/// newlines only where `multiline`.
fn clean(text: &str, multiline: bool) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control() || (multiline && *c == '\n'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn check_len(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        Err(format!("{field} is too long (max {max} characters)"))
    } else {
        Ok(())
    }
}

fn decode_jpeg(field: &str, input: &str, max_bytes: usize) -> Result<Jpeg, String> {
    let b64 = match input.split_once(',') {
        Some((prefix, rest)) if prefix.starts_with("data:") => {
            if !prefix.starts_with("data:image/jpeg") {
                return Err(format!("{field} must be a JPEG image"));
            }
            rest
        }
        _ => input,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| format!("{field} is not valid image data"))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "{field} is too large (max {} KB)",
            max_bytes / 1024
        ));
    }
    Jpeg::parse(bytes).map_err(|e| format!("{field}: {e}"))
}

/// Accept "tradesites.ai/listing" as well as a full URL: a bare domain gets
/// `https://` added. Anything with another scheme is left for `valid_link`.
pub fn normalize_link(input: &str) -> String {
    let link = input.trim();
    if link.is_empty() || link.contains("://") {
        return link.to_string();
    }
    let host = link.split(['/', '?', '#']).next().unwrap_or("");
    let looks_like_domain = host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if looks_like_domain {
        format!("https://{link}")
    } else {
        link.to_string()
    }
}

fn valid_link(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() > "https://".len()
        && url.chars().all(|c| c.is_ascii_graphic())
}

pub fn validate(req: QuoteRequest) -> Result<Quote, String> {
    let business = Business {
        name: clean(&req.business.name, false),
        phone: clean(&req.business.phone, false),
        email: clean(&req.business.email, false),
        address: clean(&req.business.address, true),
        vat_number: clean(&req.business.vat_number, false),
    };
    let customer = Customer {
        name: clean(&req.customer.name, false),
        address: clean(&req.customer.address, true),
    };
    let quote_number = clean(&req.quote_number, false);
    let date = clean(&req.date, false);
    let valid_until = clean(&req.valid_until, false);
    let scope = clean(&req.scope, true);
    let notes = clean(&req.notes, true);
    let footer_note = clean(&req.footer_note, false);
    let footer_link = normalize_link(&req.footer_link);

    if business.name.is_empty() {
        return Err("Business name is required".into());
    }
    if customer.name.is_empty() {
        return Err("Customer name is required".into());
    }
    for (field, value, max) in [
        ("Business name", &business.name, 100),
        ("Phone", &business.phone, 40),
        ("Email", &business.email, 100),
        ("Business address", &business.address, 300),
        ("VAT number", &business.vat_number, 30),
        ("Customer name", &customer.name, 100),
        ("Customer address", &customer.address, 300),
        ("Quote number", &quote_number, 30),
        ("Date", &date, 30),
        ("Valid until", &valid_until, 30),
        ("Description of the work", &scope, 5000),
        ("Notes", &notes, 2000),
        ("Footer note", &footer_note, 120),
        ("Listing link", &footer_link, 300),
    ] {
        check_len(field, value, max)?;
    }
    if !footer_link.is_empty() && !valid_link(&footer_link) {
        return Err("Listing link must be a web address starting with https://".into());
    }
    let rating = match req.rating {
        None => None,
        Some(r) if r.is_finite() && (0.0..=5.0).contains(&r) => {
            Some(((r * 10.0).round() / 10.0, req.review_count))
        }
        Some(_) => return Err("Rating must be between 0 and 5".into()),
    };

    if req.items.is_empty() {
        return Err("Add at least one line item".into());
    }
    if req.items.len() > MAX_ITEMS {
        return Err(format!("Too many line items (max {MAX_ITEMS})"));
    }

    let mut lines = Vec::with_capacity(req.items.len());
    for (i, item) in req.items.iter().enumerate() {
        let n = i + 1;
        let description = clean(&item.description, true);
        if description.is_empty() {
            return Err(format!("Item {n}: description is required"));
        }
        check_len(&format!("Item {n} description"), &description, 2000)?;
        let kind = match item.kind.trim().to_ascii_lowercase().as_str() {
            "" => ItemKind::Unspecified,
            "labour" | "labor" => ItemKind::Labour,
            "materials" | "material" => ItemKind::Materials,
            _ => return Err(format!("Item {n}: type must be labour or materials")),
        };
        if !item.quantity.is_finite() || item.quantity <= 0.0 || item.quantity > MAX_QUANTITY {
            return Err(format!(
                "Item {n}: quantity must be above 0 and at most {MAX_QUANTITY}"
            ));
        }
        if !item.unit_price.is_finite() || item.unit_price < 0.0 || item.unit_price > MAX_UNIT_PRICE
        {
            return Err(format!(
                "Item {n}: price must be between £0 and £{MAX_UNIT_PRICE}"
            ));
        }
        let quantity_hundredths = to_hundredths(item.quantity);
        if quantity_hundredths == 0 {
            return Err(format!("Item {n}: quantity is too small"));
        }
        let unit_pence = to_hundredths(item.unit_price);
        lines.push(PricedLine {
            description,
            kind,
            quantity_hundredths,
            unit_pence,
            total_pence: line_total(quantity_hundredths, unit_pence),
            taxable: item.taxable,
        });
    }

    let logo = if req.logo.trim().is_empty() {
        None
    } else {
        Some(decode_jpeg("Logo", &req.logo, MAX_LOGO_BYTES)?)
    };
    if req.photos.len() > MAX_PHOTOS {
        return Err(format!("Too many photos (max {MAX_PHOTOS})"));
    }
    let mut photos = Vec::with_capacity(req.photos.len());
    for (i, p) in req.photos.iter().enumerate() {
        let caption = clean(&p.caption, false);
        check_len(&format!("Photo {} caption", i + 1), &caption, 200)?;
        photos.push(Photo {
            image: decode_jpeg(&format!("Photo {}", i + 1), &p.data, MAX_PHOTO_BYTES)?,
            caption,
        });
    }

    let tax_setup = tax::setup(req.tax.clone(), req.vat)?;
    let (totals, taxes) = totals(&lines, &tax_setup);
    let currency = clean(&req.currency, false);
    let currency = if currency.is_empty() {
        "£".to_string()
    } else {
        currency
    };
    if currency.chars().count() > 4 || currency.chars().any(|c| c.is_ascii_digit()) {
        return Err("Currency should be a short symbol, like £, $ or €".into());
    }
    let paper = match req.paper.trim().to_ascii_lowercase().as_str() {
        "" | "a4" => Paper::A4,
        "letter" => Paper::Letter,
        _ => return Err("Paper must be A4 or Letter".into()),
    };
    let title = clean(&req.title, false);
    let title = if title.is_empty() {
        "Quote".to_string()
    } else {
        title
    };
    check_len("Document title", &title, 20)?;
    let reg_label = clean(&req.reg_label, false);
    let reg_label = if reg_label.is_empty() {
        "VAT no.".to_string()
    } else {
        reg_label
    };
    check_len("Registration label", &reg_label, 30)?;
    Ok(Quote {
        business,
        customer,
        quote_number,
        date,
        valid_until,
        scope,
        tax: tax_setup,
        taxes,
        currency,
        paper,
        title,
        reg_label,
        notes,
        footer_note,
        footer_link,
        rating,
        logo,
        photos,
        lines,
        totals,
    })
}

/// `123456` -> `"£1,234.56"`.
pub fn format_money(pence: i64) -> String {
    format_money_in("£", pence)
}

/// `("$", 123456)` -> `"$1,234.56"`.
pub fn format_money_in(symbol: &str, pence: i64) -> String {
    let negative = pence < 0;
    let pence = pence.abs();
    let pounds = (pence / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in pounds.chars().enumerate() {
        if i > 0 && (pounds.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!(
        "{}{symbol}{}.{:02}",
        if negative { "-" } else { "" },
        grouped,
        pence % 100
    )
}

/// `250` -> `"2.5"`, `300` -> `"3"`.
pub fn format_quantity(hundredths: i64) -> String {
    if hundredths % 100 == 0 {
        (hundredths / 100).to_string()
    } else {
        let s = format!("{}.{:02}", hundredths / 100, hundredths % 100);
        s.trim_end_matches('0').to_string()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pdf::test_jpeg;

    pub(crate) fn item(description: &str, quantity: f64, unit_price: f64) -> LineItem {
        LineItem {
            description: description.into(),
            quantity,
            unit_price,
            kind: String::new(),
            taxable: true,
        }
    }

    pub(crate) fn request(items: Vec<LineItem>, vat: bool) -> QuoteRequest {
        QuoteRequest {
            business: Business {
                name: "Sparks Electrical".into(),
                ..Default::default()
            },
            customer: Customer {
                name: "Mrs Jones".into(),
                ..Default::default()
            },
            quote_number: "Q-0001".into(),
            date: "20 Nov 2026".into(),
            valid_until: "20 Dec 2026".into(),
            scope: String::new(),
            items,
            vat,
            tax: None,
            currency: String::new(),
            paper: String::new(),
            title: String::new(),
            reg_label: String::new(),
            notes: String::new(),
            footer_note: String::new(),
            footer_link: String::new(),
            rating: None,
            review_count: None,
            logo: String::new(),
            photos: vec![],
        }
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn totals_with_vat() {
        let q = validate(request(
            vec![
                item("Consumer unit", 1.0, 450.0),
                item("Labour (hours)", 6.0, 45.0),
            ],
            true,
        ))
        .unwrap();
        assert_eq!(
            (q.totals.subtotal, q.totals.tax, q.totals.total),
            (72_000, 14_400, 86_400)
        );
    }

    #[test]
    fn totals_without_vat() {
        let q = validate(request(vec![item("Socket", 3.0, 12.5)], false)).unwrap();
        assert_eq!(
            (q.totals.subtotal, q.totals.tax, q.totals.total),
            (3_750, 0, 3_750)
        );
    }

    #[test]
    fn labour_and_materials_are_split() {
        let mut labour = item("Fitting", 6.0, 45.0);
        labour.kind = "Labour".into();
        let mut materials = item("Cable", 2.0, 30.0);
        materials.kind = "materials".into();
        let q = validate(request(
            vec![labour, materials, item("Other", 1.0, 10.0)],
            false,
        ))
        .unwrap();
        assert_eq!(q.totals.labour, 27_000);
        assert_eq!(q.totals.materials, 6_000);
        assert_eq!(q.totals.subtotal, 34_000);
        assert_eq!(q.lines[2].kind, ItemKind::Unspecified);

        let mut bad = item("x", 1.0, 1.0);
        bad.kind = "plant hire".into();
        assert!(validate(request(vec![bad], false))
            .unwrap_err()
            .contains("labour or materials"));
    }

    #[test]
    fn float_prices_do_not_drift() {
        let q = validate(request(
            vec![item("A", 3.0, 0.1), item("B", 1.0, 0.2)],
            false,
        ))
        .unwrap();
        assert_eq!(q.totals.subtotal, 50);
        let q = validate(request(vec![item("Cable (m)", 1.15, 19.99)], false)).unwrap();
        assert_eq!(q.lines[0].total_pence, 2_299); // 22.9885 -> £22.99
    }

    #[test]
    fn vat_rounds_half_up() {
        let q = validate(request(vec![item("Fuse", 1.0, 2.53)], true)).unwrap();
        assert_eq!(q.totals.tax, 51); // 0.506 -> 0.51
        assert_eq!(q.totals.total, 304);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(validate(request(vec![], false))
            .unwrap_err()
            .contains("at least one"));
        assert!(validate(request(vec![item("x", 0.0, 1.0)], false))
            .unwrap_err()
            .contains("quantity"));
        assert!(validate(request(vec![item("x", -1.0, 1.0)], false)).is_err());
        assert!(validate(request(vec![item("x", 1.0, -5.0)], false))
            .unwrap_err()
            .contains("price"));
        assert!(validate(request(vec![item("x", f64::NAN, 1.0)], false)).is_err());
        assert!(validate(request(vec![item("   ", 1.0, 1.0)], false))
            .unwrap_err()
            .contains("description"));
        let too_many = (0..=MAX_ITEMS).map(|_| item("x", 1.0, 1.0)).collect();
        assert!(validate(request(too_many, false))
            .unwrap_err()
            .contains("Too many"));

        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.business.name = "  ".into();
        assert!(validate(r).unwrap_err().contains("Business name"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.customer.name = String::new();
        assert!(validate(r).unwrap_err().contains("Customer name"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.notes = "a".repeat(2001);
        assert!(validate(r).unwrap_err().contains("Notes"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.scope = "a".repeat(5001);
        assert!(validate(r).unwrap_err().contains("Description of the work"));
    }

    #[test]
    fn job_description_allows_long_text() {
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.scope = "Rewire kitchen. ".repeat(250); // 4,000 chars
        assert!(validate(r).is_ok());
        let r = request(vec![item(&"y".repeat(2000), 1.0, 1.0)], false);
        assert!(validate(r).is_ok());
    }

    #[test]
    fn cleans_control_characters_and_tabs() {
        let mut r = request(
            vec![item("Line one\r\nLine\u{7} two\tthree", 1.0, 1.0)],
            false,
        );
        r.business.name = "Sparks\u{0}Ltd\n".into();
        let q = validate(r).unwrap();
        assert_eq!(q.business.name, "SparksLtd");
        assert_eq!(q.lines[0].description, "Line one\nLine two three");
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(normalize_link("tradesites.ai"), "https://tradesites.ai");
        assert_eq!(
            normalize_link(" www.tradesites.ai/listing/sparks "),
            "https://www.tradesites.ai/listing/sparks"
        );
        assert_eq!(
            normalize_link("https://tradesites.ai"),
            "https://tradesites.ai"
        );
        assert_eq!(normalize_link("javascript:alert(1)"), "javascript:alert(1)");
        assert_eq!(normalize_link("localhost"), "localhost");
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.footer_link = "tradesites.ai".into();
        assert_eq!(validate(r).unwrap().footer_link, "https://tradesites.ai");
    }

    #[test]
    fn footer_link_and_rating_are_validated() {
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.footer_link = "javascript:alert(1)".into();
        assert!(validate(r.clone()).unwrap_err().contains("Listing link"));
        r.footer_link = "not a link".into();
        assert!(validate(r).unwrap_err().contains("Listing link"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.footer_link = "https://electriciansranked.co.uk/x y".into();
        assert!(validate(r).is_err());
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.rating = Some(5.5);
        assert!(validate(r).unwrap_err().contains("Rating"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.footer_link = "https://electriciansranked.co.uk/sparks".into();
        r.rating = Some(4.86);
        r.review_count = Some(37);
        let q = validate(r).unwrap();
        assert_eq!(q.rating, Some((4.9, Some(37))));
    }

    #[test]
    fn logo_and_photos_are_decoded_and_limited() {
        let jpeg = test_jpeg(400, 300);
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.logo = format!("data:image/jpeg;base64,{}", b64(&jpeg));
        r.photos = vec![PhotoInput {
            data: b64(&jpeg),
            caption: "Old board".into(),
        }];
        let q = validate(r).unwrap();
        assert_eq!(q.logo.unwrap().width, 400);
        assert_eq!(q.photos[0].caption, "Old board");

        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.logo = format!("data:image/png;base64,{}", b64(&jpeg));
        assert!(validate(r).unwrap_err().contains("JPEG"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.logo = "!!!not base64".into();
        assert!(validate(r).unwrap_err().contains("not valid image"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.logo = b64(b"GIF89a....");
        assert!(validate(r).unwrap_err().contains("Logo"));
        let mut big = jpeg.clone();
        big.extend(std::iter::repeat_n(0u8, MAX_LOGO_BYTES));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.logo = b64(&big);
        assert!(validate(r).unwrap_err().contains("too large"));
        let mut r = request(vec![item("x", 1.0, 1.0)], false);
        r.photos = (0..=MAX_PHOTOS)
            .map(|_| PhotoInput {
                data: b64(&jpeg),
                caption: String::new(),
            })
            .collect();
        assert!(validate(r).unwrap_err().contains("Too many photos"));
    }

    #[test]
    fn money_and_quantity_formatting() {
        assert_eq!(format_money(0), "£0.00");
        assert_eq!(format_money(5), "£0.05");
        assert_eq!(format_money(123_456), "£1,234.56");
        assert_eq!(format_money(100_000_000), "£1,000,000.00");
        assert_eq!(format_quantity(300), "3");
        assert_eq!(format_quantity(250), "2.5");
        assert_eq!(format_quantity(115), "1.15");
    }
}
