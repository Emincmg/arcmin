use arcweave_rust::Content;

/// Strips Arcweave's simple HTML wrapping (titles, labels) down to plain text.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .trim()
        .to_owned()
}

/// Flattens a rendered [Content] tree into a single display string.
pub fn flatten(content: &Content) -> String {
    let mut out = String::new();
    flatten_into(content, &mut out, false);
    out.trim().to_owned()
}

fn flatten_into(content: &Content, out: &mut String, quoted: bool) {
    match content {
        Content::Paragraph(s) => {
            push_block(out, &strip_html(s), quoted);
        }
        Content::Inline(s) => {
            if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(&strip_html(s));
        }
        Content::Block(items) => {
            for item in items {
                flatten_into(item, out, quoted);
            }
        }
        Content::Quote(items) => {
            for item in items {
                flatten_into(item, out, true);
            }
        }
    }
}

fn push_block(out: &mut String, text: &str, quoted: bool) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    if quoted {
        for line in text.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push_str(text);
    }
}

// ---------------------------------------------------------------------------
// Editor text <-> Arcweave HTML
//
// Element content and connection labels are stored as Arcweave HTML. The editor
// shows them as plain text with a tiny markup so nothing is lost when editing:
//   * blank line          -> new paragraph (`<p>`), single newline -> `<br>`
//   * `*italic*`, `**bold**` -> `<em>`, `<strong>`
//   * a line starting with `$ ` is an Arcscript statement (`<pre><code>`),
//     e.g. `$ hp += 10`, `$ if hp < 40`, `$ else`, `$ endif`
//   * a line starting with `!html ` is raw HTML the editor doesn't understand
//     (kept byte for byte)
// ---------------------------------------------------------------------------

enum Block<'a> {
    Para(&'a str),
    Code(&'a str),
    Raw(&'a str),
}

fn split_blocks(html: &str) -> Vec<Block<'_>> {
    const CODE_END: &str = "</code></pre>";
    let mut out = Vec::new();
    let mut rest = html.trim();
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix("<p>") {
            if let Some(end) = inner.find("</p>") {
                out.push(Block::Para(&inner[..end]));
                rest = inner[end + 4..].trim_start();
                continue;
            }
        }
        if rest.starts_with("<pre") {
            let code = rest
                .find('>')
                .map(|i| &rest[i + 1..])
                .filter(|after| after.starts_with("<code"))
                .and_then(|after| after.find('>').map(|i| &after[i + 1..]))
                .and_then(|body| body.find(CODE_END).map(|end| (body, end)));
            if let Some((body, end)) = code {
                out.push(Block::Code(&body[..end]));
                rest = body[end + CODE_END.len()..].trim_start();
                continue;
            }
        }
        let next = [rest[1..].find("<p>"), rest[1..].find("<pre")]
            .into_iter()
            .flatten()
            .min()
            .map_or(rest.len(), |i| i + 1);
        out.push(Block::Raw(rest[..next].trim()));
        rest = rest[next..].trim_start();
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn encode_entities(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Paragraph inner HTML -> editor text. `None` if it holds tags we can't represent.
fn inline_to_text(inner: &str) -> Option<String> {
    // Whitespace right after an opening marker (`<em> x`) is moved in front of it,
    // because `* x*` wouldn't read back as emphasis.
    fn push_text(out: &mut String, open_at: &mut Option<usize>, piece: &str) {
        let piece = escape_marks(&decode_entities(piece));
        if let Some(pos) = open_at.take() {
            let rest = piece.trim_start();
            out.insert_str(pos, &piece[..piece.len() - rest.len()]);
            out.push_str(rest);
        } else {
            out.push_str(&piece);
        }
    }

    let mut out = String::new();
    let mut open_at: Option<usize> = None;
    let mut rest = inner;
    while let Some(lt) = rest.find('<') {
        push_text(&mut out, &mut open_at, &rest[..lt]);
        let gt = rest[lt..].find('>')? + lt;
        let tag = rest[lt + 1..gt].trim().trim_end_matches('/').trim();
        match tag {
            "br" => out.push('\n'),
            "em" | "i" => {
                open_at = Some(out.len());
                out.push('*');
            }
            "strong" | "b" => {
                open_at = Some(out.len());
                out.push_str("**");
            }
            "/em" | "/i" => out.push('*'),
            "/strong" | "/b" => out.push_str("**"),
            _ => return None,
        }
        rest = &rest[gt + 1..];
    }
    push_text(&mut out, &mut open_at, rest);
    Some(out)
}

fn escape_marks(s: &str) -> String {
    s.replace('\\', "\\\\").replace('*', "\\*")
}

pub fn html_to_editor(html: &str) -> String {
    let mut out = String::new();
    let mut prev_was_para = false;
    for block in split_blocks(html) {
        let (text, is_para) = match block {
            Block::Para(inner) => match inline_to_text(inner) {
                Some(t) => (t, true),
                None => (format!("!html <p>{inner}</p>"), false),
            },
            Block::Code(inner) if !inner.contains('<') => {
                let code = decode_entities(inner);
                let lines: Vec<String> = code.lines().map(|l| format!("$ {}", l.trim())).collect();
                (lines.join("\n"), false)
            }
            Block::Code(inner) => (format!("!html <pre><code>{inner}</code></pre>"), false),
            Block::Raw(raw) if !raw.contains('<') => (escape_marks(&decode_entities(raw)), true),
            Block::Raw(raw) => (format!("!html {}", raw.replace('\n', " ")), false),
        };
        if !out.is_empty() {
            out.push_str(if prev_was_para && is_para { "\n\n" } else { "\n" });
        }
        out.push_str(&text);
        prev_was_para = is_para;
    }
    out
}

/// Turns `*italic*` / `**bold**` (and `\*` escapes) into tags, escaping everything else.
fn marks_to_html(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let (mut em, mut strong) = (false, false);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() && matches!(chars[i + 1], '*' | '\\') => {
                out.push_str(&encode_entities(&chars[i + 1].to_string()));
                i += 2;
            }
            '*' => {
                let run = if chars.get(i + 1) == Some(&'*') { 2 } else { 1 };
                let next_ok = chars.get(i + run).is_some_and(|c| !c.is_whitespace());
                let flag = if run == 2 { &mut strong } else { &mut em };
                let (open, close) = if run == 2 { ("<strong>", "</strong>") } else { ("<em>", "</em>") };
                if *flag {
                    *flag = false;
                    out.push_str(close);
                } else if !*flag && next_ok && has_closer(&chars, i + run, run) {
                    *flag = true;
                    out.push_str(open);
                } else {
                    out.push_str(&"*".repeat(run));
                }
                i += run;
            }
            c => {
                out.push_str(&encode_entities(&c.to_string()));
                i += 1;
            }
        }
    }
    if em {
        out.push_str("</em>");
    }
    if strong {
        out.push_str("</strong>");
    }
    out
}

/// Is there a matching closing run of `*` after `from`?
fn has_closer(chars: &[char], from: usize, run: usize) -> bool {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == '\\' {
            j += 2;
            continue;
        }
        if chars[j] == '*' {
            let this_run = if chars.get(j + 1) == Some(&'*') { 2 } else { 1 };
            if this_run == run && j > from {
                return true;
            }
            j += this_run;
            continue;
        }
        j += 1;
    }
    false
}

pub fn editor_to_html(text: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = Vec::new();
    let flush = |out: &mut String, para: &mut Vec<String>| {
        if !para.is_empty() {
            out.push_str(&format!("<p>{}</p>", para.join("<br>")));
            para.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(raw) = line.strip_prefix("!html ") {
            flush(&mut out, &mut para);
            out.push_str(raw);
        } else if let Some(code) = trimmed.strip_prefix("$ ") {
            flush(&mut out, &mut para);
            out.push_str(&format!("<pre><code>{}</code></pre>", encode_entities(code.trim())));
        } else if trimmed.is_empty() {
            flush(&mut out, &mut para);
        } else {
            para.push(marks_to_html(trimmed));
        }
    }
    flush(&mut out, &mut para);
    if out.is_empty() {
        "<p></p>".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod editor_text_tests {
    use super::*;

    fn rt(html: &str) -> String {
        editor_to_html(&html_to_editor(html))
    }

    #[test]
    fn plain_paragraphs_and_breaks_round_trip() {
        let html = "<p>One<br>two</p><p>Three</p>";
        assert_eq!(html_to_editor(html), "One\ntwo\n\nThree");
        assert_eq!(rt(html), html);
    }

    #[test]
    fn italics_and_bold_survive_editing() {
        let html = "<p>He said <em>no</em> and <strong>left</strong>.</p>";
        assert_eq!(html_to_editor(html), "He said *no* and **left**.");
        assert_eq!(rt(html), html);
    }

    #[test]
    fn literal_asterisks_and_entities_are_not_mistaken_for_markup() {
        assert_eq!(rt("<p>5 * 3 = 15 &amp; more</p>"), "<p>5 * 3 = 15 &amp; more</p>");
        assert_eq!(rt("<p>a &lt; b</p>"), "<p>a &lt; b</p>");
        assert_eq!(rt("<p>2*3 and 4*5</p>"), "<p>2*3 and 4*5</p>");
    }

    #[test]
    fn script_blocks_become_dollar_lines_and_back() {
        let html = "<p>Hi</p><pre><code>hp += 10</code></pre><pre><code>if hp &lt; 40</code></pre><p>weak</p><pre><code>endif</code></pre>";
        let text = html_to_editor(html);
        assert_eq!(text, "Hi\n$ hp += 10\n$ if hp < 40\nweak\n$ endif");
        assert_eq!(rt(html), html);
    }

    #[test]
    fn unknown_markup_is_kept_verbatim() {
        let html = "<p>See <a href=\"x\">this</a></p>";
        assert!(html_to_editor(html).starts_with("!html "));
        assert_eq!(rt(html), html);
        let mention = "<pre><code>visits(<span data-id=\"a\" data-type=\"element\">E</span>)</code></pre>";
        assert_eq!(rt(mention), mention);
    }

    #[test]
    fn empty_content() {
        assert_eq!(html_to_editor("<p></p>"), "");
        assert_eq!(editor_to_html(""), "<p></p>");
    }

    /// Set `ARCMIN_TEST_JSON`: every element/label of a real export must survive
    /// open-in-editor -> save unchanged.
    #[test]
    fn real_export_content_round_trips_exactly() {
        let Ok(path) = std::env::var("ARCMIN_TEST_JSON") else {
            return;
        };
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut checked = 0;
        for (section, field) in [("elements", "content"), ("connections", "label")] {
            for (id, item) in raw[section].as_object().unwrap() {
                let Some(html) = item.get(field).and_then(|v| v.as_str()) else {
                    continue;
                };
                // Invisible trailing noise (a trailing nbsp, an empty last paragraph)
                // is allowed to disappear when an item is edited; nothing else is.
                let norm = |s: &str| {
                    s.replace("<br/>", "<br>")
                        .replace("<br />", "<br>")
                        .replace("<p><br></p>", "")
                        .replace("<p></p>", "")
                        .replace('\u{a0}', " ")
                        .replace(" </p>", "</p>")
                };
                assert_eq!(norm(&rt(html)), norm(html), "{section}/{id}/{field}");
                checked += 1;
            }
        }
        assert!(checked > 20, "only checked {checked}");
    }
}

#[cfg(test)]
mod emphasis_whitespace_tests {
    use super::*;

    #[test]
    fn whitespace_inside_emphasis_round_trips() {
        for html in [
            "<p><em>trailing space </em></p>",
            "<p>a <em>b</em> c</p>",
            "<p><strong>bold </strong>rest</p>",
        ] {
            assert_eq!(editor_to_html(&html_to_editor(html)), html);
        }
        // Leading whitespace moves outside the marker but still renders the same text.
        assert_eq!(html_to_editor("<p>x<em> y</em></p>"), "x *y*");
    }
}
