//! 行高按 Windows Word 的规则算（src/render/metrics.rs）。期望值是 2026-11-18 在 Windows Word 2024 上
//! 导出 PDF 量出来的（见 tests/fonts/README.md）；PDF 基线在 0.12pt 的格子上，所以容差 0.002 em。

use document_render::render::metrics::{FontMetrics, LineMetrics, RawMetrics, line_box};
use document_render::style::{Len, LineHeight};

fn font(name: &str) -> RawMetrics {
    let path = format!("{}/tests/fonts/{name}.ttf", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap();
    let face = ttf_parser::Face::parse(&data, 0).unwrap();
    RawMetrics::from_face(&face)
}

fn single(name: &str) -> f32 {
    font(name).word().unwrap().single()
}

fn close(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() < 0.002,
        "{what}：算出 {got}，Word 实测 {want}"
    );
}

#[test]
fn latin_rules_match_word() {
    close(single("LHSynA"), 1.7006, "bit7 关，win 最大");
    close(single("LHSynB"), 1.4004, "bit7 开，只看 typo");
    close(single("LHSynC"), 1.7002, "bit7 关，hhea 最大");
    close(single("LHSynE"), 1.3996, "hhea 含 lineGap");
    close(single("LHSynF"), 1.5003, "typo 含 typoLineGap");
    close(single("LH5CP21"), 1.2004, "只有 bit21 不算东亚");
    assert!(!font("LH5CP21").word().unwrap().east_asian);
}

#[test]
fn east_asian_rules_match_word() {
    close(single("LH5R256x220x36"), 1.2969, "宋体那种 upm 256");
    close(
        single("LH5R100x93x20"),
        1.4700,
        "upm 100，0.15T = 16.95 → 17",
    );
    close(single("LH5CP17"), 1.5604, "只开 bit17 也乘 1.3");
    close(
        single("LH5EAb7g3"),
        1.6887,
        "东亚 + bit7：1.3 × typo 全高含 gap",
    );
    assert_eq!(font("LH5CP17").charset(), "80");
    assert_eq!(font("LH5R256x220x36").charset(), "86");
    assert_eq!(font("LH5CP21").charset(), "00");
}

#[test]
fn external_leading_goes_above_baseline() {
    // MXhgap（单独 1.40）+ MY（asc 0.4 / desc 0.8）：加在上面是 2.0，Word 实测 2.0006
    let x = font("LH5MXhgap").word().unwrap();
    let my = FontMetrics {
        asc: 0.4,
        desc: 0.8,
        east_asian: false,
    };
    let m = LineMetrics::of(&[(x, 1.0), (my, 1.0)]);
    close(m.natural(), 2.0006, "MXhgap + MY");
    // 1.5 倍：40 + 0.5 × 28 = 54pt（20pt 字）
    let b = line_box(&m, Some(LineHeight::FontMultiple(1.5)), 20.0);
    assert!((b.height(20.0) - 54.0).abs() < 0.05, "{}", b.height(20.0));
}

#[test]
fn exact_baseline_at_80_percent() {
    let m = LineMetrics::of(&[(single_metrics(), 1.0)]);
    for v in [8.0, 15.0, 30.0] {
        let b = line_box(&m, Some(LineHeight::Exact(Len::Pt(v))), 11.0);
        assert!((b.top(11.0) - 0.8 * v).abs() < 1e-4);
        assert!((b.height(11.0) - v).abs() < 1e-4);
    }
}

fn single_metrics() -> FontMetrics {
    font("LHSynA").word().unwrap()
}

/// MiSans：hhea = win = typo = 1.326，有 bit18，Windows Word 实测 1.7243。本机没装就跳过。
#[test]
fn misans_if_installed() {
    let Some(r) = document_render::render::metrics::family_raw("MiSans") else {
        eprintln!("本机没有 MiSans，跳过");
        return;
    };
    close(r.word().unwrap().single(), 1.7243, "MiSans");
    assert_eq!(r.charset(), "86");
}
