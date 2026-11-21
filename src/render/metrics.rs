//! 行高：按 Windows 版 Word 的规则算字体度量和行盒，pdf、html 共用；docx 用它给 fontTable 写 charset。
//!
//! 规则来自 2026-11-18 在 Windows Word 2024 上的实测（26 个系统字体加一批改过度量的合成字体）：
//!
//! - 西文字体：fsSelection bit7（USE_TYPO_METRICS）关时，单倍 = max(win 全高, hhea 全高含 lineGap)，
//!   多出 win 的部分（external leading）全加在基线上面；bit7 开时只看 typo，typoLineGap 也加在上面。
//! - 东亚字体（ulCodePageRange1 有 bit17–20 任一位）：底数 T 是 win 全高（bit7 开时是 typo 全高含 lineGap），
//!   不看 hhea；上下各加 e = floor(0.15 × T + 0.5) 个字体单位。所以单倍约 1.3 × T，宋体是 1.29688。
//!   东亚字体里的西文字母也一样。
//! - 一行里有几种字体：上沿取最大 ascent，下沿取最大 descent。
//! - auto 倍数 m：行高 = 混排单倍 + (m − 1) × 各字体单倍里最大的那个，多出来的加在基线下面。
//! - exact：行高就是设定值，基线在 0.8 × 行高处。
//! - atLeast：行高 = max(设定值, 自然行高)，多出来的加在基线上面。
//!
//! 数字 lineHeight（CSS 倍数 x）在 docx 里写成「至少 字号 × x」，所以这里也按 atLeast 算，三种格式一致。

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use typst::text::FontVariant;

use crate::style::{LineHeight, Style};

/// 默认西文字体（样式里没写字体时），和 pdf 的默认字体列表前半截一样。
pub const DEFAULT_LATIN: &[&str] = &["Helvetica Neue", "Arial", "Liberation Sans"];
/// 默认中文字体。docx 里写的是 PingFang SC；Windows 上没有它，Word 换成微软雅黑（2026-11-18 实测），
/// 所以雅黑排在思源、Noto 前面，pdf 在装了雅黑的机器上跟 Windows Word 一样。
pub const DEFAULT_EAST_ASIA: &[&str] = &[
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
];
/// 一个字体都找不到时的 ascent、descent（em）。
const FALLBACK: FontMetrics = FontMetrics {
    asc: 0.88,
    desc: 0.29,
    east_asian: false,
};

/// 从字体文件里读出来的原始度量（字体单位）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RawMetrics {
    pub upm: f32,
    pub hhea_asc: f32,
    /// hhea.descender，通常是负数
    pub hhea_desc: f32,
    pub hhea_gap: f32,
    pub os2: Option<Os2>,
}

/// OS/2 表里和行高有关的字段。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Os2 {
    pub use_typo: bool,
    pub typo_asc: f32,
    /// sTypoDescender，通常是负数
    pub typo_desc: f32,
    pub typo_gap: f32,
    pub win_asc: f32,
    /// usWinDescent，正数
    pub win_desc: f32,
    /// ulCodePageRange1；OS/2 版本 0 没有这个字段，按 0 算
    pub code_pages: u32,
    /// 下标、上标的字号和偏移（ySubscriptYSize、ySubscriptYOffset、ySuperscriptYSize、
    /// ySuperscriptYOffset，字体单位，偏移都是正数：下标往下、上标往上）
    pub sub_size: f32,
    pub sub_offset: f32,
    pub sup_size: f32,
    pub sup_offset: f32,
}

/// 一种字体在 Word 里的 ascent、descent（字号的倍数，都是正数），东亚字体已经加过上下的 e。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    pub asc: f32,
    pub desc: f32,
    pub east_asian: bool,
}

impl FontMetrics {
    /// 单倍行高（字号的倍数）。
    pub fn single(&self) -> f32 {
        self.asc + self.desc
    }
}

/// ulCodePageRange1 的 bit17（932 日文）、bit18（936 简中）、bit19（949 韩文）、bit20（950 繁中）。
const EA_BITS: u32 = 0b1111 << 17;

impl RawMetrics {
    pub fn from_face(face: &ttf_parser::Face) -> RawMetrics {
        let hhea = face.tables().hhea;
        let os2 = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"OS/2"))
            .and_then(parse_os2);
        RawMetrics {
            upm: face.units_per_em() as f32,
            hhea_asc: hhea.ascender as f32,
            hhea_desc: hhea.descender as f32,
            hhea_gap: hhea.line_gap as f32,
            os2,
        }
    }

    /// Word 会不会把它当东亚字体（乘 1.3）：代码页里有日文、简中、韩文、繁中任一种。
    pub fn east_asian(&self) -> bool {
        self.os2.is_some_and(|o| o.code_pages & EA_BITS != 0)
    }

    /// 按 Word（Windows）的规则算 ascent、descent。
    pub fn word(&self) -> Option<FontMetrics> {
        if self.upm <= 0.0 {
            return None;
        }
        let ea = self.east_asian();
        let (mut asc, mut desc) = match self.os2 {
            Some(o) if o.use_typo => (o.typo_asc + o.typo_gap, -o.typo_desc),
            Some(o) if ea => (o.win_asc, o.win_desc),
            Some(o) => {
                let hhea = self.hhea_asc - self.hhea_desc + self.hhea_gap;
                let lead = (hhea - (o.win_asc + o.win_desc)).max(0.0);
                (o.win_asc + lead, o.win_desc)
            }
            None => (self.hhea_asc + self.hhea_gap, -self.hhea_desc),
        };
        if ea {
            // 上下各加 0.15 × 全高，按字体单位四舍五入（.5 进位）
            let e = (0.15 * (asc + desc) + 0.5).floor();
            asc += e;
            desc += e;
        }
        let (asc, desc) = (asc / self.upm, desc / self.upm);
        (asc + desc > 0.0).then_some(FontMetrics {
            asc,
            desc,
            east_asian: ea,
        })
    }

    /// docx fontTable 里 w:charset 该写的值。Word 只在 charset 和字体的代码页位对上时才用这个字体、乘 1.3
    /// （86↔bit18、80↔bit17、81↔bit19、88↔bit20），写 00 就不乘，写错了会换成别的字体。
    /// 几种都有时按简中、日文、韩文、繁中的顺序挑。符号字体是 02。
    pub fn charset(&self) -> &'static str {
        let cp = self.os2.map(|o| o.code_pages).unwrap_or(0);
        if cp & (1 << 18) != 0 {
            "86"
        } else if cp & (1 << 17) != 0 {
            "80"
        } else if cp & (1 << 19) != 0 {
            "81"
        } else if cp & (1 << 20) != 0 {
            "88"
        } else if cp & (1 << 31) != 0 && cp & 1 == 0 {
            "02"
        } else {
            "00"
        }
    }
}

fn parse_os2(d: &[u8]) -> Option<Os2> {
    let u16_at = |i: usize| d.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let i16_at = |i: usize| u16_at(i).map(|v| v as i16 as f32);
    let version = u16_at(0)?;
    let code_pages = if version >= 1 {
        d.get(78..82)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .unwrap_or(0)
    } else {
        0
    };
    Some(Os2 {
        use_typo: u16_at(62)? & (1 << 7) != 0,
        typo_asc: i16_at(68)?,
        typo_desc: i16_at(70)?,
        typo_gap: i16_at(72)?,
        win_asc: u16_at(74)? as f32,
        win_desc: u16_at(76)? as f32,
        code_pages,
        sub_size: i16_at(12)?,
        sub_offset: i16_at(16)?,
        sup_size: i16_at(20)?,
        sup_offset: i16_at(24)?,
    })
}

/// 按字体名找本机（typst 自带加系统）字体的原始度量，有缓存。找不到是 None。
pub fn family_raw(name: &str) -> Option<RawMetrics> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<RawMetrics>>>> = OnceLock::new();
    let key = crate::fonts::resolve(name)
        .unwrap_or_else(|| name.trim().to_string())
        .to_lowercase();
    if key.is_empty() {
        return None;
    }
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok()?.get(&key) {
        return *v;
    }
    let store = super::pdf::font_store();
    let v = store
        .book()
        .select(&key, FontVariant::default())
        .and_then(|i| store.font(i))
        .and_then(|f| {
            ttf_parser::Face::parse(f.data(), f.index())
                .ok()
                .map(|face| RawMetrics::from_face(&face))
        });
    if let Ok(mut c) = cache.lock() {
        c.insert(key, v);
    }
    v
}

/// 逗号分开的字体列表里第一个本机找得到的字体的 Word 度量。
fn first_found<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<FontMetrics> {
    names
        .into_iter()
        .flat_map(|x| x.split(','))
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .find_map(|n| family_raw(n).and_then(|r| r.word()))
}

/// 上标、下标的字号和偏移（都是原字号的倍数，偏移正数：上标往上、下标往下）。
/// Word 直接用字体 OS/2 里的值（2026-11-20 在 Windows Word 2024 上量过：宋体、雅黑 20pt 上标
/// 字号 9.96、抬高 9.96，Calibri 字号 12.96、抬高 6.48），Typst 的 super/sub 默认也读它们。
/// 西文看 latin 字体，有中日韩文字时看东亚字体；字体里没有这些值时用 Typst 的默认
/// （上标 0.6 倍、抬高 0.5，下标 0.6 倍、下移 0.2）。
pub fn script_metrics(s: &Style, cjk: bool, sup: bool) -> (f32, f32) {
    let f = s.font_family.as_ref();
    let names: Vec<&str> = if cjk {
        f.and_then(|f| f.east_asia.as_deref())
            .into_iter()
            .chain(DEFAULT_EAST_ASIA.iter().copied())
            .collect()
    } else {
        f.and_then(|f| f.latin.as_deref())
            .into_iter()
            .chain(DEFAULT_LATIN.iter().copied())
            .collect()
    };
    let raw = names
        .into_iter()
        .flat_map(|x| x.split(','))
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .find_map(family_raw);
    script_from(raw, sup)
}

fn script_from(raw: Option<RawMetrics>, sup: bool) -> (f32, f32) {
    let fallback = if sup { (0.6, 0.5) } else { (0.6, 0.2) };
    let Some(r) = raw else { return fallback };
    let Some(o) = r.os2 else { return fallback };
    let (size, off) = if sup {
        (o.sup_size, o.sup_offset)
    } else {
        (o.sub_size, o.sub_offset)
    };
    if size <= 0.0 || r.upm <= 0.0 {
        return fallback;
    }
    (size / r.upm, off / r.upm)
}

/// 一行的度量：最大 ascent、最大 descent、各字体单倍里最大的（都是段落字号的倍数）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    pub asc: f32,
    pub desc: f32,
    pub max_single: f32,
}

impl LineMetrics {
    /// 几种字体排在同一行，每种带一个相对段落字号的比例。
    pub fn of(fonts: &[(FontMetrics, f32)]) -> LineMetrics {
        let mut m = LineMetrics {
            asc: 0.0,
            desc: 0.0,
            max_single: 0.0,
        };
        for (f, k) in fonts {
            m.asc = m.asc.max(f.asc * k);
            m.desc = m.desc.max(f.desc * k);
            m.max_single = m.max_single.max(f.single() * k);
        }
        m
    }

    /// 自然行高（单倍，字号的倍数）。
    pub fn natural(&self) -> f32 {
        self.asc + self.desc
    }
}

/// 样式这一行用到的字体：西文字体总在（段落标记、数字、标点），有中日韩文字时加上东亚字体，
/// 编号用了单独的字体（比如 Symbol）时再加上编号字体（第二项是编号字号相对段落字号的比例）。
/// 字体都按「第一个本机找得到的」算，和 pdf 实际用的字体一致；docx 只写列表里第一个。
pub fn style_line(s: &Style, cjk: bool, marker: Option<(&str, f32)>) -> LineMetrics {
    let f = s.font_family.as_ref();
    let latin = f
        .and_then(|f| f.latin.as_deref())
        .into_iter()
        .chain(DEFAULT_LATIN.iter().copied());
    let mut fonts = vec![(first_found(latin).unwrap_or(FALLBACK), 1.0)];
    if cjk {
        let ea = f
            .and_then(|f| f.east_asia.as_deref())
            .into_iter()
            .chain(DEFAULT_EAST_ASIA.iter().copied());
        if let Some(m) = first_found(ea) {
            fonts.push((m, 1.0));
        }
    }
    if let Some((name, k)) = marker
        && let Some(m) = first_found([name])
    {
        fonts.push((m, k));
    }
    LineMetrics::of(&fonts)
}

/// 一行的行盒：基线以上 top、以下 bottom，各是「em 部分 × 字号 + pt 部分」。
/// em 部分跟着各 run 自己的字号走，一行里有大字时跟 Word 一样撑高。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    pub top_em: f32,
    pub top_pt: f32,
    pub bottom_em: f32,
    pub bottom_pt: f32,
}

impl LineBox {
    pub fn top(&self, fs: f32) -> f32 {
        self.top_em * fs + self.top_pt
    }

    pub fn bottom(&self, fs: f32) -> f32 {
        self.bottom_em * fs + self.bottom_pt
    }

    /// 行高（相邻两行基线的距离），pt。
    pub fn height(&self, fs: f32) -> f32 {
        self.top(fs) + self.bottom(fs)
    }

    /// 写成 Typst text 的 top-edge、bottom-edge。
    pub fn typst(&self) -> String {
        format!(
            "top-edge: {:.4}em + {:.3}pt, bottom-edge: {:.4}em + {:.3}pt",
            self.top_em, self.top_pt, -self.bottom_em, -self.bottom_pt
        )
    }
}

/// 按 Word 的行距规则算行盒。`fs` 是段落字号（pt），`None` 是 Word 的单倍。
pub fn line_box(m: &LineMetrics, l: Option<LineHeight>, fs: f32) -> LineBox {
    let natural = LineBox {
        top_em: m.asc,
        top_pt: 0.0,
        bottom_em: m.desc,
        bottom_pt: 0.0,
    };
    // 至少 v：多出来的加在基线上面
    let at_least = |v: f32| LineBox {
        top_pt: (v - m.natural() * fs).max(0.0),
        ..natural
    };
    match l {
        None => natural,
        // auto 倍数：多出来的按最大单倍算，加在基线下面
        Some(LineHeight::FontMultiple(x)) => LineBox {
            bottom_em: m.desc + (x - 1.0) * m.max_single,
            ..natural
        },
        // CSS 倍数在 docx 里写成「至少 字号 × 倍数」
        Some(LineHeight::Multiple(x)) => at_least(fs * x),
        Some(LineHeight::AtLeast(v)) => at_least(v.pt(fs, 0.0)),
        // 固定值：基线在 0.8 × 行高处，跟字体无关
        Some(LineHeight::Exact(v)) => {
            let h = v.pt(fs, 0.0);
            LineBox {
                top_em: 0.0,
                top_pt: 0.8 * h,
                bottom_em: 0.0,
                bottom_pt: 0.2 * h,
            }
        }
    }
}

/// 文字里有没有 Word 会用东亚字体排的字（中日韩文字、假名、谚文、全角标点）。
pub fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x3000..=0x30FF
            | 0x3100..=0x31FF
            | 0x3200..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF)
    })
}

/// 本机找不到字体文件时，按名字认几个常见的中日韩字体，给 fontTable 写 charset 用。
pub fn known_charset(name: &str) -> Option<&'static str> {
    let n = name.trim().to_lowercase();
    const ZH_CN: &[&str] = &[
        "pingfang sc",
        "hiragino sans gb",
        "microsoft yahei",
        "微软雅黑",
        "simsun",
        "宋体",
        "nsimsun",
        "simhei",
        "黑体",
        "kaiti",
        "楷体",
        "fangsong",
        "仿宋",
        "dengxian",
        "等线",
        "stsong",
        "stheiti",
        "stkaiti",
        "stfangsong",
        "stxihei",
        "songti sc",
        "heiti sc",
        "kaiti sc",
        "noto sans cjk sc",
        "noto serif cjk sc",
        "noto sans sc",
        "noto serif sc",
        "source han sans sc",
        "source han serif sc",
        "source han sans cn",
        "source han serif cn",
        "思源黑体",
        "思源宋体",
        "misans",
        "harmonyos sans sc",
        "alibaba puhuiti",
        "lxgw wenkai",
    ];
    const JA: &[&str] = &[
        "ms mincho",
        "ms gothic",
        "ms pgothic",
        "ms pmincho",
        "meiryo",
        "yu gothic",
        "yu mincho",
        "biz udgothic",
        "biz udmincho",
        "hiragino sans",
        "hiragino mincho pron",
        "noto sans cjk jp",
        "noto sans jp",
    ];
    const KO: &[&str] = &[
        "malgun gothic",
        "batang",
        "gulim",
        "dotum",
        "apple sd gothic neo",
        "noto sans cjk kr",
        "noto sans kr",
    ];
    const ZH_TW: &[&str] = &[
        "microsoft jhenghei",
        "微軟正黑體",
        "pmingliu",
        "mingliu",
        "pingfang tc",
        "pingfang hk",
        "noto sans cjk tc",
        "noto sans cjk hk",
        "noto sans tc",
    ];
    if ZH_CN.contains(&n.as_str()) {
        Some("86")
    } else if JA.contains(&n.as_str()) {
        Some("80")
    } else if KO.contains(&n.as_str()) {
        Some("81")
    } else if ZH_TW.contains(&n.as_str()) {
        Some("88")
    } else {
        None
    }
}

/// docx fontTable 里这个字体该写的 charset：本机有就按字体的代码页位，没有就按名字认，认不出是 None。
pub fn font_charset(name: &str) -> Option<&'static str> {
    if name.trim().eq_ignore_ascii_case("symbol") {
        return Some("02");
    }
    match family_raw(name) {
        Some(r) => Some(r.charset()),
        None => known_charset(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(upm: f32, win: (f32, f32), hhea: (f32, f32, f32), cp: u32) -> RawMetrics {
        RawMetrics {
            upm,
            hhea_asc: hhea.0,
            hhea_desc: hhea.1,
            hhea_gap: hhea.2,
            os2: Some(Os2 {
                use_typo: false,
                typo_asc: win.0,
                typo_desc: -win.1,
                typo_gap: 0.0,
                win_asc: win.0,
                win_desc: win.1,
                code_pages: cp,
                ..Default::default()
            }),
        }
    }

    const SC: u32 = 1 | (1 << 18);

    #[test]
    fn script_metrics_from_os2() {
        let mut r = raw(256.0, (220.0, 36.0), (220.0, -36.0, 36.0), SC);
        let o = r.os2.as_mut().unwrap();
        o.sup_size = 128.0;
        o.sup_offset = 128.0;
        o.sub_size = 128.0;
        o.sub_offset = 32.0;
        assert_eq!(script_from(Some(r), true), (0.5, 0.5));
        assert_eq!(script_from(Some(r), false), (0.5, 0.125));
        // 没有 OS/2 值时用 Typst 的默认
        let r0 = raw(1000.0, (800.0, 200.0), (800.0, -200.0, 0.0), 0);
        assert_eq!(script_from(Some(r0), true), (0.6, 0.5));
        assert_eq!(script_from(None, false), (0.6, 0.2));
    }

    #[test]
    fn simsun_rounds_e_in_font_units() {
        // 宋体：upm 256，win 220 / 36；0.15 × 256 = 38.4 → 38
        let m = raw(256.0, (220.0, 36.0), (220.0, -36.0, 36.0), SC)
            .word()
            .unwrap();
        assert!((m.single() - 332.0 / 256.0).abs() < 1e-6, "{}", m.single());
        assert!((m.single() - 1.29688).abs() < 1e-5);
        assert!(m.east_asian);
    }

    #[test]
    fn yahei_is_1_3_times_win() {
        // 微软雅黑：upm 2048，win 2167 / 536，0.15 × 2703 = 405.45 → 405
        let m = raw(2048.0, (2167.0, 536.0), (2167.0, -536.0, 0.0), SC)
            .word()
            .unwrap();
        assert!((m.single() - 1.71533).abs() < 1e-4, "{}", m.single());
        assert!((m.asc - (2167.0 + 405.0) / 2048.0).abs() < 1e-6);
    }

    #[test]
    fn rounding_table_from_report() {
        // 报告 K 节：upm 100 的合成字体
        for (a, d, want) in [
            (93.0, 20.0, 1.47),
            (97.0, 20.0, 1.53),
            (90.0, 27.0, 1.53),
            (99.0, 20.0, 1.55),
            (101.0, 20.0, 1.57),
            (55.0, 15.0, 0.92),
            (90.0, 20.0, 1.44),
            (125.0, 25.0, 1.96),
            (110.0, 20.0, 1.70),
        ] {
            let m = raw(100.0, (a, d), (a, -d, 0.0), SC).word().unwrap();
            assert!((m.single() - want).abs() < 1e-5, "{a}/{d}: {}", m.single());
        }
    }

    #[test]
    fn east_asian_ignores_hhea() {
        // Yu Gothic 那种：hhea 比 win 高，东亚字体只看 win
        let m = raw(1000.0, (900.0, 200.0), (1300.0, -300.0, 0.0), 1 << 17)
            .word()
            .unwrap();
        assert!((m.single() - (1100.0 + 2.0 * 165.0) / 1000.0).abs() < 1e-6);
    }

    #[test]
    fn latin_takes_max_of_win_and_hhea_with_lead_above() {
        // Arial：win 1854 / 434，hhea 1854 / −434 + 67
        let m = raw(2048.0, (1854.0, 434.0), (1854.0, -434.0, 67.0), 1)
            .word()
            .unwrap();
        assert!((m.single() - 1.14990).abs() < 1e-4, "{}", m.single());
        assert!((m.desc - 434.0 / 2048.0).abs() < 1e-6, "leading 加在上面");
        // Roboto：win 比 hhea 大
        let m = raw(2048.0, (1946.0, 512.0), (1900.0, -500.0, 0.0), 1)
            .word()
            .unwrap();
        assert!((m.single() - 1.2002).abs() < 1e-4);
    }

    #[test]
    fn use_typo_metrics_bit7() {
        let mut r = raw(1000.0, (1400.0, 300.0), (1000.0, -100.0, 0.0), 1);
        let o = r.os2.as_mut().unwrap();
        o.use_typo = true;
        o.typo_asc = 800.0;
        o.typo_desc = -200.0;
        o.typo_gap = 500.0;
        let m = r.word().unwrap();
        assert!((m.single() - 1.5).abs() < 1e-6);
        assert!((m.asc - 1.3).abs() < 1e-6, "typoLineGap 加在上面");
        // 东亚字体开 bit7：1.3 乘 typo 全高（含 gap）。EAb7g3：typo 0.8 / 0.2 + gap 0.3
        let mut r = raw(2048.0, (2785.0, 696.0), (2253.0, -0.0, 0.0), SC);
        let o = r.os2.as_mut().unwrap();
        o.use_typo = true;
        o.typo_asc = 1638.0;
        o.typo_desc = -410.0;
        o.typo_gap = 614.0;
        let m = r.word().unwrap();
        assert!((m.single() - 1.6895).abs() < 5e-4, "{}", m.single());
    }

    #[test]
    fn charset_follows_code_page_bits() {
        let c = |cp| raw(1000.0, (800.0, 200.0), (800.0, -200.0, 0.0), cp).charset();
        assert_eq!(c(SC), "86");
        assert_eq!(c(1 | (1 << 17)), "80");
        assert_eq!(c(1 | (1 << 19)), "81");
        assert_eq!(c(1 | (1 << 20)), "88");
        assert_eq!(c(1 | (1 << 17) | (1 << 18)), "86");
        assert_eq!(c(1 | (1 << 21)), "00");
        assert_eq!(c(1), "00");
        assert_eq!(known_charset("Microsoft YaHei"), Some("86"));
        assert_eq!(known_charset("Calibri"), None);
    }

    fn metrics(asc: f32, desc: f32) -> FontMetrics {
        FontMetrics {
            asc,
            desc,
            east_asian: false,
        }
    }

    #[test]
    fn mixed_fonts_take_max_ascent_and_descent() {
        // 报告 G：P（1.20 / 0.10）+ Q（0.60 / 0.60）= 1.80，不是 max(单倍) 1.30
        let m = LineMetrics::of(&[(metrics(1.2, 0.1), 1.0), (metrics(0.6, 0.6), 1.0)]);
        assert!((m.natural() - 1.8).abs() < 1e-6);
        assert!((m.max_single - 1.3).abs() < 1e-6);
        // 倍数部分按最大单倍：1.5 倍 = 36 + 0.5 × 26 = 49（20pt）
        let b = line_box(&m, Some(LineHeight::FontMultiple(1.5)), 20.0);
        assert!((b.height(20.0) - 49.0).abs() < 1e-4);
        assert!((b.top(20.0) - 24.0).abs() < 1e-4, "倍数多出来的加在下面");
    }

    #[test]
    fn exact_and_at_least_baselines() {
        use crate::style::Len;
        // Calibri 11：asc 0.9521，desc 0.2686
        let m = LineMetrics::of(&[(metrics(0.9521, 0.2686), 1.0)]);
        let b = line_box(&m, Some(LineHeight::Exact(Len::Pt(20.0))), 11.0);
        assert!((b.height(11.0) - 20.0).abs() < 1e-4);
        assert!((b.top(11.0) - 16.0).abs() < 1e-4);
        // atLeast 20：基线离行顶 = 20 − descent
        let b = line_box(&m, Some(LineHeight::AtLeast(Len::Pt(20.0))), 11.0);
        assert!((b.height(11.0) - 20.0).abs() < 1e-4);
        assert!((b.top(11.0) - (20.0 - 0.2686 * 11.0)).abs() < 1e-3);
        // 设定值比自然行高小时就是自然行高
        let b = line_box(&m, Some(LineHeight::AtLeast(Len::Pt(10.0))), 11.0);
        assert!((b.height(11.0) - 1.2207 * 11.0).abs() < 1e-3);
        // CSS 倍数 = 至少 字号 × 倍数
        let b = line_box(&m, Some(LineHeight::Multiple(1.5)), 11.0);
        assert!((b.height(11.0) - 16.5).abs() < 1e-4);
        let b = line_box(&m, Some(LineHeight::Multiple(1.0)), 11.0);
        assert!((b.height(11.0) - 1.2207 * 11.0).abs() < 1e-3);
    }

    #[test]
    fn cjk_detection() {
        assert!(has_cjk("abc 中文"));
        assert!(has_cjk("かな"));
        assert!(has_cjk("한국어"));
        assert!(has_cjk("，"));
        assert!(!has_cjk("plain ascii — é"));
    }
}
