//! 标题自动编号：`styles` 里 `Heading.N` 上的列表字段打开编号，四种格式都写出算好的号，
//! docx 用 Word 原生的多级列表绑在标题样式上；`numbered: false`、分节重新编号、读回 JSON。

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

/// html 里每个标题编号 span 的文字。
fn html_numbers(s: &str) -> Vec<String> {
    s.split("class=\"document-render-heading-number\"")
        .skip(1)
        .map(|c| {
            let c = &c[c.find('>').unwrap() + 1..];
            c[..c.find("</span>").unwrap()].to_string()
        })
        .collect()
}

/// typst 源码里每个标题的编号（numbering 函数返回的文字）。
fn typst_numbers(src: &str) -> Vec<String> {
    src.split("numbering: (..n) => [")
        .skip(1)
        .map(|c| {
            let at = c.find("#\"").unwrap() + 2;
            c[at..at + c[at..].find('"').unwrap()].to_string()
        })
        .collect()
}

const DOC: &str = r##"{
  "styles": {
    "Heading.1": { "listStyleType": "cjk-decimal", "listMarkerFormat": "第%1章" },
    "Heading.2": { "listMarkerFormat": "%1.%2" }
  },
  "root": "doc",
  "elements": {
    "doc": { "type": "Document", "children": ["s1", "s2"] },
    "s1": { "type": "Section", "children": ["toc", "pre", "c1", "c1-1", "c1-2", "c1-2-1", "c2", "c2-1"] },
    "toc": { "type": "TableOfContents", "props": { "title": "目录" } },
    "pre": { "type": "Heading", "props": { "level": 1, "text": "前言", "numbered": false } },
    "c1": { "type": "Heading", "props": { "level": 1, "text": "总则" } },
    "c1-1": { "type": "Heading", "props": { "level": 2, "text": "范围" } },
    "c1-2": { "type": "Heading", "props": { "level": 2, "text": "术语" } },
    "c1-2-1": { "type": "Heading", "props": { "level": 3, "text": "三级不编号" } },
    "c2": { "type": "Heading", "props": { "level": 1, "text": "实施" } },
    "c2-1": { "type": "Heading", "props": { "level": 2, "text": "步骤" } },
    "s2": { "type": "Section", "props": { "restartHeadingNumbering": true }, "children": ["a1", "a1-1"] },
    "a1": { "type": "Heading", "props": { "level": 1, "text": "附录" } },
    "a1-1": { "type": "Heading", "props": { "level": 2, "text": "资料" } }
  }
}"##;

const WANT: [&str; 7] = ["第一章", "1.1", "1.2", "第二章", "2.1", "第一章", "1.1"];

#[test]
fn numbers_in_every_format() {
    let h = html(DOC);
    assert_eq!(html_numbers(&h), WANT, "{h}");
    // 目录也带编号；不编号的标题没有编号
    assert!(h.contains("<span>第一章 总则</span>"), "{h}");
    assert!(h.contains("<span>前言</span>"), "{h}");
    assert!(h.contains("<span>1.2 术语</span>"), "{h}");
    assert_eq!(typst_numbers(&typst(DOC)), WANT);
    let src = typst(DOC);
    assert!(src.contains("it.prefix()"), "目录要显示编号：{src}");
    let md = markdown(DOC);
    for line in [
        "# 前言",
        "# 第一章 总则",
        "## 1.2 术语",
        "### 三级不编号",
        "## 2.1 步骤",
        "# 第一章 附录",
        "- [第一章 总则](#c1)",
        "  - [1.2 术语](#c1-2)",
    ] {
        assert!(md.lines().any(|l| l == line), "缺 `{line}`：{md}");
    }
    assert!(!render(DOC, Format::Pdf).is_empty());
}

#[test]
fn docx_binds_numbering_to_heading_styles() {
    let d = render(DOC, Format::Docx);
    let num = unzip(&d, "word/numbering.xml");
    let styles = unzip(&d, "word/styles.xml");
    let doc = unzip(&d, "word/document.xml");
    // 一个 abstractNum，各级绑标题样式；第二级是阿拉伯数字，上级也写成数字（isLgl）
    assert!(num.contains("<w:pStyle w:val=\"Heading1\" />"), "{num}");
    assert!(num.contains("<w:pStyle w:val=\"Heading2\" />"), "{num}");
    assert!(
        !num.contains("<w:pStyle w:val=\"Heading3\" />"),
        "三级没开编号：{num}"
    );
    assert!(num.contains("<w:numFmt w:val=\"chineseCounting\" />"));
    assert!(num.contains("<w:lvlText w:val=\"第%1章\" />"));
    assert!(num.contains("<w:isLgl />"));
    assert!(num.contains("<w:suff w:val=\"space\" />"));
    // 标题样式里写 numPr
    let h1 = &styles[styles.find("w:styleId=\"Heading1\"").unwrap()..];
    let h1 = &h1[..h1.find("</w:style>").unwrap()];
    assert!(
        h1.contains("<w:numPr><w:numId w:val=\"2\" /><w:ilvl w:val=\"0\" /></w:numPr>"),
        "{h1}"
    );
    let h3 = &styles[styles.find("w:styleId=\"Heading3\"").unwrap()..];
    assert!(!h3[..h3.find("</w:style>").unwrap()].contains("numPr"));
    // 前言写 numId 0；第一节的编号标题段落不写 numPr，靠样式
    let paras: Vec<&str> = doc.split("<w:p ").skip(1).collect();
    // 目录里也有同名的项，只找标题段落
    let find = |t: &str| {
        *paras
            .iter()
            .find(|p| p.contains("w:val=\"Heading") && p.contains(&format!(">{t}<")))
            .unwrap()
    };
    assert!(find("前言").contains("<w:numId w:val=\"0\" />"));
    assert!(!find("总则").contains("numPr"));
    // 段落标记的 rPr 写在 pPr 最后（编号的字号跟它走）
    let p = find("总则");
    let ppr = &p[p.find("<w:pPr>").unwrap()..p.find("</w:pPr>").unwrap()];
    assert!(
        ppr.ends_with("</w:rPr>") && ppr.contains("<w:sz w:val=\"40\" />"),
        "{ppr}"
    );
    // 第二节重新编号：单开一个 num，各级 startOverride，标题直接引用它
    assert!(
        find("附录").contains("<w:numId w:val=\"3\" />"),
        "{}",
        find("附录")
    );
    assert!(find("资料").contains("<w:numId w:val=\"3\" /><w:ilvl w:val=\"1\" />"));
    let n3 = &num[num.find("<w:num w:numId=\"3\"").unwrap()..];
    let n3 = &n3[..n3.find("</w:num>").unwrap()];
    assert!(n3.contains("<w:abstractNumId w:val=\"2\" />"), "{n3}");
    assert_eq!(
        n3.matches("<w:startOverride w:val=\"1\" />").count(),
        2,
        "{n3}"
    );
    // 目录预先填的项带编号
    assert!(doc.contains(">第一章 总则<"), "{doc}");
}

#[test]
fn default_format_and_gaps() {
    let json = r##"{
      "styles": {
        "Heading": { "listStyleType": "decimal" },
        "Heading.1": { "listStyleType": "cjk-decimal", "listMarkerFormat": "%1、", "listMarkerGap": "none" },
        "Heading.3": { "listMarkerGap": "tab", "listHanging": 40 },
        "Heading.4": { "listStyleType": "none" }
      },
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["a", "b", "c", "d", "e"] },
        "a": { "type": "Heading", "props": { "level": 1, "text": "总体" } },
        "b": { "type": "Heading", "props": { "level": 2, "text": "二级" } },
        "c": { "type": "Heading", "props": { "level": 3, "text": "三级" } },
        "d": { "type": "Heading", "props": { "level": 4, "text": "四级关掉" } },
        "e": { "type": "Heading", "props": { "level": 5, "text": "五级" } }
      }
    }"##;
    let h = html(json);
    // 五级的默认模板只连开了编号的级别（四级关了）：1.1.1.1
    assert_eq!(html_numbers(&h), ["一、", "1.1", "1.1.1", "1.1.1.1"], "{h}");
    // none：编号紧挨着文字；tab：编号是宽 listHanging 的行内块，标题悬挂
    assert!(h.contains(">一、</span>总体"), "{h}");
    assert!(h.contains("min-width:40pt"), "{h}");
    assert!(h.contains("padding-left:40pt;text-indent:-40pt;"), "{h}");
    let md = markdown(json);
    assert!(
        md.contains("# 一、总体") && md.contains("### 1.1.1 三级"),
        "{md}"
    );
    assert!(md.contains("#### 四级关掉"), "{md}");
    let src = typst(json);
    assert!(src.contains("grid(columns: (calc.max(40pt"), "{src}");
    let d = render(json, Format::Docx);
    let num = unzip(&d, "word/numbering.xml");
    assert!(num.contains("<w:suff w:val=\"nothing\" />"), "{num}");
    assert!(num.contains("<w:lvlText w:val=\"%1.%2.%3\" />"), "{num}");
    // tab：left = hanging = 40pt
    assert!(
        num.contains("<w:ind w:left=\"800\" w:right=\"0\" w:hanging=\"800\" />"),
        "{num}"
    );
    assert!(num.contains("<w:lvlText w:val=\"%1.%2.%3.%5\" />"), "{num}");
}

/// 先出现二级标题：一级按「起始号 - 1」填，和 Word 一样是 0.1。listStart 管起始号。
#[test]
fn counters_follow_word() {
    let json = r##"{
      "styles": { "Heading.1": { "listStyleType": "decimal", "listStart": 3 }, "Heading.2": { "listMarkerFormat": "%1.%2" } },
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["a", "b", "c", "d"] },
        "a": { "type": "Heading", "props": { "level": 2, "text": "先来的二级" } },
        "b": { "type": "Heading", "props": { "level": 1, "text": "一级" } },
        "c": { "type": "Heading", "props": { "level": 2, "text": "二级", "numbered": false } },
        "d": { "type": "Heading", "props": { "level": 2, "text": "二级" } }
      }
    }"##;
    assert_eq!(html_numbers(&html(json)), ["2.1", "3", "3.1"]);
    let d = render(json, Format::Docx);
    assert!(unzip(&d, "word/numbering.xml").contains("<w:start w:val=\"3\" />"));
}

fn check_err(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    format!("{:#}", d.tree().expect_err("应该报错"))
}

#[test]
fn numbering_fields_on_one_heading_are_rejected() {
    let e = check_err(
        r##"{ "root": "doc", "elements": {
          "doc": { "type": "Document", "children": ["h"] },
          "h": { "type": "Heading", "props": { "level": 1, "text": "x" }, "style": { "listMarkerFormat": "%1." } }
        } }"##,
    );
    assert!(e.contains("`h`") && e.contains("Heading.N"), "{e}");
    // 没开编号时 numbered: false 也合法，什么都不变
    let ok = r##"{ "root": "doc", "elements": {
      "doc": { "type": "Document", "children": ["h"] },
      "h": { "type": "Heading", "props": { "level": 1, "text": "x", "numbered": false } }
    } }"##;
    assert!(html_numbers(&html(ok)).is_empty());
    assert!(!unzip(&render(ok, Format::Docx), "word/document.xml").contains("numPr"));
}

/// 读回：标题样式上的编号变回 Heading.N 的字段，numId 0 变回 numbered: false，
/// 重新编号的节变回 restartHeadingNumbering；再渲染编号一样。
#[test]
fn parse_reads_heading_numbering() {
    let bytes = render(DOC, Format::Docx);
    let doc = document_render::parse::docx(
        &bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap()
    .json;
    let st = &doc["styles"];
    assert_eq!(st["Heading.1"]["listMarkerFormat"], "第%1章", "{st}");
    assert_eq!(st["Heading.1"]["listStyleType"], "cjk-decimal", "{st}");
    assert_eq!(st["Heading.2"]["listMarkerFormat"], "%1.%2", "{st}");
    assert!(
        st["Heading.2"]["listStyleType"].is_null(),
        "decimal 是默认：{st}"
    );
    assert!(
        st["Heading.1"]["listMarkerGap"].is_null(),
        "space 是默认：{st}"
    );
    assert!(st["Heading.1"]["marginLeft"].is_null(), "{st}");
    let els = doc["elements"].as_object().unwrap();
    let by_text = |t: &str| {
        els.values()
            .find(|e| e["type"] == "Heading" && e["props"]["text"] == t)
            .unwrap_or_else(|| panic!("没有标题 {t}：{doc}"))
    };
    assert_eq!(by_text("前言")["props"]["numbered"], false);
    assert!(by_text("总则")["props"]["numbered"].is_null());
    assert!(
        by_text("附录")["props"].get("numbered").is_none(),
        "{}",
        by_text("附录")
    );
    let sec = els.values().find(|e| e["type"] == "Section").unwrap();
    assert_eq!(sec["props"]["restartHeadingNumbering"], true, "{sec}");
    // 编号标题不能读成列表
    assert!(!els.values().any(|e| e["type"] == "List"), "{doc}");
    let again = serde_json::to_string(&doc).unwrap();
    assert_eq!(html_numbers(&html(&again)), WANT);
}

/// examples/headings.json（Windows Word 对照用的样例）。
#[test]
fn headings_example() {
    let json = include_str!("../examples/headings.json");
    assert_eq!(
        html_numbers(&html(json)),
        [
            "第一章",
            "1.1",
            "1.2",
            "1.2.1",
            "1.2.2",
            "第二章",
            "2.1",
            "2.1.1",
            "2.2",
            "第一章",
            "1.1"
        ]
    );
    assert!(!render(json, Format::Pdf).is_empty());
    assert!(!render(json, Format::Docx).is_empty());
    assert!(markdown(json).contains("### 1.2.2 缩写"));
}
