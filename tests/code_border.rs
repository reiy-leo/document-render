//! Code 的上下边框照 Windows Word 2024（2026-11-22 实测）：Word 里 Code 是一个段落，
//! 每行是段内换行，上下边框的线宽算进段落高度，行高按代码自己的字体字号算。
//!
//! pdf 看 Typst 源码：线宽一半加进里面块的内边距，另一半放在外层块上；
//! 行盒按代码的字体字号写（9pt，不是父级的 10.5pt）。html 的 pre 没定高度，边框本来就占高度。

use document_render::render::{Format, pdf};

fn doc(children: &[(&str, &str)]) -> String {
    let ids: Vec<String> = children.iter().map(|(id, _)| format!("\"{id}\"")).collect();
    let els: Vec<String> = children
        .iter()
        .map(|(id, body)| format!("\"{id}\": {body}"))
        .collect();
    format!(
        r#"{{ "root": "d", "styles": {{ "Code": {{ "fontFamily": "Consolas" }} }}, "elements": {{ "d": {{ "type": "Document", "children": [{}] }}, {} }} }}"#,
        ids.join(", "),
        els.join(", ")
    )
}

fn typst(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    let tree = d.tree().unwrap();
    pdf::markup(&d, &tree, &Default::default()).0
}

fn html(json: &str) -> String {
    let out = document_render::render_str(json, Format::Html, None).unwrap();
    String::from_utf8(out.bytes).unwrap()
}

const THICK: &str = r##"{ "type": "Code", "props": { "code": "a\nb" }, "style": { "borderTop": "3pt solid #000000", "borderBottom": "3pt solid #000000", "padding": 4 } }"##;
const THIN_NO_PAD: &str = r##"{ "type": "Code", "props": { "code": "a" }, "style": { "borderTop": "0.5pt solid #000000", "borderBottom": "1.5pt solid #000000", "padding": 0 } }"##;

#[test]
fn pdf_code_border_width_counts_toward_height() {
    let src = typst(&doc(&[("c", THICK)]));
    // 3pt 线：外层上下各 1.5pt，里面 4pt 内边距 + 1.5pt
    assert!(
        src.contains("#block(below: 8pt, inset: (top: 1.5pt, bottom: 1.5pt), width: 100%)[#block(fill: rgb(\"#F6F8FA\"), stroke: (top: (paint: rgb(\"#000000\"), thickness: 3pt), bottom: (paint: rgb(\"#000000\"), thickness: 3pt)), inset: (top: 5.5pt, right: 4pt, bottom: 5.5pt, left: 4pt)"),
        "{src}"
    );
}

#[test]
fn pdf_code_border_without_padding() {
    let src = typst(&doc(&[("c", THIN_NO_PAD)]));
    // 上 0.5pt、下 1.5pt，没有内边距。Word 画成 0.48pt、1.44pt（往下取整到 1/600 英寸），线贴着文字一侧：
    // 里面各留画出来的一半，外层补足标称粗细
    assert!(
        src.contains("inset: (top: 0.26pt, bottom: 0.78pt), width: 100%)[#block("),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 0.24pt, right: 0pt, bottom: 0.72pt, left: 0pt)"),
        "{src}"
    );
}

#[test]
fn pdf_code_line_box_uses_code_font_size() {
    let src = typst(&doc(&[("c", THICK)]));
    // 外层块设成代码的字体字号，行盒跟着按 9pt 的 Consolas 算
    let i = src.find("#raw(").unwrap();
    let head = &src[..i];
    let set = head.rfind("#set text(font: (\"Consolas\"").unwrap();
    assert!(head[set..].contains("size: 9pt)"), "{src}");
    assert!(head[set..].contains("top-edge: 0.9199em"), "{src}");
}

#[test]
fn pdf_code_merged_with_text_counts_border_once() {
    let st = r##""style": { "borderTop": "3pt solid #000000", "borderBottom": "3pt solid #000000", "padding": 8 }"##;
    let t = format!(r#"{{ "type": "Text", "props": {{ "text": "x" }}, {st} }}"#);
    let c = format!(r#"{{ "type": "Code", "props": {{ "code": "y" }}, {st} }}"#);
    let src = typst(&doc(&[("t", &t), ("c", &c)]));
    // 合成一个框：只有一圈线，线宽照样一半在里一半在外
    assert_eq!(src.matches("thickness: 3pt").count(), 2, "{src}");
    assert!(
        src.contains("inset: (top: 1.5pt, bottom: 1.5pt), width: 100%)[#block(stroke: (top: (paint: rgb(\"#000000\"), thickness: 3pt), bottom: (paint: rgb(\"#000000\"), thickness: 3pt)), inset: (top: 1.5pt, bottom: 1.5pt)"),
        "{src}"
    );
}

#[test]
fn html_pre_keeps_border_outside_content() {
    let h = html(&doc(&[("c", THICK)]));
    // pre 不定高度：边框和内边距都在内容外面，跟 Word 一样占高度
    let i = h.find("<pre id=\"c\"").unwrap();
    let tag = &h[i..i + h[i..].find('>').unwrap()];
    assert!(tag.contains("border-top:3pt solid #000000"), "{tag}");
    assert!(
        !tag.contains(";height:") && !tag.contains("\"height:"),
        "{tag}"
    );
}
