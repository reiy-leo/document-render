//! 多级列表的层级规则：List 元素 `style` 上的编号类型、编号模板、起始号只管第一级，
//! 第二级起只认自己的 `parts.levelN`，没写就按级别取默认；符号级别不用 `listMarkerFormat`。
//! 四种格式里都不能出现 `%1` 这种没填的模板文字，docx 的 numbering.xml 每级 lvlText 要对。

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

/// pdf 的 typst 源码（编号就是源码里的字符串）。
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

/// 文档里第一个列表的 abstractNum（docx-rs 自带一个 id 1 的，列表从 2 起）。
fn first_list_numbering(json: &str) -> String {
    let xml = unzip(&render(json, Format::Docx), "word/numbering.xml");
    let at = xml
        .find("w:abstractNumId=\"2\"")
        .unwrap_or_else(|| panic!("没有列表的 abstractNum：{xml}"));
    let rest = &xml[at..];
    rest[..rest.find("</w:abstractNum>").unwrap()].to_string()
}

/// 第一个列表每一级的 (numFmt, lvlText)。
fn word_levels(json: &str) -> Vec<(String, String)> {
    let first = first_list_numbering(json);
    let attr = |chunk: &str, tag: &str| -> String {
        let at = chunk
            .find(&format!("<w:{tag} w:val=\""))
            .unwrap_or_else(|| panic!("缺 {tag}：{chunk}"));
        let rest = &chunk[at + tag.len() + 11..];
        rest[..rest.find('"').unwrap()].to_string()
    };
    first
        .split("<w:lvl ")
        .skip(1)
        .map(|c| (attr(c, "numFmt"), attr(c, "lvlText")))
        .collect()
}

/// html 里每个编号 span 的文字。
fn html_markers(s: &str) -> Vec<String> {
    s.split("class=\"document-render-marker\"")
        .skip(1)
        .map(|c| {
            let c = &c[c.find('>').unwrap() + 1..];
            c[..c.find("</span>").unwrap()].to_string()
        })
        .collect()
}

/// 模板文字 `%` 后跟数字。
fn has_template(s: &str) -> bool {
    s.as_bytes()
        .windows(2)
        .any(|w| w[0] == b'%' && w[1].is_ascii_digit())
}

fn no_template_anywhere(json: &str) {
    let h = html(json);
    for m in html_markers(&h) {
        assert!(!has_template(&m), "html 编号里有模板文字：{m}");
    }
    let src = typst(json);
    for line in src.lines().filter(|l| l.contains("let m = [")) {
        assert!(!has_template(line), "pdf 编号里有模板文字：{line}");
    }
    assert!(!has_template(&markdown(json)), "markdown 里有模板文字");
    // docx 的 lvlText 允许 %k，但只能指本级和上级，符号级别不能有
    for (i, (fmt, text)) in word_levels(json).iter().enumerate() {
        if fmt == "bullet" {
            assert!(
                !text.contains('%'),
                "docx 第 {} 级是符号，lvlText 却是 {text}",
                i + 1
            );
        }
        for k in text.split('%').skip(1) {
            if let Some(d) = k.chars().next().and_then(|c| c.to_digit(10)) {
                assert!(
                    d as usize <= i + 1,
                    "docx 第 {} 级的 lvlText {text} 指向更深的级别",
                    i + 1
                );
            }
        }
    }
    assert!(!render(json, Format::Pdf).is_empty());
}

/// style 上写 decimal，子列表是无序的。
const DECIMAL_WITH_BULLETS: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["l"] },
    "l": {
      "type": "List",
      "props": { "ordered": true, "items": [
        { "text": "一", "ordered": false, "items": [{ "text": "甲", "items": ["子"] }, "乙"] },
        "二"
      ] },
      "style": { "listStyleType": "decimal" }
    }
  }
}"##;

#[test]
fn style_type_only_applies_to_first_level() {
    let m = html_markers(&html(DECIMAL_WITH_BULLETS));
    assert_eq!(m, ["1.", "◦", "▪", "◦", "2."], "二级三级按默认符号走");
    let lv = word_levels(DECIMAL_WITH_BULLETS);
    assert_eq!(lv[0], ("decimal".into(), "%1.".into()));
    assert_eq!(lv[1], ("bullet".into(), "◦".into()));
    assert_eq!(lv[2], ("bullet".into(), "▪".into()));
    no_template_anywhere(DECIMAL_WITH_BULLETS);
}

/// style 上写模板，子列表有序也有无序。
const FORMAT_NESTED: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["l"] },
    "l": {
      "type": "List",
      "props": { "ordered": true, "items": [
        { "text": "一", "items": [{ "text": "一.1", "ordered": false, "items": ["点"] }, "一.2"] },
        "二"
      ] },
      "style": { "listStyleType": "cjk-decimal", "listMarkerFormat": "第%1条", "listStart": 3 }
    }
  }
}"##;

#[test]
fn style_format_only_applies_to_first_level() {
    let m = html_markers(&html(FORMAT_NESTED));
    // 二级默认 lower-alpha、模板 %2.，从 1 起；三级（无序）默认方块
    assert_eq!(m, ["第三条", "a.", "▪", "b.", "第四条"]);
    let lv = word_levels(FORMAT_NESTED);
    assert_eq!(lv[0], ("chineseCounting".into(), "第%1条".into()));
    assert_eq!(lv[1], ("lowerLetter".into(), "%2.".into()));
    assert_eq!(lv[2], ("bullet".into(), "▪".into()));
    let xml = first_list_numbering(FORMAT_NESTED);
    let lvl2 = xml.split("<w:lvl ").nth(2).unwrap();
    assert!(
        lvl2.contains("<w:start w:val=\"1\""),
        "listStart 只管第一级：{lvl2}"
    );
    no_template_anywhere(FORMAT_NESTED);
}

/// parts.levelN 覆盖：二级写多级模板，三级写符号又写了模板（符号级别不用模板）。
const PARTS: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["l"] },
    "l": {
      "type": "List",
      "props": { "ordered": true, "items": [
        "一",
        { "text": "二", "items": [{ "text": "二.1", "items": ["深"] }, "二.2"] }
      ] },
      "style": { "listMarkerFormat": "%1、", "listMarkerColor": "#CF222E" },
      "parts": {
        "level2": { "listStyleType": "decimal", "listMarkerFormat": "%1.%2" },
        "level3": { "listStyleType": "dash", "listMarkerFormat": "(%3)" }
      }
    }
  }
}"##;

#[test]
fn parts_override_each_level() {
    let h = html(PARTS);
    assert_eq!(html_markers(&h), ["1、", "2、", "2.1", "–", "2.2"]);
    // 元素样式上的其他列表字段（颜色）还是管所有级别
    assert_eq!(h.matches("color:#CF222E").count(), 5, "{h}");
    let lv = word_levels(PARTS);
    assert_eq!(lv[0], ("decimal".into(), "%1、".into()));
    assert_eq!(lv[1], ("decimal".into(), "%1.%2".into()));
    assert_eq!(lv[2], ("bullet".into(), "–".into()));
    no_template_anywhere(PARTS);
}

/// 模板里指向更深级别的 %k 没有编号可填，去掉。
#[test]
fn deeper_placeholders_are_dropped() {
    let json = r##"{
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["l"] },
        "l": { "type": "List", "props": { "ordered": true, "items": ["一", "二"] },
               "style": { "listMarkerFormat": "%1.%2)" } }
      }
    }"##;
    assert_eq!(html_markers(&html(json)), ["1.)", "2.)"]);
    assert_eq!(word_levels(json)[0].1, "%1.)");
    no_template_anywhere(json);
}

/// 改之前：style 上的 listMarkerFormat 用在所有级别，符号子层在 docx 里显示字面的 `%1`。
#[test]
fn bullet_child_of_formatted_list() {
    let json = r##"{
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["l"] },
        "l": { "type": "List",
               "props": { "items": [{ "text": "一", "items": ["点"] }] },
               "style": { "listStyleType": "upper-roman", "listMarkerFormat": "%1)" } }
      }
    }"##;
    assert_eq!(html_markers(&html(json)), ["I)", "◦"]);
    assert_eq!(word_levels(json)[1], ("bullet".into(), "◦".into()));
    no_template_anywhere(json);
}

/// docx 再解析回来：二级按默认比，和默认不一样的才写进 levelN。
#[test]
fn reparse_writes_levels_against_defaults() {
    let bytes = render(FORMAT_NESTED, Format::Docx);
    let parsed = document_render::parse::docx(
        &bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap();
    let l = parsed.json["elements"]
        .as_object()
        .unwrap()
        .values()
        .find(|e| e["type"] == "List")
        .unwrap();
    assert_eq!(l["style"]["listStyleType"], "cjk-decimal", "{l}");
    assert_eq!(l["style"]["listMarkerFormat"], "第%1条");
    assert_eq!(l["style"]["listStart"], 3);
    // 二级就是默认（lower-alpha、%2.、从 1 起），三级是默认的方块：都不用写
    assert!(l["parts"]["level2"].is_null(), "{l}");
    assert!(l["parts"]["level3"].is_null(), "{l}");
    // 解析出来的 JSON 再渲染，编号一样
    let again = serde_json::to_string(&parsed.json).unwrap();
    assert_eq!(
        html_markers(&html(&again)),
        html_markers(&html(FORMAT_NESTED))
    );
}

/// numbering.xml 的 w:lvl 要按 schema（CT_Lvl）的顺序写子元素：isLgl、suff 在 lvlText 前，
/// rPr 在最后；级别里的 pPr 不能带 rPr（空的也不行），rPr 里 rFonts 在 b、color、sz 前。
#[test]
fn numbering_levels_follow_schema_order() {
    let json = r##"{
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["l"] },
        "l": {
          "type": "List",
          "props": { "ordered": true, "items": [
            "一",
            { "text": "二", "items": [{ "text": "二.1", "items": ["深"] }, "二.2"] }
          ] },
          "style": { "listMarkerColor": "#CF222E", "listMarkerFont": "SimHei", "listMarkerGap": "space" },
          "parts": {
            "marker": { "bold": true },
            "level2": { "listStyleType": "decimal", "listMarkerFormat": "%1.%2" }
          }
        }
      }
    }"##;
    let xml = first_list_numbering(json);
    let lvl_order = [
        "start",
        "numFmt",
        "lvlRestart",
        "pStyle",
        "isLgl",
        "suff",
        "lvlText",
        "lvlPicBulletId",
        "legacy",
        "lvlJc",
        "pPr",
        "rPr",
    ];
    let rpr_order = ["rFonts", "b", "bCs", "i", "iCs", "color", "sz", "szCs"];
    let pos = |s: &str, tag: &str| {
        let pat = format!("<w:{tag}");
        s.match_indices(&pat)
            .find(|(i, _)| matches!(s[i + pat.len()..].chars().next(), Some(' ' | '/' | '>')))
            .map(|(i, _)| i)
    };
    let lvls: Vec<&str> = xml.split("<w:lvl ").skip(1).collect();
    assert!(!lvls.is_empty(), "{xml}");
    let mut saw_lgl = false;
    let mut saw_fonts = false;
    for lvl in &lvls {
        let lvl = &lvl[..lvl.find("</w:lvl>").unwrap()];
        // pPr 和 rPr 之外的顶层子元素按 CT_Lvl 顺序
        let ppr = pos(lvl, "pPr").expect("每级都有 pPr");
        let ppr_end = ppr + lvl[ppr..].find("</w:pPr>").unwrap();
        let top: Vec<usize> = lvl_order
            .iter()
            .filter_map(|t| match *t {
                "pPr" => Some(ppr),
                "rPr" => lvl[ppr_end..].find("<w:rPr").map(|j| ppr_end + j),
                t => pos(&lvl[..ppr], t),
            })
            .collect();
        assert!(top.windows(2).all(|w| w[0] < w[1]), "顺序不对：{lvl}");
        if let (Some(a), Some(b)) = (pos(lvl, "isLgl"), pos(lvl, "lvlText")) {
            assert!(a < b, "isLgl 要在 lvlText 前：{lvl}");
            saw_lgl = true;
        }
        let p = &lvl[ppr..ppr_end];
        assert!(!p.contains("<w:rPr"), "pPr 里不该有 rPr：{lvl}");
        let r = &lvl[ppr_end..];
        let rp: Vec<usize> = rpr_order.iter().filter_map(|t| pos(r, t)).collect();
        assert!(rp.windows(2).all(|w| w[0] < w[1]), "rPr 顺序不对：{lvl}");
        assert!(pos(r, "b").is_some(), "{lvl}");
        saw_fonts |= pos(r, "rFonts").is_some();
    }
    assert!(saw_fonts, "第一级的编号字体要写进 rFonts：{xml}");
    assert!(saw_lgl, "二级 decimal 应该有 isLgl：{xml}");
    assert!(xml.contains("<w:suff w:val=\"space\""), "{xml}");
}

// ---------- continueFrom：两个分开的列表接着编号 ----------

/// 三段列表：B 接 A，C 接 B，中间隔着正文。A 有子列表，接续只数第一级。
const CHAIN: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["a", "t1", "b", "t2", "c"] },
    "a": { "type": "List", "props": { "ordered": true, "items": [
      "一", { "text": "二", "items": ["二甲", "二乙"] }, "三"
    ] } },
    "t1": { "type": "Text", "props": { "text": "中间一段正文" } },
    "b": { "type": "List", "props": { "ordered": true, "continueFrom": "a", "items": ["四", "五"] } },
    "t2": { "type": "Text", "props": { "text": "又一段" } },
    "c": { "type": "List", "props": { "ordered": true, "continueFrom": "b", "items": ["六"] } }
  }
}"##;

/// document.xml 里每个列表段落的 (numId, ilvl)。
fn para_nums(docx: &[u8]) -> Vec<(String, String)> {
    let xml = unzip(docx, "word/document.xml");
    xml.split("<w:numPr>")
        .skip(1)
        .map(|c| {
            let get = |tag: &str| {
                let at = c.find(&format!("<w:{tag} w:val=\"")).unwrap();
                let r = &c[at + tag.len() + 11..];
                r[..r.find('"').unwrap()].to_string()
            };
            (get("numId"), get("ilvl"))
        })
        .collect()
}

/// numbering.xml 里 numId 为 `id` 的 w:num。
fn num_xml(docx: &[u8], id: &str) -> String {
    let xml = unzip(docx, "word/numbering.xml");
    let at = xml
        .find(&format!("<w:num w:numId=\"{id}\""))
        .unwrap_or_else(|| panic!("没有 num {id}：{xml}"));
    let r = &xml[at..];
    r[..r.find("</w:num>").unwrap()].to_string()
}

#[test]
fn continue_from_numbers_in_every_format() {
    let h = html(CHAIN);
    assert_eq!(
        html_markers(&h),
        ["1.", "2.", "a.", "b.", "3.", "4.", "5.", "6."],
        "{h}"
    );
    let src = typst(CHAIN);
    let pdf: Vec<&str> = src
        .lines()
        .filter(|l| l.contains("let m = ["))
        .map(|l| {
            let at = l.find("#\"").unwrap() + 2;
            &l[at..at + l[at..].find('"').unwrap()]
        })
        .collect();
    assert_eq!(pdf, ["1.", "2.", "a.", "b.", "3.", "4.", "5.", "6."]);
    let md = markdown(CHAIN);
    assert!(
        md.contains("4. 四") && md.contains("5. 五") && md.contains("6. 六"),
        "{md}"
    );
    // docx：三段列表的第一级共用一个 num，Word 里真正接着数
    let d = render(CHAIN, Format::Docx);
    let nums = para_nums(&d);
    assert_eq!(nums.len(), 8);
    let top: Vec<&String> = nums
        .iter()
        .filter(|(_, l)| l == "0")
        .map(|(n, _)| n)
        .collect();
    assert!(top.iter().all(|n| *n == top[0]), "{nums:?}");
    assert!(!num_xml(&d, top[0]).contains("startOverride"));
}

#[test]
fn continue_from_after_list_start() {
    let json = CHAIN.replace(
        r#""a": { "type": "List", "props""#,
        r#""a": { "type": "List", "style": { "listStart": 3 }, "props""#,
    );
    assert_eq!(
        html_markers(&html(&json)),
        ["3.", "4.", "a.", "b.", "5.", "6.", "7.", "8."]
    );
    assert!(markdown(&json).contains("8. 六"));
}

/// 第一级定义不同（B 用大写罗马数字）：docx 单开一个 abstractNum，w:start 写算好的号。
#[test]
fn continue_from_with_other_format() {
    let json = CHAIN.replace(
        r#""continueFrom": "a", "items": ["四", "五"] } },"#,
        r#""continueFrom": "a", "items": ["四", "五"] }, "style": { "listStyleType": "upper-roman" } },"#,
    );
    assert_eq!(
        html_markers(&html(&json)),
        ["1.", "2.", "a.", "b.", "3.", "IV.", "V.", "6."]
    );
    let d = render(&json, Format::Docx);
    let nums = para_nums(&d);
    let xml = unzip(&d, "word/numbering.xml");
    assert_ne!(nums[0].0, nums[5].0, "{nums:?}");
    assert!(xml.contains("<w:start w:val=\"4\" />"), "{xml}");
    assert!(xml.contains("upperRoman"));
    // C 接的是 B，B 的定义和 C 不同，C 也单开，从 6 起
    assert!(xml.contains("<w:start w:val=\"6\" />"), "{xml}");
}

fn check_err(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    format!("{:#}", d.tree().expect_err("应该报错"))
}

#[test]
fn continue_from_errors() {
    let missing = CHAIN.replace(r#""continueFrom": "a""#, r#""continueFrom": "nope""#);
    assert!(check_err(&missing).contains("`nope` 不存在"));
    let not_list = CHAIN.replace(r#""continueFrom": "a""#, r#""continueFrom": "t1""#);
    let e = check_err(&not_list);
    assert!(e.contains("要写一个 List") && e.contains("Text"), "{e}");
    let later = CHAIN.replace(r#""continueFrom": "a""#, r#""continueFrom": "c""#);
    assert!(check_err(&later).contains("出现在它前面"));
    let selfref = CHAIN.replace(r#""continueFrom": "a""#, r#""continueFrom": "b""#);
    assert!(check_err(&selfref).contains("出现在它前面"));
    let both = CHAIN.replace(
        r#""continueFrom": "a", "items": ["四", "五"] } },"#,
        r#""continueFrom": "a", "items": ["四", "五"] }, "style": { "listStart": 9 } },"#,
    );
    let e = check_err(&both);
    assert!(e.contains("continueFrom 和 listStart"), "{e}");
}

// ---------- 同一级里有序、无序混用 ----------

const MIXED: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["l"] },
    "l": { "type": "List", "props": { "ordered": true, "items": [
      { "text": "一", "items": ["甲", "乙"] },
      { "text": "二", "ordered": false, "items": ["点", "点"] },
      { "text": "三", "items": ["丙", "丁", "戊"] },
      "四"
    ] } }
  }
}"##;

#[test]
fn mixed_sublists_in_every_format() {
    let want = [
        "1.", "a.", "b.", "2.", "◦", "◦", "3.", "a.", "b.", "c.", "4.",
    ];
    assert_eq!(html_markers(&html(MIXED)), want);
    let src = typst(MIXED);
    let n = src.lines().filter(|l| l.contains("let m = [")).count();
    assert_eq!(n, want.len());
    assert!(src.contains("#\"◦\""));
    let md = markdown(MIXED);
    assert!(
        md.contains("2. 二") && md.contains("- 点") && md.contains("1. 丙"),
        "{md}"
    );
}

#[test]
fn mixed_sublists_docx_numbering() {
    let d = render(MIXED, Format::Docx);
    let nums = para_nums(&d);
    assert_eq!(nums.len(), 11);
    // 第一级四项同一个 num，父级编号连续
    let main = &nums[0].0;
    for i in [0, 3, 6, 10] {
        assert_eq!(&nums[i].0, main, "{nums:?}");
        assert_eq!(nums[i].1, "0");
    }
    // 第一串、第三串子列表和定义一样，用主 num（Word 在新的第一级项下自己从头数）
    for i in [1, 2, 7, 8, 9] {
        assert_eq!(&nums[i].0, main, "{nums:?}");
    }
    // 第二串单开一个 num：lvlOverride 换成符号，startOverride 在 lvl 前
    let other = &nums[4].0;
    assert_ne!(other, main);
    assert_eq!(&nums[5].0, other);
    let x = num_xml(&d, other);
    let abs_main = {
        let m = num_xml(&d, main);
        let at = m.find("w:abstractNumId w:val=\"").unwrap() + 23;
        m[at..at + m[at..].find('"').unwrap()].to_string()
    };
    assert!(
        x.contains(&format!("<w:abstractNumId w:val=\"{abs_main}\" />")),
        "{x}"
    );
    assert!(x.contains("<w:lvlOverride w:ilvl=\"1\">"), "{x}");
    let so = x.find("<w:startOverride w:val=\"1\" />").expect(&x);
    let lv = x.find("<w:lvl w:ilvl=\"1\"").expect(&x);
    assert!(so < lv, "{x}");
    assert!(x.contains("<w:numFmt w:val=\"bullet\" />"));
    assert!(x.contains("<w:lvlText w:val=\"◦\" />"));
    // 主定义第二级是 lowerLetter、%2.
    let lv = word_levels(MIXED);
    assert_eq!(lv[1], ("lowerLetter".into(), "%2.".into()));
}

/// 第一串子列表是符号，后面的有序子列表单开 num，也从 1（a.）起。
#[test]
fn mixed_sublists_bullet_first() {
    let json = r##"{
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["l"] },
        "l": { "type": "List", "props": { "ordered": true, "items": [
          { "text": "一", "ordered": false, "items": ["点"] },
          { "text": "二", "items": ["甲", "乙"] },
          { "text": "三", "items": ["丙"] }
        ] } }
      }
    }"##;
    assert_eq!(
        html_markers(&html(json)),
        ["1.", "◦", "2.", "a.", "b.", "3.", "a."]
    );
    let d = render(json, Format::Docx);
    let nums = para_nums(&d);
    assert_ne!(nums[3].0, nums[0].0);
    assert_ne!(
        nums[6].0, nums[3].0,
        "每串不同定义的子列表各开一个 num，各自从头数"
    );
    let x = num_xml(&d, &nums[3].0);
    assert!(x.contains("lowerLetter") && x.contains("<w:startOverride w:val=\"1\" />"));
}

// ---------- itemSpacing ----------

const SPACED: &str = r##"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["l"] },
    "l": {
      "type": "List",
      "props": { "ordered": true, "items": ["一", { "text": "二", "items": ["甲", "乙"] }, "三"] },
      "style": { "itemSpacing": 6, "spaceBefore": 10, "spaceAfter": 12 },
      "parts": { "level2": { "itemSpacing": 2 } }
    }
  }
}"##;

/// document.xml 里每个列表段落的 (before, after)，缇。
fn para_spacing(docx: &[u8]) -> Vec<(u32, u32)> {
    let xml = unzip(docx, "word/document.xml");
    let mut paras = Vec::new();
    let mut rest = xml.as_str();
    while let Some(at) = rest.find("<w:p>").or_else(|| rest.find("<w:p ")) {
        let at = [rest.find("<w:p>"), rest.find("<w:p ")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(at);
        let end = at + rest[at..].find("</w:p>").unwrap();
        paras.push(&rest[at..end]);
        rest = &rest[end..];
    }
    paras
        .into_iter()
        .filter(|c| c.contains("<w:numPr>"))
        .map(|c| {
            let sp = &c[c.find("<w:spacing").unwrap()..];
            let sp = &sp[..sp.find("/>").unwrap()];
            let get = |a: &str| {
                sp.find(&format!("w:{a}=\""))
                    .map(|at| {
                        let r = &sp[at + a.len() + 4..];
                        r[..r.find('"').unwrap()].parse().unwrap()
                    })
                    .unwrap_or(0)
            };
            (get("before"), get("after"))
        })
        .collect()
}

#[test]
fn item_spacing_docx() {
    let sp = para_spacing(&render(SPACED, Format::Docx));
    // 一→二 6pt；二→甲、甲→乙、乙→三按第二级 2pt；整体段前 10、段后 12
    assert_eq!(sp, [(200, 0), (120, 0), (40, 0), (40, 0), (40, 240)]);
}

#[test]
fn item_spacing_html_and_pdf() {
    let h = html(SPACED);
    let items: Vec<&str> = h
        .split("<div style=\"")
        .skip(1)
        .filter(|c| c.contains("document-render-marker"))
        .map(|c| &c[..c.find('"').unwrap()])
        .collect();
    assert_eq!(items.len(), 5);
    assert!(!items[0].contains("margin-top"), "{}", items[0]);
    assert!(items[1].contains("margin-top:6pt"), "{}", items[1]);
    for x in &items[2..] {
        assert!(x.contains("margin-top:2pt"), "{x}");
    }
    for x in &items[..4] {
        assert!(x.contains("margin-bottom:0pt"), "{x}");
    }
    let src = typst(SPACED);
    assert_eq!(src.matches("above: 6pt").count(), 1, "{src}");
    assert_eq!(src.matches("above: 2pt").count(), 3, "{src}");
    // markdown 没有间距，照常出
    assert!(markdown(SPACED).contains("1. 一"));
}

// ---------- parse 读回 ----------

fn reparse(json: &str) -> serde_json::Value {
    let bytes = render(json, Format::Docx);
    document_render::parse::docx(
        &bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap()
    .json
}

/// 按文档顺序的 List 元素。
fn lists_in_order(doc: &serde_json::Value) -> Vec<(String, serde_json::Value)> {
    let els = &doc["elements"];
    let mut out = Vec::new();
    let mut stack = vec![doc["root"].as_str().unwrap().to_string()];
    while let Some(id) = stack.pop() {
        let e = &els[&id];
        if e["type"] == "List" {
            out.push((id.clone(), e.clone()));
        }
        if let Some(ch) = e["children"].as_array() {
            for c in ch.iter().rev() {
                stack.push(c.as_str().unwrap().to_string());
            }
        }
    }
    out
}

#[test]
fn parse_reads_continue_from() {
    let doc = reparse(CHAIN);
    let ls = lists_in_order(&doc);
    assert_eq!(ls.len(), 3, "{doc}");
    assert!(ls[0].1["props"]["continueFrom"].is_null());
    assert_eq!(ls[1].1["props"]["continueFrom"], ls[0].0.as_str());
    assert_eq!(ls[2].1["props"]["continueFrom"], ls[1].0.as_str());
    let again = serde_json::to_string(&doc).unwrap();
    assert_eq!(html_markers(&html(&again)), html_markers(&html(CHAIN)));
    assert_eq!(
        para_nums(&render(&again, Format::Docx)),
        para_nums(&render(CHAIN, Format::Docx))
    );
}

#[test]
fn parse_reads_mixed_sublists() {
    let doc = reparse(MIXED);
    let ls = lists_in_order(&doc);
    assert_eq!(ls.len(), 1, "{doc}");
    let items = &ls[0].1["props"]["items"];
    assert_eq!(items[1]["ordered"], false, "{items}");
    assert!(items[0]["ordered"].is_null() && items[2]["ordered"].is_null());
    assert!(ls[0].1["parts"]["level2"]["listStyleType"].is_null());
    let again = serde_json::to_string(&doc).unwrap();
    assert_eq!(html_markers(&html(&again)), html_markers(&html(MIXED)));
}

#[test]
fn parse_reads_item_spacing() {
    let doc = reparse(SPACED);
    let ls = lists_in_order(&doc);
    let l = &ls[0].1;
    assert_eq!(l["style"]["itemSpacing"], 6.0, "{l}");
    assert_eq!(l["parts"]["level2"]["itemSpacing"], 2.0, "{l}");
    assert!(l["parts"]["item"].is_null(), "{l}");
    let again = serde_json::to_string(&doc).unwrap();
    assert_eq!(
        para_spacing(&render(&again, Format::Docx)),
        para_spacing(&render(SPACED, Format::Docx))
    );
}

/// examples/lists.json（也是 Windows Word 对照用的样例）：四种格式都能出，编号对。
#[test]
fn lists_example() {
    let json = include_str!("../examples/lists.json");
    let want = [
        "1.", "2.", "a.", "b.", "3.", "4.", "5.", "6.", // 接着编号
        "1.", "a.", "b.", "2.", "◦", "◦", "3.", "a.", "b.", "c.", "4.", // 同级混用
        "1.", "◦", "2.", "2.a", "2.b", "3.", "3.a", // 子级模板带上级编号
        "1.", "2.", "a.", "b.", "3.", "4.", // 项间距
    ];
    assert_eq!(html_markers(&html(json)), want);
    assert!(!render(json, Format::Pdf).is_empty());
    assert!(!render(json, Format::Docx).is_empty());
    assert!(markdown(json).contains("6. C 应为 6"));
}
