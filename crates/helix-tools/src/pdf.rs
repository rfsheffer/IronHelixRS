//! The page images of a scanned PDF (like the PC release's manual): each
//! page drawn by one JPEG image, which is taken out as it is.
//!
//! Only what such files need: objects found by scanning for `N 0 obj`
//! (no cross-reference table), the page tree from the catalog, and each
//! page's image XObject with the `DCTDecode` filter. Anything else (text,
//! several images on a page, other image formats) is reported, not drawn.

use anyhow::{Context, Result, bail};

pub struct Pdf<'a> {
    data: &'a [u8],
    /// Each object's body, from after `obj` to `endobj`.
    objects: std::collections::HashMap<u32, &'a [u8]>,
}

impl<'a> Pdf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if !data.starts_with(b"%PDF") {
            bail!("not a PDF");
        }
        let mut objects = std::collections::HashMap::new();
        let mut i = 0;
        while let Some(at) = find(&data[i..], b" 0 obj").map(|p| p + i) {
            // The object number before " 0 obj", at the start of a line or token.
            let start = data[..at].iter().rposition(|b| !b.is_ascii_digit()).map_or(0, |p| p + 1);
            let body_start = at + b" 0 obj".len();
            if let (Ok(n), Some(end)) = (std::str::from_utf8(&data[start..at]).unwrap_or("").parse::<u32>(), find(&data[body_start..], b"endobj")) {
                // Later definitions (incremental updates) win.
                objects.insert(n, &data[body_start..body_start + end]);
            }
            i = body_start;
        }
        Ok(Self { data, objects })
    }

    fn object(&self, n: u32) -> Result<&'a [u8]> {
        self.objects.get(&n).copied().with_context(|| format!("object {n} is missing"))
    }

    /// Each page's JPEG, in page order.
    pub fn page_jpegs(&self) -> Result<Vec<&'a [u8]>> {
        let trailer_root = find_last(self.data, b"/Root").map(|p| &self.data[p..]);
        let catalog = match trailer_root.and_then(|t| reference_after(t, b"/Root")) {
            Some(n) => self.object(n)?,
            None => self.objects.values().find(|o| find(o, b"/Type /Catalog").is_some() || find(o, b"/Type/Catalog").is_some()).copied().context("no catalog")?,
        };
        let pages = reference_after(catalog, b"/Pages").context("the catalog has no pages")?;
        let mut out = Vec::new();
        self.collect_pages(pages, &mut out, 0)?;
        Ok(out)
    }

    fn collect_pages(&self, n: u32, out: &mut Vec<&'a [u8]>, depth: usize) -> Result<()> {
        if depth > 32 {
            bail!("the page tree is too deep");
        }
        let node = self.object(n)?;
        if let Some(kids) = array_after(node, b"/Kids") {
            for kid in references(kids) {
                self.collect_pages(kid, out, depth + 1)?;
            }
            return Ok(());
        }
        out.push(self.page_image(node).with_context(|| format!("page {}", out.len() + 1))?);
        Ok(())
    }

    /// The one JPEG a page draws.
    fn page_image(&self, page: &'a [u8]) -> Result<&'a [u8]> {
        let resources = self.dict_after(page, b"/Resources").context("no resources")?;
        let xobjects = self.dict_after(resources, b"/XObject").context("no images (is the page text?)")?;
        let images: Vec<u32> = references(xobjects).collect();
        let [image] = images[..] else { bail!("{} images on the page; expected one", images.len()) };
        let obj = self.object(image)?;
        if find(obj, b"/DCTDecode").is_none() {
            bail!("the page image isn't a JPEG");
        }
        self.stream(obj)
    }

    /// A dictionary value: inline `<< ... >>` or a reference to one.
    fn dict_after(&self, obj: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
        let at = find(obj, key)? + key.len();
        let rest = trim_start(&obj[at..]);
        if rest.starts_with(b"<<") { Some(balanced(rest)) } else { self.object(reference(rest)?).ok() }
    }

    /// A stream's bytes (`/Length`, direct or a reference).
    fn stream(&self, obj: &'a [u8]) -> Result<&'a [u8]> {
        let start = find(obj, b"stream").context("no stream")? + b"stream".len();
        let start = start + if obj[start..].starts_with(b"\r\n") { 2 } else { 1 };
        let len_at = find(obj, b"/Length").context("no stream length")? + b"/Length".len();
        let len_text = trim_start(&obj[len_at..]);
        let len = match reference(len_text) {
            Some(n) => number(trim_start(self.object(n)?)),
            None => number(len_text),
        }
        .context("bad stream length")?;
        obj.get(start..start + len).context("the stream runs past its object")
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &s[n..]
}

fn number(s: &[u8]) -> Option<usize> {
    let digits = s.iter().take_while(|b| b.is_ascii_digit()).count();
    std::str::from_utf8(&s[..digits]).ok()?.parse().ok()
}

/// `N 0 R` at the start of `s`.
fn reference(s: &[u8]) -> Option<u32> {
    let head = String::from_utf8_lossy(&s[..s.len().min(32)]).into_owned();
    let mut words = head.split_ascii_whitespace();
    let n = words.next()?.parse().ok()?;
    let generation = words.next()?;
    let r = words.next()?;
    (generation.chars().all(|c| c.is_ascii_digit()) && r.starts_with('R')).then_some(n)
}

fn reference_after(obj: &[u8], key: &[u8]) -> Option<u32> {
    reference(trim_start(&obj[find(obj, key)? + key.len()..]))
}

/// The `[ ... ]` after a key.
fn array_after<'a>(obj: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let rest = trim_start(&obj[find(obj, key)? + key.len()..]);
    let end = rest.iter().position(|&b| b == b']')?;
    rest.starts_with(b"[").then(|| &rest[1..end])
}

/// A `<< ... >>` with what it nests.
fn balanced(s: &[u8]) -> &[u8] {
    let mut depth = 0;
    let mut i = 0;
    while i + 1 < s.len() {
        match &s[i..i + 2] {
            b"<<" => {
                depth += 1;
                i += 2;
            }
            b">>" => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return &s[..i];
                }
            }
            _ => i += 1,
        }
    }
    s
}

/// Every `N 0 R` in some text, in order.
fn references(s: &[u8]) -> impl Iterator<Item = u32> + '_ {
    let text = String::from_utf8_lossy(s).into_owned();
    let words: Vec<String> = text.split(|c: char| c.is_ascii_whitespace() || c == '/' || c == '<' || c == '>').filter(|w| !w.is_empty()).map(str::to_string).collect();
    let found: Vec<u32> = words
        .windows(3)
        .filter(|w| w[2] == "R" && w[1].chars().all(|c| c.is_ascii_digit()))
        .filter_map(|w| w[0].parse().ok())
        .collect();
    found.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_in_tree_order() {
        let pdf = b"%PDF-1.4
1 0 obj
<</Type /Page /Resources <</XObject 2 0 R /ProcSet [ /PDF ] >> /Parent 9 0 R>>
endobj
2 0 obj
<</R7 3 0 R>>
endobj
3 0 obj
<</Filter /DCTDecode /Subtype /Image /Length 4>>stream
JPG1
endstream
endobj
4 0 obj
<</Type /Page /Resources <</XObject <</Im0 5 0 R>> >> >>
endobj
5 0 obj
<</Filter /DCTDecode /Subtype /Image /Length 6 0 R>>stream
JPG22
endstream
endobj
6 0 obj
5
endobj
9 0 obj
<</Type /Pages /Count 2 /Kids [ 4 0 R
1 0 R
] >>
endobj
10 0 obj
<</Pages 9 0 R /Type /Catalog>>
endobj
trailer
<</Root 10 0 R>>
%%EOF
";
        let doc = Pdf::parse(pdf).unwrap();
        let pages = doc.page_jpegs().unwrap();
        assert_eq!(pages, [&b"JPG22"[..], &b"JPG1"[..]]);
    }

    #[test]
    fn text_pages_are_refused() {
        let pdf = b"%PDF-1.4
1 0 obj
<</Type /Page /Resources <</Font <</F1 2 0 R>> >> >>
endobj
3 0 obj
<</Type /Pages /Kids [ 1 0 R ] >>
endobj
4 0 obj
<</Type /Catalog /Pages 3 0 R>>
endobj
";
        assert!(Pdf::parse(pdf).unwrap().page_jpegs().is_err());
    }
}
