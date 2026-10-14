//! A minimal PDF writer for text reports: A4 pages, Helvetica, word wrapping. It exists so a job
//! report can be saved as a real PDF without a browser or a PDF library. Text only.

const WIDTH: f32 = 595.0;
const HEIGHT: f32 = 842.0;
const MARGIN: f32 = 50.0;

/// A photo ready for a PDF: JPEG bytes and their size in pixels.
pub struct Image {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Image {
    /// Reads any picture the `image` crate knows and re-encodes it as a JPEG no bigger than 1200 px.
    pub fn from_file(path: &std::path::Path) -> Option<Image> {
        let img = image::open(path).ok()?.thumbnail(1200, 1200).to_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode_image(&img).ok()?;
        Some(Image { jpeg, width: img.width(), height: img.height() })
    }
}

pub struct Line {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    /// Extra space above the line, in points.
    pub gap: f32,
    /// Distance from the left margin, in points.
    pub indent: f32,
    /// Placed at the foot of the last page instead of straight after the previous line.
    pub pinned: bool,
    /// A picture drawn in place of the text, at most this many points tall (see [`Image`]).
    pub image: Option<(usize, f32)>,
}

impl Line {
    pub fn body(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 10.5, bold: false, gap: 6.0, indent: 0.0, pinned: false, image: None }
    }
    pub fn title(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 20.0, bold: true, gap: 0.0, indent: 0.0, pinned: false, image: None }
    }
    pub fn heading(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 13.0, bold: true, gap: 14.0, indent: 0.0, pinned: false, image: None }
    }
    pub fn muted(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 9.0, bold: false, gap: 6.0, indent: 0.0, pinned: false, image: None }
    }
    /// Small print under the line above it (a detail of one entry).
    pub fn detail(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 9.0, bold: false, gap: 6.0, indent: 14.0, pinned: false, image: None }
    }
    /// Picture number `n` of the list given to [`render`], drawn at most `height` points tall.
    pub fn picture(n: usize, height: f32) -> Self {
        Self { text: String::new(), size: 10.0, bold: false, gap: 8.0, indent: 0.0, pinned: false, image: Some((n, height)) }
    }
    /// The small print at the foot of the page.
    pub fn footer(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 8.5, bold: false, gap: 0.0, indent: 0.0, pinned: true, image: None }
    }
}

/// The byte a character becomes in WinAnsi, or `?`.
fn winansi(c: char) -> u8 {
    match c {
        '\u{2022}' | '\u{25cf}' => 0x95,
        '\u{2013}' => 0x96,
        '\u{2014}' => b'-',
        '\u{2018}' | '\u{2019}' => b'\'',
        '\u{201c}' | '\u{201d}' => b'"',
        '\u{2714}' | '\u{2713}' => b'+',
        '\u{2716}' => b'x',
        '\u{25cb}' => b'o',
        '\u{2192}' => b'>',
        c if (c as u32) < 0x100 && !c.is_control() => c as u32 as u8,
        _ => b'?',
    }
}

fn escape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        match winansi(c) {
            b @ (b'(' | b')' | b'\\') => out.extend([b'\\', b]),
            b if (32..127).contains(&b) => out.push(b),
            b => out.extend(format!("\\{b:03o}").bytes()),
        }
    }
    out
}

/// Breaks `text` into lines of at most `max` characters, at spaces where possible.
fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > max {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let head: String = word.chars().take(max).collect();
            word = word.chars().skip(max).collect();
            lines.push(head);
        }
        let need = if cur.is_empty() { 0 } else { 1 } + word.chars().count();
        if cur.chars().count() + need > max && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// One page: its drawing operators and which pictures it shows.
struct Page {
    content: Vec<u8>,
    images: Vec<usize>,
}

/// The page contents, with the pictures `images` sized to fit the lines that ask for them.
fn layout(lines: &[Line], images: &[Image]) -> Vec<Page> {
    let mut pages: Vec<Page> = vec![Page { content: Vec::new(), images: Vec::new() }];
    let mut y = HEIGHT - MARGIN;
    for line in lines {
        if let Some((n, max_h)) = line.image {
            let Some(img) = images.get(n) else { continue };
            let aspect = img.width as f32 / img.height.max(1) as f32;
            let h = max_h.min((WIDTH - 2.0 * MARGIN) / aspect);
            let w = h * aspect;
            if y - line.gap - h < MARGIN {
                pages.push(Page { content: Vec::new(), images: Vec::new() });
                y = HEIGHT - MARGIN;
            } else {
                y -= line.gap;
            }
            y -= h;
            let page = pages.last_mut().expect("at least one page");
            if !page.images.contains(&n) {
                page.images.push(n);
            }
            page.content.extend(format!("q {w:.1} 0 0 {h:.1} {:.1} {y:.1} cm /Im{n} Do Q\n", MARGIN + line.indent).bytes());
            continue;
        }
        let leading = line.size * 1.35;
        let max = ((WIDTH - 2.0 * MARGIN - line.indent) / (line.size * 0.5)) as usize;
        let wrapped = wrap(&line.text, max);
        if line.pinned {
            // the foot of the last page; a new page when what is left of this one is too short
            let total = wrapped.len() as f32 * leading;
            if y - total < MARGIN {
                pages.push(Page { content: Vec::new(), images: Vec::new() });
            }
            y = MARGIN + total;
        }
        for (i, text) in wrapped.into_iter().enumerate() {
            let gap = if i == 0 && !line.pinned { line.gap } else { 0.0 };
            if y - gap - leading < MARGIN {
                pages.push(Page { content: Vec::new(), images: Vec::new() });
                y = HEIGHT - MARGIN;
            } else {
                y -= gap;
            }
            y -= leading;
            let page = &mut pages.last_mut().expect("at least one page").content;
            page.extend(format!("BT /F{} {} Tf {} {:.1} Td (", if line.bold { 2 } else { 1 }, line.size, MARGIN + line.indent, y).bytes());
            page.extend(escape(&text));
            page.extend(b") Tj ET\n");
        }
    }
    pages
}

/// Like [`render`], with pictures that lines refer to by number (see [`Line::picture`]).
pub fn render_with(lines: &[Line], images: &[Image]) -> Vec<u8> {
    let pages = layout(lines, images);
    // objects: 1 catalog, 2 page tree, 3 and 4 fonts, then (page, contents) per page
    let first_page = 5usize;
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let object = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: Vec<u8>| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    object(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", first_page + 2 * i)).collect();
    object(&mut out, &mut offsets, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).into_bytes());
    for font in ["Helvetica", "Helvetica-Bold"] {
        object(&mut out, &mut offsets, format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding >>").into_bytes());
    }
    let first_image = first_page + 2 * pages.len();
    for (i, page) in pages.iter().enumerate() {
        let contents_id = first_page + 2 * i + 1;
        let xobjects: String = page.images.iter().map(|n| format!("/Im{n} {} 0 R ", first_image + n)).collect();
        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << {xobjects}>> >> /Contents {contents_id} 0 R >>"
            )
            .into_bytes(),
        );
        let mut body = format!("<< /Length {} >>\nstream\n", page.content.len()).into_bytes();
        body.extend(&page.content);
        body.extend(b"endstream");
        object(&mut out, &mut offsets, body);
    }
    for img in images {
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            img.width,
            img.height,
            img.jpeg.len()
        )
        .into_bytes();
        body.extend(&img.jpeg);
        body.extend(b"\nendstream");
        object(&mut out, &mut offsets, body);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).bytes());
    for o in &offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(b: &[u8]) -> String {
        b.iter().map(|&c| c as char).collect()
    }

    #[test]
    fn xref_points_at_the_table_and_objects() {
        let pdf = render_with(&[Line::title("Report"), Line::body("P0113 intake air temperature (circuit)")], &[]);
        let s = text(&pdf);
        assert!(s.starts_with("%PDF-1.4") && s.trim_end().ends_with("%%EOF"));
        let xref: usize = s.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(s[xref..].starts_with("xref"));
        // first object offset is where "1 0 obj" starts
        let first: usize = s[xref..].lines().nth(3).unwrap()[..10].parse().unwrap();
        assert!(s[first..].starts_with("1 0 obj"));
        assert!(s.contains("(P0113 intake air temperature \\(circuit\\)) Tj"));
    }

    #[test]
    fn accents_become_octal_and_unknown_characters_question_marks() {
        let s = text(&render_with(&[Line::body("Revisão ✔ 日本")], &[]));
        assert!(s.contains("Revis\\343o + ??"), "{s}");
    }

    #[test]
    fn long_text_wraps_and_many_lines_paginate() {
        assert_eq!(wrap("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        let lines: Vec<Line> = (0..200).map(|i| Line::body(format!("line {i}"))).collect();
        let s = text(&render_with(&lines, &[]));
        assert!(s.contains("/Count 6"), "{}", s.split("/Count").nth(1).unwrap_or("").chars().take(6).collect::<String>());
    }
}
