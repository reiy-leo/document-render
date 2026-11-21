//! 列表项里的块（项对象的 `children`）：段落、表格、图片、代码排在项文字后面、子列表前面，
//! 不带编号，左边对齐到这一项的文字；check 的报错；docx 读回成 children。

use document_render::{Env, render::Format};

fn render(json: &str, f: Format) -> Vec<u8> {
    document_render::render_with(json, f, None, &Env::default())
        .unwrap_or_else(|e| panic!("{f:?}: {e:#}"))
        .bytes
}

fn html(json: &str) -> String {
    String::from_utf8(render(json, Format::Html)).unwrap()
}

fn markdown(json: &str) -> String {
    String::from_utf8(render(json, Format::Markdown)).unwrap()
}

fn typst(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    let tree = d.tree().unwrap();
    let (src, _, _) = document_render::render::pdf::markup(&d, &tree, &Default::default());
    src
}

fn unzip(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    s
}

/// 字号 10pt、listIndent 20pt：第一级文字在 20pt，第二级在 40pt。
const DOC: &str = r##"{
  "styles": { "List": { "fontSize": 10, "listIndent": 20, "listHanging": 15 } },
  "root": "doc",
  "elements": {
    "doc": { "type": "Document", "children": ["l", "after"] },
    "l": {
      "type": "List",
      "props": {
        "ordered": true,
        "items": [
          { "text": "第一项", "children": ["p1", "code"] },
          { "text": "第二项", "children": ["tbl"], "items": [{ "text": "子项", "children": ["p2"] }] },
          "第三项"
        ]
      }
    },
    "p1": { "type": "Text", "props": { "text": "第一项的第二段" } },
    "code": { "type": "Code", "props": { "code": "echo hi" } },
    "tbl": { "type": "Table", "props": { "columns": ["甲", "乙"], "rows": [["1", "2"]] } },
    "p2": { "type": "Text", "props": { "text": "子项的第二段" } },
    "after": { "type": "Text", "props": { "text": "列表后面" } }
  }
}"##;

#[test]
fn html_blocks_follow_item_text() {
    let h = html(DOC);
    let at = |t: &str| h.find(t).unwrap_or_else(|| panic!("缺 {t}：{h}"));
    // 顺序：项文字 → 续块 → 子列表 → 下一项
    assert!(at("第一项<") < at("第一项的第二段") && at("第一项的第二段") < at("echo hi"));
    assert!(at("echo hi") < at("第二项<") && at("第二项<") < at(">甲<"));
    assert!(at(">甲<") < at("子项<") && at("子项<") < at("子项的第二段"));
    assert!(at("子项的第二段") < at("第三项"));
    // 文字前面紧挨着的开始标签
    let tag = |t: &str| {
        let i = at(t);
        h[h[..i].rfind('<').unwrap()..i].to_string()
    };
    assert!(
        tag("第一项的第二段").contains("margin-left:20pt"),
        "{}",
        tag("第一项的第二段")
    );
    assert!(
        tag("子项的第二段").contains("margin-left:40pt"),
        "{}",
        tag("子项的第二段")
    );
    assert!(!tag("列表后面").contains("margin-left"));
    // 撑满的表格扣掉缩进
    assert!(
        h.contains("margin-left:20pt") && h.contains("width:calc(100% - 20pt)"),
        "{h}"
    );
}

#[test]
fn pdf_blocks_are_padded() {
    let src = typst(DOC);
    assert!(src.contains("pad(left: 20pt"), "{src}");
    assert!(src.contains("pad(left: 40pt"), "{src}");
    assert!(!render(DOC, Format::Pdf).is_empty());
}

#[test]
fn markdown_continuation_blocks() {
    let md = markdown(DOC);
    let want = "1. 第一项\n\n   第一项的第二段\n\n   ```\n   echo hi\n   ```\n2. 第二项\n\n   | 甲 | 乙 |\n   | --- | --- |\n   | 1 | 2 |\n\n   1. 子项\n\n      子项的第二段\n3. 第三项";
    assert!(md.contains(want), "{md}");
}

/// docx：续块是不带编号的段落，w:ind left 等于这一级文字的位置；表格写 w:tblInd，宽度扣掉缩进。
#[test]
fn docx_blocks_align_with_item_text() {
    let d = render(DOC, Format::Docx);
    let doc = unzip(&d, "word/document.xml");
    let para = |t: &str| {
        let i = doc
            .find(&format!(">{t}<"))
            .unwrap_or_else(|| panic!("缺 {t}"));
        let s = doc[..i].rfind("<w:p ").unwrap();
        doc[s..i].to_string()
    };
    for (t, left) in [
        ("第一项的第二段", 400),
        ("echo hi", 400),
        ("子项的第二段", 800),
    ] {
        let p = para(t);
        assert!(!p.contains("numPr"), "{t}：{p}");
        assert!(p.contains(&format!("<w:ind w:left=\"{left}\"")), "{t}：{p}");
        assert!(
            !p.contains("w:hanging") && !p.contains("w:firstLine"),
            "{t}：{p}"
        );
    }
    assert!(!para("列表后面").contains("w:ind"));
    // 项本身照常带编号
    assert!(para("第一项").contains("numPr"));
    assert!(
        doc.contains("<w:tblInd w:w=\"400\" w:type=\"dxa\" />"),
        "{doc}"
    );
    // A4、20mm 页边距：版心 481.9pt，减 20pt 缩进
    let w = doc.split("<w:tblW w:w=\"").nth(1).unwrap();
    let w: i32 = w[..w.find('"').unwrap()].parse().unwrap();
    assert!((w - 9238).abs() <= 2, "{w}");
}

fn check_err(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    format!("{:#}", d.tree().expect_err("应该报错"))
}

fn with_children(kids: &str, extra: &str) -> String {
    format!(
        r##"{{ "root": "doc", "elements": {{
          "doc": {{ "type": "Document", "children": ["l"{extra}] }},
          "l": {{ "type": "List", "props": {{ "items": ["一", {{ "text": "二", "items": [{{ "text": "二甲", "children": {kids} }}] }}] }} }},
          "p": {{ "type": "Text", "props": {{ "text": "段" }} }},
          "h": {{ "type": "Heading", "props": {{ "level": 2, "text": "标题" }} }},
          "l2": {{ "type": "List", "props": {{ "items": ["x"] }} }}
        }} }}"##
    )
}

#[test]
fn check_errors() {
    let e = check_err(&with_children(r#"["nope"]"#, ""));
    assert!(
        e.contains("`l` 第 2.1 项") && e.contains("`nope` 不存在"),
        "{e}"
    );
    let e = check_err(&with_children(r#"["h"]"#, ""));
    assert!(e.contains("`h` 是 Heading") && e.contains("items"), "{e}");
    let e = check_err(&with_children(r#"["l2"]"#, ""));
    assert!(e.contains("`l2` 是 List"), "{e}");
    let e = check_err(&with_children(r#"["p"]"#, r#", "p""#));
    assert!(e.contains("不止一次"), "{e}");
    let e = check_err(&with_children(r#"["p", "p"]"#, ""));
    assert!(e.contains("不止一次"), "{e}");
    // 合法的：item 只写 children 不写 text 也行
    let ok = r##"{ "root": "doc", "elements": {
      "doc": { "type": "Document", "children": ["l"] },
      "l": { "type": "List", "props": { "items": [{ "children": ["p"] }] } },
      "p": { "type": "Text", "props": { "text": "只有续块" } }
    } }"##;
    assert!(html(ok).contains("只有续块"));
}

/// 读回：编号段落后面对齐到项文字、没有编号的段落和表格变回这一项的 children，
/// 元素上不留 marginLeft；不对齐的照旧是列表后面的普通段落。
#[test]
fn parse_reads_item_children() {
    let bytes = render(DOC, Format::Docx);
    let doc = document_render::parse::docx(
        &bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap()
    .json;
    let els = &doc["elements"];
    let lists: Vec<&serde_json::Value> = els
        .as_object()
        .unwrap()
        .values()
        .filter(|e| e["type"] == "List")
        .collect();
    assert_eq!(lists.len(), 1, "{doc}");
    let items = &lists[0]["props"]["items"];
    let kids = |v: &serde_json::Value| -> Vec<String> {
        v["children"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|id| {
                        els[id.as_str().unwrap()]["type"]
                            .as_str()
                            .unwrap()
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(kids(&items[0]), ["Text", "Text"], "{items}");
    assert_eq!(kids(&items[1]), ["Table"], "{items}");
    assert_eq!(kids(&items[1]["items"][0]), ["Text"], "{items}");
    assert!(items[2].is_string(), "{items}");
    for v in items[0]["children"].as_array().unwrap() {
        let e = &els[v.as_str().unwrap()];
        assert!(e["style"]["marginLeft"].is_null(), "{e}");
    }
    let tbl = &els[items[1]["children"][0].as_str().unwrap()];
    assert!(tbl["style"]["marginLeft"].is_null(), "{tbl}");
    // 「列表后面」还是 Document 下的段落
    let root = &els[doc["root"].as_str().unwrap()]["children"];
    assert_eq!(root.as_array().unwrap().len(), 2, "{doc}");
    // 再渲染：续块位置一样
    let again = serde_json::to_string(&doc).unwrap();
    let d2 = unzip(&render(&again, Format::Docx), "word/document.xml");
    assert!(
        d2.contains("<w:ind w:left=\"800\"") && d2.contains("<w:tblInd w:w=\"400\""),
        "{d2}"
    );
}

/// 和列表文字不对齐的缩进段落不算续块。
#[test]
fn parse_keeps_unaligned_paragraphs_out() {
    let json = r##"{
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["l", "p"] },
        "l": { "type": "List", "props": { "items": ["一", "二"] } },
        "p": { "type": "Text", "props": { "text": "缩进不一样" }, "style": { "marginLeft": 30 } }
      }
    }"##;
    let bytes = render(json, Format::Docx);
    let doc = document_render::parse::docx(
        &bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap()
    .json;
    let root = &doc["elements"][doc["root"].as_str().unwrap()]["children"];
    assert_eq!(root.as_array().unwrap().len(), 2, "{doc}");
}

/// examples/list-blocks.json（Windows Word 对照用的样例）。
#[test]
fn list_blocks_example() {
    let json = include_str!("../examples/list-blocks.json");
    let h = html(json);
    assert!(h.contains("这是第一项的第二段") && h.contains("<table"));
    assert!(!render(json, Format::Pdf).is_empty());
    assert!(!render(json, Format::Docx).is_empty());
    assert!(markdown(json).contains("\n   这是第一项的第二段"));
}
