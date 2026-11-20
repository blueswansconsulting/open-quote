//! Minimal, dependency-free PDF writer.
//!
//! Uses the two standard Helvetica fonts every PDF reader ships with, so no
//! font files are embedded and output stays small. Text is encoded as
//! WinAnsi (covers £, €, accents and smart quotes); Latin Extended-A letters
//! such as Welsh ŵ/ŷ or Polish ł are folded to their base letter. JPEG images
//! are embedded as-is (DCTDecode), so nothing is re-encoded server-side.

pub const PAGE_WIDTH: f32 = 595.0; // A4 in points
pub const PAGE_HEIGHT: f32 = 842.0;

/// Paper size. Content is laid out for A4's width, which also fits Letter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

impl Paper {
    /// (width, height) in points.
    pub fn size(self) -> (f32, f32) {
        match self {
            Paper::A4 => (PAGE_WIDTH, PAGE_HEIGHT),
            Paper::Letter => (612.0, 792.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
}

impl Font {
    fn resource(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
        }
    }
}

/// Base letters for U+0100..=U+017F (Latin Extended-A), in code point order.
const LATIN_EXT_A_FOLD: &str = concat!(
    "AaAaAa",
    "CcCcCcCc",
    "DdDd",
    "EeEeEeEeEe",
    "GgGgGgGg",
    "HhHh",
    "IiIiIiIiIi",
    "Ii",
    "Jj",
    "Kkk",
    "LlLlLlLlLl",
    "NnNnNnnNn",
    "OoOoOo",
    "Oo",
    "RrRrRr",
    "SsSsSsSs",
    "TtTtTt",
    "UuUuUuUuUuUu",
    "Ww",
    "YyY",
    "ZzZzZz",
    "s",
);

/// Map a char to its WinAnsiEncoding byte. Returns `None` if it has no
/// sensible representation (emoji, CJK, symbols such as ✓).
pub fn try_win_ansi(c: char) -> Option<u8> {
    let byte = match c {
        ' '..='~' => c as u8,
        '\t' => b' ',
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '\u{100}'..='\u{17f}' => LATIN_EXT_A_FOLD.as_bytes()[(c as usize) - 0x100],
        _ => return None,
    };
    Some(byte)
}

/// WinAnsi byte for `c`, or `?` when it can't be represented.
pub fn win_ansi(c: char) -> u8 {
    try_win_ansi(c).unwrap_or(b'?')
}

/// Helvetica advance widths (1/1000 em) for ASCII 32..=126.
const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278,
    278, // space../
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // :;<=>?@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // A-M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // N-Z
    278, 278, 278, 469, 556, 333, // [\]^_`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // a-m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // n-z
    334, 260, 334, 584, // {|}~
];

fn char_width(c: char, font: Font) -> f32 {
    let byte = win_ansi(c);
    let base = match byte {
        32..=126 => HELVETICA_WIDTHS[(byte as usize) - 32] as f32,
        _ => 556.0,
    };
    // Digits, £ and punctuation used in amounts share widths across both
    // fonts; for letters bold is ~5% wider, which is close enough for wrapping.
    match font {
        Font::Bold if c.is_alphabetic() => base * 1.06,
        _ => base,
    }
}

pub fn text_width(text: &str, font: Font, size: f32) -> f32 {
    text.chars().map(|c| char_width(c, font)).sum::<f32>() * size / 1000.0
}

/// Greedy word wrap. Honours `\n`; breaks words longer than a line.
pub fn wrap(text: &str, font: Font, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if text_width(&candidate, font, size) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // Word alone may still be too wide: hard-break it.
            for c in word.chars() {
                let mut next = line.clone();
                next.push(c);
                if !line.is_empty() && text_width(&next, font, size) > max_width {
                    lines.push(std::mem::take(&mut line));
                    line.push(c);
                } else {
                    line = next;
                }
            }
        }
        lines.push(line);
    }
    lines
}

/// Truncate to one line that fits, adding an ellipsis if anything was cut.
pub fn fit_line(text: &str, font: Font, size: f32, max_width: f32) -> String {
    if text_width(text, font, size) <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        let mut next = out.clone();
        next.push(c);
        next.push('…');
        if text_width(&next, font, size) > max_width {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// A JPEG to embed. Only the header is parsed; the bytes go in untouched.
#[derive(Clone, Debug)]
pub struct Jpeg {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub components: u8,
}

impl Jpeg {
    /// Read dimensions from the first SOF marker. Rejects non-JPEG data and
    /// CMYK images (which need per-encoder colour handling).
    pub fn parse(data: Vec<u8>) -> Result<Self, String> {
        if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
            return Err("not a JPEG image".into());
        }
        let mut i = 2;
        while i + 3 < data.len() {
            if data[i] != 0xFF {
                return Err("corrupt JPEG header".into());
            }
            let marker = data[i + 1];
            if marker == 0xFF {
                i += 1; // fill byte
                continue;
            }
            if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
                i += 2; // standalone markers
                continue;
            }
            let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if len < 2 || i + 2 + len > data.len() {
                return Err("corrupt JPEG header".into());
            }
            let is_sof = (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker);
            if is_sof {
                if len < 8 {
                    return Err("corrupt JPEG header".into());
                }
                let seg = &data[i + 4..];
                let height = u16::from_be_bytes([seg[1], seg[2]]) as u32;
                let width = u16::from_be_bytes([seg[3], seg[4]]) as u32;
                let components = seg[5];
                if width == 0 || height == 0 {
                    return Err("JPEG has no dimensions".into());
                }
                if components != 1 && components != 3 {
                    return Err("only greyscale or RGB JPEGs are supported".into());
                }
                return Ok(Jpeg {
                    data,
                    width,
                    height,
                    components,
                });
            }
            if marker == 0xDA {
                break; // start of scan before any SOF
            }
            i += 2 + len;
        }
        Err("JPEG has no frame header".into())
    }
}

/// A clickable URL region on a page.
struct Link {
    rect: [f32; 4],
    url: String,
}

/// Drawing operations for one page, in PDF user space (origin bottom-left).
#[derive(Default)]
pub struct Page {
    ops: String,
    links: Vec<Link>,
}

impl Page {
    pub fn text(&mut self, x: f32, y: f32, font: Font, size: f32, gray: f32, text: &str) {
        let hex: String = text
            .chars()
            .map(|c| format!("{:02X}", win_ansi(c)))
            .collect();
        self.ops.push_str(&format!(
            "BT {gray:.2} g /{} {size:.1} Tf {x:.2} {y:.2} Td <{hex}> Tj ET\n",
            font.resource()
        ));
    }

    pub fn text_right(
        &mut self,
        right_x: f32,
        y: f32,
        font: Font,
        size: f32,
        gray: f32,
        text: &str,
    ) {
        let x = right_x - text_width(text, font, size);
        self.text(x, y, font, size, gray, text);
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, gray: f32) {
        self.ops.push_str(&format!(
            "{gray:.2} G {width:.2} w {x1:.2} {y1:.2} m {x2:.2} {y2:.2} l S\n"
        ));
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, gray: f32) {
        self.ops.push_str(&format!(
            "{gray:.2} g {x:.2} {y:.2} {w:.2} {h:.2} re f 0 g\n"
        ));
    }

    /// Draw image number `index` (from the list passed to [`write_pdf`]).
    pub fn image(&mut self, index: usize, x: f32, y: f32, w: f32, h: f32) {
        self.ops.push_str(&format!(
            "q {w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /Im{index} Do Q\n"
        ));
    }

    /// Make a rectangle clickable. `url` must be printable ASCII.
    pub fn link(&mut self, x: f32, y: f32, w: f32, h: f32, url: &str) {
        self.links.push(Link {
            rect: [x, y, x + w, y + h],
            url: url.to_string(),
        });
    }
}

fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars().filter(|c| c.is_ascii() && !c.is_ascii_control()) {
        if matches!(c, '(' | ')' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push(')');
    out
}

/// Serialise pages (and any images they reference) into a complete PDF file.
pub fn write_pdf(pages: &[Page], images: &[Jpeg], paper: Paper) -> Vec<u8> {
    let (page_w, page_h) = paper.size();
    // Assign object numbers up front so pages can reference images/annots.
    let first_image = 5;
    let mut next = first_image + images.len();
    let mut page_ids = Vec::with_capacity(pages.len());
    for page in pages {
        let page_id = next;
        let content_id = next + 1;
        let annot_ids: Vec<usize> = (0..page.links.len()).map(|k| next + 2 + k).collect();
        next += 2 + page.links.len();
        page_ids.push((page_id, content_id, annot_ids));
    }

    let mut objects: Vec<Vec<u8>> = Vec::with_capacity(next - 1);
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = page_ids
        .iter()
        .map(|(id, _, _)| format!("{id} 0 R"))
        .collect();
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    for img in images {
        let space = if img.components == 1 {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        let mut obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{space} \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            img.width,
            img.height,
            img.data.len()
        )
        .into_bytes();
        obj.extend_from_slice(&img.data);
        obj.extend_from_slice(b"\nendstream");
        objects.push(obj);
    }
    let xobjects: String = (0..images.len())
        .map(|k| format!("/Im{k} {} 0 R", first_image + k))
        .collect::<Vec<_>>()
        .join(" ");

    for (page, (_, content_id, annot_ids)) in pages.iter().zip(&page_ids) {
        let annots = if annot_ids.is_empty() {
            String::new()
        } else {
            let refs: Vec<String> = annot_ids.iter().map(|id| format!("{id} 0 R")).collect();
            format!(" /Annots [{}]", refs.join(" "))
        };
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w} {page_h}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << {xobjects} >> >> \
                 /Contents {content_id} 0 R{annots} >>"
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", page.ops.len()).into_bytes();
        stream.extend_from_slice(page.ops.as_bytes());
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
        for link in &page.links {
            let [x1, y1, x2, y2] = link.rect;
            objects.push(
                format!(
                    "<< /Type /Annot /Subtype /Link /Rect [{x1:.2} {y1:.2} {x2:.2} {y2:.2}] \
                     /Border [0 0 0] /A << /S /URI /URI {} >> >>",
                    pdf_string(&link.url)
                )
                .into_bytes(),
            );
        }
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Minimal valid-header JPEG for tests (SOI, SOF0 WxH RGB, SOS, EOI).
#[cfg(test)]
pub(crate) fn test_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&width.to_be_bytes());
    v.extend_from_slice(&[0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_pound_smart_quotes_and_folds_latin_ext_a() {
        assert_eq!(win_ansi('£'), 0xA3);
        assert_eq!(win_ansi('€'), 0x80);
        assert_eq!(win_ansi('’'), 0x92);
        assert_eq!(win_ansi('A'), b'A');
        assert_eq!(win_ansi('Š'), 0x8A);
        // Welsh and Polish letters fold to their base letter rather than '?'.
        let folded: String = "Tŷ Gwyn Ŵ Łódź Llŷr"
            .chars()
            .map(|c| win_ansi(c) as char)
            .collect();
        assert_eq!(folded, "Ty Gwyn W L\u{f3}dz Llyr");
        assert_eq!(win_ansi('ſ'), b's');
        assert_eq!(LATIN_EXT_A_FOLD.len(), 128);
        assert_eq!(win_ansi('漢'), b'?');
        assert_eq!(try_win_ansi('✓'), None);
    }

    #[test]
    fn wraps_long_text_and_long_words() {
        let lines = wrap("one two three four five six", Font::Regular, 10.0, 60.0);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(text_width(l, Font::Regular, 10.0) <= 60.0, "{l}");
        }
        let lines = wrap(&"x".repeat(200), Font::Regular, 10.0, 50.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), "x".repeat(200));
        assert_eq!(
            wrap("a\n\nb", Font::Regular, 10.0, 100.0),
            vec!["a", "", "b"]
        );
    }

    #[test]
    fn fit_line_truncates_with_ellipsis() {
        assert_eq!(fit_line("short", Font::Regular, 8.0, 200.0), "short");
        let cut = fit_line(&"word ".repeat(100), Font::Regular, 8.0, 100.0);
        assert!(cut.ends_with('…'));
        assert!(text_width(&cut, Font::Regular, 8.0) <= 100.0);
    }

    #[test]
    fn parses_jpeg_headers() {
        let j = Jpeg::parse(test_jpeg(640, 480)).unwrap();
        assert_eq!((j.width, j.height, j.components), (640, 480, 3));
        assert!(Jpeg::parse(b"\x89PNG\r\n\x1a\n".to_vec()).is_err());
        assert!(Jpeg::parse(vec![0xFF, 0xD8, 0xFF]).is_err());
        assert!(Jpeg::parse(vec![0xFF, 0xD8, 0xFF, 0xC0, 0xFF, 0xFF]).is_err());
        let mut cmyk = test_jpeg(10, 10);
        cmyk[17] = 4; // component count
        assert!(Jpeg::parse(cmyk).unwrap_err().contains("RGB"));
    }

    fn xref_entries(pdf: &[u8]) -> Vec<usize> {
        let marker = b"startxref\n";
        let pos = pdf
            .windows(marker.len())
            .rposition(|w| w == marker)
            .unwrap();
        let tail = std::str::from_utf8(&pdf[pos + marker.len()..]).unwrap();
        let startxref: usize = tail.lines().next().unwrap().parse().unwrap();
        let xref = std::str::from_utf8(&pdf[startxref..]).unwrap();
        assert!(xref.starts_with("xref"));
        xref.lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .map(|l| l[..10].parse().unwrap())
            .collect()
    }

    fn contains(pdf: &[u8], needle: &[u8]) -> bool {
        pdf.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let mut p = Page::default();
        p.text(50.0, 800.0, Font::Bold, 12.0, 0.0, "Total £1,234.56");
        let pdf = write_pdf(&[p, Page::default()], &[], Paper::A4);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let entries = xref_entries(&pdf);
        assert_eq!(entries.len(), 8); // catalog, pages, 2 fonts, 2 x (page + content)
        for (i, off) in entries.iter().enumerate() {
            assert!(pdf[*off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
        assert!(contains(&pdf, b"/Count 2"));
        let letter = write_pdf(&[Page::default()], &[], Paper::Letter);
        assert!(contains(&letter, b"/MediaBox [0 0 612 792]"));
    }

    #[test]
    fn embeds_images_and_links_with_valid_xref() {
        let img = Jpeg::parse(test_jpeg(100, 50)).unwrap();
        let mut p = Page::default();
        p.image(0, 50.0, 700.0, 100.0, 50.0);
        p.link(50.0, 30.0, 200.0, 10.0, "https://example.com/a(b)");
        let pdf = write_pdf(&[p, Page::default()], &[img], Paper::A4);
        let entries = xref_entries(&pdf);
        // catalog, pages, 2 fonts, 1 image, page1 + content + annot, page2 + content
        assert_eq!(entries.len(), 10);
        for (i, off) in entries.iter().enumerate() {
            assert!(pdf[*off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()));
        }
        assert!(contains(&pdf, b"/Filter /DCTDecode"));
        assert!(contains(&pdf, b"/Im0 5 0 R"));
        assert!(contains(&pdf, b"/Annots [8 0 R]"));
        assert!(contains(&pdf, b"/URI (https://example.com/a\\(b\\))"));
    }
}
