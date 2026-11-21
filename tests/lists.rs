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
