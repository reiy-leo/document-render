//! 一格里多行：Word 逐行按这一行用到的字体算行高（2026-11-22 Windows Word 2024 实测）。
//! 有中文的行算上东亚字体，纯西文的行只算西文字体。两种行都有时，pdf、html 每行开头
//! 放一个零宽字符，带上这一行的行高。

use document_render::render::{Format, pdf};

fn cell_doc(text: &str) -> String {
    format!(
        r#"{{
  "root": "d",
  "elements": {{
    "d": {{ "type": "Document", "children": ["t"] }},
    "t": {{ "type": "Table", "props": {{ "rows": [[{text}, "x"]] }} }}
  }}
}}"#
    )
}

fn typst(json: &str) -> String {
    let (d, tree) = document_render::prepare(json, &document_render::Env::default()).unwrap();
    pdf::markup(&d, &tree, &Default::default()).0
}

fn html(json: &str) -> String {
    String::from_utf8(
        document_render::render_str(json, Format::Html, None)
            .unwrap()
            .bytes,
    )
    .unwrap()
}

/// 撑高标记（pdf）：`#text(top-edge: …pt, bottom-edge: …pt)[#"\u{200B}"]`
fn pdf_marks(s: &str) -> usize {
    s.matches("pt)[#\"\u{200B}\"]").count()
}

fn html_marks(s: &str) -> usize {
    s.matches("pt\">\u{200B}</span>").count()
}

#[test]
fn mixed_cell_marks_every_line() {
    let json = cell_doc(r#""第一行\nsecond\n第三行""#);
    let t = typst(&json);
    assert_eq!(pdf_marks(&t), 3, "{t}");
    let h = html(&json);
    assert_eq!(html_marks(&h), 3, "{h}");
    // 第一、三行（有中文）的标记一样，第二行（纯西文）的矮一些
    let marks: Vec<&str> = t
        .split("#text(top-edge: ")
        .skip(1)
        .map(|m| &m[..m.find(')').unwrap()])
        .collect();
    assert_eq!(marks.len(), 3, "{t}");
    assert_eq!(marks[0], marks[2]);
    assert_ne!(marks[0], marks[1]);
}

#[test]
fn uniform_cells_have_no_marks() {
    for text in [
        r#""first\nsecond""#,
        r#""第一行\n第二行""#,
        r#""一行中文""#,
        r#""one line""#,
    ] {
        let json = cell_doc(text);
        assert_eq!(pdf_marks(&typst(&json)), 0, "{text}");
        assert_eq!(html_marks(&html(&json)), 0, "{text}");
    }
}

#[test]
fn lines_span_runs() {
    // 换行落在两个 run 中间：第二行的中文在后一个 run 里，三行各一个标记
    let json = cell_doc(r#"{ "text": [{ "text": "Latin\n", "bold": true }, "中文", "\nend"] }"#);
    let t = typst(&json);
    assert_eq!(pdf_marks(&t), 3, "{t}");
    let h = html(&json);
    assert_eq!(html_marks(&h), 3, "{h}");
}
