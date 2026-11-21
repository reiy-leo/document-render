# typst-layout 0.15.1（document-render 补丁版）

来源：crates.io 上的 `typst-layout` 0.15.1 原样拷进来（上游 git 提交 9dfd3a08500b7896045f907433cf7b4b02434fad，
仓库 https://github.com/typst/typst 的 `crates/typst-layout`），许可证 Apache-2.0，见 `LICENSE`。
document-render 的 `Cargo.toml` 用 `[patch.crates-io]` 指到这里。

## 改了什么

加了一个开关 `typst_layout::set_cjk_punct_compression(on)`，默认开（和上游一样）。document-render 排 pdf
前把它关掉，让中文标点都占整字宽，和 Word 默认的「不压缩」一致。关掉后：

1. `inline/shaping.rs` 的 `base_adjustability`：中文标点不再有可收缩量，两端对齐时不会被挤窄。
2. `inline/shaping.rs` 的 `calculate_adjustability`：不做相邻标点的半宽调整（「。」」「），」这类）。
3. `inline/line.rs` 的 `adjust_cj_at_line_start`：行首的「（「《“」等不收左半宽。
4. `inline/line.rs` 的 `adjust_cj_at_line_end`：行尾的「，。、」」等不收右半宽。

另外 `inline/mod.rs`、`lib.rs` 把开关导出来。改动处都标了 `document-render patch`。
开关是进程级的，要在第一次排版前设好；排版结果有缓存，同一进程里中途切换不保证生效。

### 第二个开关：中西文间距照 Word

`typst_layout::set_word_autospace(on)`，默认关（和上游一样）。document-render 排 pdf 前打开，让
`cjk-latin-spacing` 照 Word 2024 的自动间距（`w:autoSpaceDE`、`w:autoSpaceDN`）排。打开后：

1. `inline/shaping.rs`：加开关和两个判断字符的函数。`is_word_autospace_ea` 是中文这一边（汉字、
   假名、韩文，不含 U+3000～U+303F 里的 `々` `〇` 等）；`is_word_autospace_latin` 是西文这一边（ASCII
   数字，拉丁、希腊、西里尔字母，`µ`；不含全角字母和罗马数字）。上游的 `is_cj_script`、
   `is_letter_or_number` 没动，开关关着时照用。
2. `inline/prepare.rs` 的 `add_cjk_latin_spacing`：用上面两个函数判断加不加；每处空隙另给 1/4 em 的
   可拉伸量（stretchability），两端对齐时 `commit` 先用它，再把剩下的分给汉字之间，和 Word 一样。
3. `inline/line.rs` 的 `adjust_cj_at_line_boundaries`、`adjust_cj_at_line_start`、`adjust_cj_at_line_end`：
   行首行尾去掉空隙时用同样的字符判断，可拉伸量一起去掉。行尾那处按 1/4 em 去（上游用「字宽大于 1 em」
   判断，半角假名会漏）。
4. `inline/linebreak.rs` 的 `linebreak_simple`：两端对齐的段落里，一行算不算放得下，要减去
   `word_autospace_allowance` 算出的可压缩量（Word 为多放一个字压中西文空隙的实测上限，见函数注释和
   document-render 的 `docs/STYLE.md`「中西文间距」）。压多少由 `commit` 按原来的收缩量（每处 1/8 em）
   平均分。

实测依据：document-render 仓库外的对照记录（Word 2024 导出 pdf 逐字量位置），结论写在 `docs/STYLE.md`。

没删上游代码，也没用 `region: "TW"`（它会让带 `locl` 的字体换成台湾字形）。

## 跟上游

换 Typst 版本时：拷新版 crate 源码进来，把上面两个开关的改动和导出照着再加一遍（`git diff` 这个目录的首个提交就是全部改动），
或者上游有了正式参数后删掉这个目录和 `[patch.crates-io]`。
