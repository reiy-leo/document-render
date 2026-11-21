//! 和 Windows Word 2024 对位置：代码块行高、段落边框的水平位置、页眉页脚的竖直位置。
//! 期望值的来历见 docs/STYLE.md；这里看 Typst 源码和 HTML 结构，不依赖本机装了哪些字体。

use document_render::render::{Format, pdf};

fn typst(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    let tree = d.tree().unwrap();
    pdf::markup(&d, &tree, &Default::default()).0
}

fn html(json: &str) -> String {
    let out = document_render::render_str(json, Format::Html, None).unwrap();
    String::from_utf8(out.bytes).unwrap()
}

/// 读 `top-edge: Aem + Bpt, bottom-edge: -Cem + -Dpt`，按字号 `fs` 算出行高（pt）。
fn edges_height(src: &str, fs: f32) -> f32 {
    let num = |s: &str, key: &str| -> (f32, f32) {
        let i = s.find(key).unwrap() + key.len();
        let rest = &s[i..];
        let em_end = rest.find("em").unwrap();
        let em: f32 = rest[..em_end].trim().parse().unwrap();
        let rest = &rest[em_end + 2..];
        let rest = rest.trim_start().trim_start_matches('+').trim_start();
        let pt_end = rest.find("pt").unwrap();
        let pt: f32 = rest[..pt_end].trim().parse().unwrap();
        (em, pt)
    };
    let (te, tp) = num(src, "top-edge:");
    let (be, bp) = num(src, "bottom-edge:");
    te * fs + tp - (be * fs + bp)
}

fn code_doc(code: &str, style: &str) -> String {
    format!(
        r#"{{ "root": "d", "elements": {{ "d": {{ "type": "Document", "children": ["c"] }},
            "c": {{ "type": "Code", "props": {{ "code": {} }}, "style": {style} }} }} }}"#,
        serde_json::to_string(code).unwrap()
    )
}

/// 代码块的行盒按代码自己的字号算：默认 9pt × 1.4 = 至少 12.6pt（Word 实测 12.6），
/// 以前按父级 10.5pt 的字体算成 12.98pt。
#[test]
fn code_line_height_uses_code_font_size() {
    let src = typst(&code_doc("a\nb\nc", "{}"));
    let rule = src
        .lines()
        .find(|l| l.starts_with("#show raw: set text("))
        .expect(&src);
    let h = edges_height(rule, 9.0);
    assert!((h - 12.6).abs() < 0.01, "代码行高 {h}，应为 12.6：{rule}");

    // 固定行高也按代码这一段算
    let src = typst(&code_doc("a\nb", r#"{ "lineHeight": "18pt" }"#));
    let rule = src
        .lines()
        .find(|l| l.starts_with("#show raw: set text("))
        .unwrap();
    assert!((edges_height(rule, 9.0) - 18.0).abs() < 0.01, "{rule}");
}

/// Word 逐行看有没有中文：只有带中文的行算上东亚字体。
#[test]
fn code_cjk_lines_get_their_own_line_box() {
    let json = code_doc("// 注释\nlet a = 1;\n// 又一行注释", "{}");
    let src = typst(&json);
    let rule = src
        .lines()
        .find(|l| l.starts_with("#show raw.line:"))
        .expect(&src);
    assert!(rule.contains("it.number in (1, 3,)"), "{rule}");
    let out = html(&json);
    let pre = &out[out.find("<pre id=\"c\"").unwrap()..];
    let pre = &pre[..pre.find("</pre>").unwrap()];
    assert!(pre.contains("line-height:1.4;"), "{pre}");
    assert_eq!(
        pre.matches("<span style=\"line-height:").count(),
        2,
        "{pre}"
    );
    assert!(pre.contains("\nlet a = 1;\n"), "{pre}");

    // 没有中文就不加
    let src = typst(&code_doc("let a = 1;", "{}"));
    assert!(!src.contains("#show raw.line:"), "{src}");
}

fn one(el: &str) -> String {
    format!(
        r#"{{ "root": "d", "elements": {{ "d": {{ "type": "Document", "children": ["p"] }}, "p": {el} }} }}"#
    )
}

/// 段落边框照 Word 画：文字留在缩进处，内边距（w:space）和线画在外面，左右再让出 1.5pt；
/// 线整条在框里（Typst 的线压在框边上，用负 outset 挪进去）。Word 2024 实测：1pt 线、4pt 内边距，
/// 左线外沿在文字左边 6.5pt 处（量到 6.36～6.44），上线里沿到文字行顶 4pt。
#[test]
fn paragraph_border_sits_outside_the_text_like_word() {
    let json = one(
        r##"{ "type": "Text", "props": { "text": "x" }, "style": { "border": "1pt solid #FF0000", "padding": 4 } }"##,
    );
    let src = typst(&json);
    assert!(src.contains("#pad(left: -6.5pt, right: -6.5pt)"), "{src}");
    assert!(
        src.contains("inset: (top: 5pt, right: 6.5pt, bottom: 5pt, left: 6.5pt), outset: (top: -0.5pt, right: -0.5pt, bottom: -0.5pt, left: -0.5pt)"),
        "{src}"
    );
    let out = html(&json);
    assert!(
        out.contains("margin-left:-6.5pt;margin-right:-6.5pt;"),
        "{out}"
    );
    assert!(
        out.contains("padding-top:4pt;padding-right:5.5pt;padding-bottom:4pt;padding-left:5.5pt;"),
        "{out}"
    );

    // 左缩进 36pt：线跟着缩进走
    let json = one(
        r##"{ "type": "Text", "props": { "text": "x" }, "style": { "border": "1pt solid #FF0000", "padding": 4, "marginLeft": 36 } }"##,
    );
    assert!(typst(&json).contains("#pad(left: 29.5pt, right: -6.5pt)"));

    // 只有左边框：其他三边 Word 不留内边距
    let json = one(
        r##"{ "type": "Text", "props": { "text": "x" }, "style": { "borderLeft": "3pt solid #0000FF", "padding": 4 } }"##,
    );
    let src = typst(&json);
    assert!(
        src.contains(
            "inset: (top: 0pt, right: 0pt, bottom: 0pt, left: 8.5pt), outset: (left: -1.5pt)"
        ),
        "{src}"
    );
}

/// Quote、Callout 在 docx 里把线宽和内边距加进了缩进（文字在 13pt 处），Word 的线外沿在缩进外 1.5pt。
#[test]
fn quote_text_and_line_match_word() {
    let json = one(r#"{ "type": "Quote", "props": { "text": "q" } }"#);
    let src = typst(&json);
    // 线外沿 −1.5，线 3pt，内边距 10 + 1.5：文字在 13pt
    assert!(src.contains("#pad(left: -1.5pt, right: 0pt)"), "{src}");
    assert!(
        src.contains("left: 14.5pt), outset: (left: -1.5pt)"),
        "{src}"
    );
    let out = html(&json);
    assert!(
        out.contains("margin-left:-1.5pt;") && out.contains("padding-left:11.5pt;"),
        "{out}"
    );
}

/// 合框的段落也一样：外框的缩进是线的外沿，内边距给各段。
#[test]
fn merged_paragraph_borders_use_word_position() {
    let p = r##"{ "type": "Text", "props": { "text": "x" }, "style": { "border": "1pt solid #FF0000", "padding": 4 } }"##;
    let json = format!(
        r#"{{ "root": "d", "elements": {{ "d": {{ "type": "Document", "children": ["a", "b"] }}, "a": {p}, "b": {p} }} }}"#
    );
    let src = typst(&json);
    assert_eq!(
        src.matches("#pad(left: -6.5pt, right: -6.5pt)").count(),
        1,
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 1pt, right: 1pt, bottom: 1pt, left: 1pt), outset: (top: -0.5pt"),
        "{src}"
    );
    // 各段左右内边距是 4 + 1.5；上下内边距只在框顶、框底（Text 段后 6pt 归上一段）
    assert!(
        src.contains("inset: (top: 4pt, right: 5.5pt, bottom: 6pt, left: 5.5pt)"),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 0pt, right: 5.5pt, bottom: 4pt, left: 5.5pt)"),
        "{src}"
    );
}
