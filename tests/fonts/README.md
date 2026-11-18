# 测试用字体

行高测试用的合成字体，从 DejaVu Sans 取子集后改了 hhea、OS/2 的度量和代码页位，改了名字（Bitstream Vera / DejaVu 的许可要求改过的字体换名）。2026-11-18 在 Windows Word 2024 上实测过的就是这几个文件，期望值见各测试里的注释。

| 文件 | 改了什么 | Windows Word 实测单倍（em） |
|---|---|---|
| LHSynA | hhea 1.10、typo 1.40、win 1.70，bit7 关 | 1.7006（取 win） |
| LHSynB | 同上，bit7 开 | 1.4004（取 typo） |
| LHSynC | hhea 1.70、typo 1.40、win 1.10，bit7 关 | 1.7002（取 hhea） |
| LHSynE | hhea 1.10 + lineGap 0.30、win 1.20 | 1.3996（hhea 含 lineGap） |
| LHSynF | typo 1.00 + typoLineGap 0.50，bit7 开 | 1.5003 |
| LH5MXhgap | win 0.8 / 0.2，hhea lineGap 0.4 | 1.4004，leading 加在基线上面 |
| LH5CP17 | win 1.20，只开 bit17（日文） | 1.5604（charset 80，乘 1.3） |
| LH5CP21 | win 1.20，只开 bit21（Johab） | 1.2004（不乘） |
| LH5R256x220x36 | upm 256，win 220 / 36，bit18 | 1.2969（宋体那种取整） |
| LH5R100x93x20 | upm 100，win 93 / 20，bit18 | 1.4700 |
| LH5EAb7g3 | bit18 + bit7，typo 0.8 / 0.2 + gap 0.3 | 1.6887 |
