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
    lines_with(text, align, 0.0, true)
}

/// 同 `lines`，段落右缩进 `margin_right` pt，`auto_space` 是中西文自动间距开关。
fn lines_with(text: &str, align: &str, margin_right: f64, auto_space: bool) -> Vec<Line> {
    let json = serde_json::json!({
        "root": "d",
        "elements": {
            "d": {"type": "Document", "children": ["p"]},
            "p": {"type": "Text", "props": {"text": text},
                  "style": {"fontSize": 10, "align": align, "marginRight": margin_right,
                            "autoSpace": auto_space}}
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

/// 一行里字符 `a` 紧接着 `b` 时，`b` 的 x 减 `a` 的 x（pt）。
fn step(l: &Line, a: char, b: char) -> f64 {
    l.chars
        .windows(2)
        .find(|w| w[0].0 == a && w[1].0 == b)
        .map(|w| w[1].1 - w[0].1)
        .unwrap_or_else(|| panic!("找不到「{a}{b}」"))
}

/// 中西文自动间距加在哪些字旁边，跟 Word 2024 实测一致：
/// 汉字和 ASCII 数字、拉丁/希腊/西里尔字母之间加 1/4 字（10pt 时 2.5pt）；
/// 「%」「$」「°」、全角字母、「〇」旁边都不加（Typst 原来在 % $ # & 旁边也加）。
#[test]
fn cjk_latin_spacing_follows_word() {
    let text = "增长18.7%其中价格$5元温度30°C左右ＡＢ全角〇5号α粒子";
    let on = &lines_with(text, "left", 0.0, true)[0];
    let off = &lines_with(text, "left", 0.0, false)[0];
    // 开关间距前后同一对字的距离差，就是加的间距
    let added = |a, b| step(on, a, b) - step(off, a, b);
    for (a, b) in [
        ('长', '1'),
        ('5', '元'),
        ('C', '左'),
        ('5', '号'),
        ('α', '粒'),
    ] {
        let g = added(a, b);
        assert!(
            (g - 2.5).abs() < 0.01,
            "「{a}{b}」加了 {g:.2}pt，应是 2.5pt"
        );
    }
    for (a, b) in [
        ('%', '其'),
        ('格', '$'),
        ('0', '°'),
        ('Ｂ', '全'),
        ('〇', '5'),
    ] {
        let g = added(a, b);
        assert!(g.abs() < 0.01, "「{a}{b}」加了 {g:.2}pt，应是 0");
    }
}

/// 一行汉字中间夹一个数字（两处中西文间距），算出右缩进让这一行自然宽度正好超出 `over` pt。
/// 返回（文字，右缩进，第一行候选的字数，数字的字宽）。
fn squeeze_case(over: f64) -> (String, f64, usize, f64) {
    // 版心宽：两端对齐的纯汉字第一行从第一个字左边到最后一个字右边
    let full = lines(&"汉".repeat(200), "justify");
    let f = &full[0].chars;
    let width = f.last().unwrap().1 + f.last().unwrap().2 - f[0].1;
    // 数字自己的宽度（关掉自动间距量）
    let probe = &lines_with("汉5汉", "left", 0.0, false)[0];
    let w5 = probe.chars[2].1 - probe.chars[1].1;
    // 候选第一行：10 个汉字 + 「5」+ k 个汉字，自然宽度 = (10 + k) × 10 + w5 + 两处 2.5
    // 取放得下的最大 k，再用右缩进把可用宽度缩到比自然宽度少 `over`
    let k = ((width - w5 - 5.0) / 10.0).floor() as usize - 10;
    let natural = (10 + k) as f64 * 10.0 + w5 + 5.0;
    let margin = width - natural + over;
    assert!(margin >= 0.0, "右缩进 {margin}");
    let text = format!("{}5{}{}", "汉".repeat(10), "字".repeat(k), "后".repeat(40));
    (text, margin, 11 + k, w5)
}

/// 两端对齐时，为了多放一个字 Word 会压中西文间距；左对齐不压。
/// 两处间距的行最多压约 0.195 字（10pt 时 1.95pt），每处不少于 1/8 字。
/// 放不下时这一行要拉开：中西文间距先拉（最多各加 1/4 字），剩下的才分给汉字之间。
#[test]
fn justified_line_squeezes_cjk_latin_spacing_to_fit() {
    let (text, margin, n, w5) = squeeze_case(1.0);
    let left = lines_with(&text, "left", margin, true);
    assert_eq!(left[0].chars.len(), n - 1, "左对齐不压，第一行少一个字");
    let just = lines_with(&text, "justify", margin, true);
    let l = &just[0];
    assert_eq!(l.chars.len(), n, "两端对齐压间距，第一行多放一个字");
    for (a, b, w) in [('汉', '5', 10.0), ('5', '字', w5)] {
        let g = step(l, a, b) - w;
        assert!(
            (1.25..2.5).contains(&g),
            "「{a}{b}」间距 {g:.2}pt，应压到 1.25～2.5pt 之间"
        );
    }
    // 汉字之间不压
    let g = step(l, '字', '字') - 10.0;
    assert!(g.abs() < 0.01, "汉字之间 {g:.2}pt");

    // 超出 2.2pt：照 Typst 原来「每处可压到 1/8 字」能放下（共 2.5pt），Word 不压这么多
    let (text, margin, n, w5) = squeeze_case(2.2);
    let just = lines_with(&text, "justify", margin, true);
    let l = &just[0];
    assert_eq!(l.chars.len(), n - 1, "超出太多，不压，少放一个字");
    // 这一行要拉开 7.8pt：两处间距先各拉到 5pt，剩下 2.8pt 才分给汉字之间
    for (a, b, w) in [('汉', '5', 10.0), ('5', '字', w5)] {
        let g = step(l, a, b) - w;
        assert!(g >= 4.99, "「{a}{b}」间距 {g:.2}pt，应先拉到 5pt");
    }
    let g = step(l, '字', '字') - 10.0;
    assert!(g > 0.0 && g < 0.2, "汉字之间只分到剩下的一点：{g:.2}pt");
}
