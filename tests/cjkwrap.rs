//! 中文断行跟 Word 默认设置一致：pdf 逐行能放就放、标点不压缩；docx 写明不压缩、不悬挂。
//! 依据是 2026-11-21 和 Windows Word 2024 的对比（tests 里不依赖 Word）。

use document_render::render::Format;
use typst::layout::{Frame, FrameItem, Point};

/// 排好的一行：基线 y，和这一行每个字（字符、左边 x、字宽，单位 pt）。
struct Line {
    y: f64,
    chars: Vec<(char, f64, f64)>,
}

fn collect(frame: &Frame, at: Point, out: &mut Vec<(f64, f64, char, f64)>) {
    for (pos, item) in frame.items() {
        let p = at + *pos;
        match item {
            FrameItem::Group(g) => collect(
                &g.frame,
                p + Point::new(g.transform.tx, g.transform.ty),
                out,
            ),
            FrameItem::Text(t) => {
                let mut x = p.x;
                for g in &t.glyphs {
                    let adv = g.x_advance.at(t.size);
                    let c = t.text[g.range()].chars().next().unwrap_or(' ');
                    out.push((
                        p.y.to_pt(),
                        (x + g.x_offset.at(t.size)).to_pt(),
                        c,
                        adv.to_pt(),
                    ));
                    x += adv;
                }
            }
            _ => {}
        }
    }
}

/// 一份只有一个段落的文档排成 pdf 后的各行（只看第一页）。
fn lines(text: &str, align: &str) -> Vec<Line> {
    let json = serde_json::json!({
        "root": "d",
        "elements": {
            "d": {"type": "Document", "children": ["p"]},
            "p": {"type": "Text", "props": {"text": text},
                  "style": {"fontSize": 10, "align": align}}
        }
    })
    .to_string();
    let d = document_render::doc::parse(&json).unwrap();
    let tree = d.tree().unwrap();
    let (doc, _) = document_render::render::pdf::layout(&d, &tree, &Default::default()).unwrap();
    let mut glyphs = Vec::new();
    collect(&doc.pages()[0].frame, Point::zero(), &mut glyphs);
    glyphs.retain(|g| !g.2.is_whitespace());
    let mut out: Vec<Line> = Vec::new();
    for (y, x, c, w) in glyphs {
        match out.iter_mut().find(|l| (l.y - y).abs() < 0.5) {
            Some(l) => l.chars.push((c, x, w)),
            None => out.push(Line {
                y,
                chars: vec![(c, x, w)],
            }),
        }
    }
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    for l in &mut out {
        l.chars.sort_by(|a, b| a.1.total_cmp(&b.1));
    }
    out
}

const PUNCT: &str = "，。、：；！？「」（）《》“”";

/// 一行放得下几个汉字（默认页面、10pt）。
fn per_line() -> usize {
    let ls = lines(&"汉".repeat(200), "left");
    let n = ls[0].chars.len();
    assert!(
        ls.iter()
            .all(|l| l.chars.iter().all(|c| (c.2 - 10.0).abs() < 0.01))
    );
    n
}

/// 相邻标点（「：「」「。」」「），」）和行尾标点都是整字宽：下一个字的 x 正好在一个字号之后。
#[test]
fn cjk_punctuation_keeps_full_width() {
    // 一行放得下 n 个字：第一行 n - 1 个字后正好落一个「，」在行尾
    let n = per_line();
    let text = format!(
        "{}，他说：「好。」然后（走了），又说「行」。{}",
        "汉".repeat(n - 1),
        "字".repeat(30)
    );
    for align in ["left", "justify"] {
        let ls = lines(&text, align);
        assert!(ls.len() >= 2, "{align}：应该排成两行以上");
        let mut seen = 0;
        for l in &ls {
            for (i, &(c, x, w)) in l.chars.iter().enumerate() {
                if !PUNCT.contains(c) {
                    continue;
                }
                seen += 1;
                assert!(
                    w >= 9.99,
                    "{align}：「{c}」字宽 {w:.2}pt，被压缩了（第 {} 行）",
                    ls.iter().position(|m| m.y == l.y).unwrap() + 1
                );
                if align == "left"
                    && let Some(&(n, nx, _)) = l.chars.get(i + 1)
                {
                    assert!(
                        (nx - x - 10.0).abs() < 0.01,
                        "{align}：「{c}{n}」间距 {:.2}pt，应是 10pt",
                        nx - x
                    );
                }
            }
        }
        assert_eq!(seen, 11, "{align}：标点没找全");
        // 行尾的「，」：第一行最后一个字，整字宽
        let first = &ls[0].chars;
        assert_eq!(
            first.last().unwrap().0,
            '，',
            "{align}：第一行该以「，」结尾"
        );
        assert_eq!(first.len(), n, "{align}：第一行 n - 1 个字加「，」");
    }
}

/// 一行放得下 n 个字。n - 1 个字后接「。」」：不压缩就放不下，避头尾把前一个汉字带到下一行，
/// 第一行只剩 n - 2 个字。（压缩时「。」」只占 1.5 字、行尾再收半字，第一行能排 n + 1 个字。）
#[test]
fn adjacent_punctuation_at_line_end_is_not_squeezed() {
    let n = per_line();
    let text = format!("{}。」{}", "汉".repeat(n - 1), "字".repeat(30));
    for align in ["left", "justify"] {
        let ls = lines(&text, align);
        let first: String = ls[0].chars.iter().map(|c| c.0).collect();
        assert_eq!(first.chars().count(), n - 2, "{align}：第一行 {first}");
        let second: String = ls[1].chars.iter().map(|c| c.0).collect();
        assert!(second.starts_with("汉。」"), "{align}：第二行 {second}");
    }
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
    s
}

/// docx 写明「不压缩标点」，段落默认关掉行尾标点悬挂，不写行首标点压缩。
#[test]
fn docx_turns_off_compression_and_hanging_punctuation() {
    let json = r#"{"root":"d","elements":{"d":{"type":"Document","children":["p"]},
      "p":{"type":"Text","props":{"text":"中文，标点。"}}}}"#;
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    let settings = part(&out.bytes, "word/settings.xml");
    assert!(
        settings.contains(r#"<w:characterSpacingControl w:val="doNotCompress" /><w:compat>"#),
        "{settings}"
    );
    assert_eq!(settings.matches("characterSpacingControl").count(), 1);
    let styles = part(&out.bytes, "word/styles.xml");
    let d = styles.find("<w:pPrDefault>").unwrap();
    let e = styles.find("</w:pPrDefault>").unwrap();
    let ppr = &styles[d..e];
    assert!(
        ppr.contains(r#"<w:pPr><w:overflowPunct w:val="0" />"#),
        "{ppr}"
    );
    assert!(!styles.contains("topLinePunct"));
    // 用 roxmltree 确认两个部件还是合法的 XML
    roxmltree::Document::parse(&settings).unwrap();
    roxmltree::Document::parse(&styles).unwrap();
}
