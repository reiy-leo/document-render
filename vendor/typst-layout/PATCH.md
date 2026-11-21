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

没删上游代码，也没用 `region: "TW"`（它会让带 `locl` 的字体换成台湾字形）。

## 跟上游

换 Typst 版本时：拷新版 crate 源码进来，把上面四处和导出照着再加一遍（`git diff` 这个目录的首个提交就是全部改动），
或者上游有了正式参数后删掉这个目录和 `[patch.crates-io]`。
