//! 三个渲染器共用的计算：列表编号、表格部位、警告去重。

use std::collections::HashSet;

use crate::{
    components::{Cell, Inline, ListItem, ListProps, RowHeight, Run, TableLook, TableProps},
    doc::Node,
    style::{Border, ColWidth, Edge, Len, ListStyleType, Style},
};

/// 同一类警告只报一次。
#[derive(Default)]
pub struct Warn {
    pub list: Vec<String>,
    seen: HashSet<String>,
}

impl Warn {
    pub fn push(&mut self, msg: String) {
        self.list.push(msg);
    }
    /// 同一个 key 只报一次。
    pub fn once(&mut self, key: &str, msg: String) {
        if self.seen.insert(key.to_string()) {
            self.list.push(msg);
        }
    }

    /// 公式转不了：同一个元素里的同一个公式只报一次。
    pub fn math_failed(&mut self, format: &str, id: &str, latex: &str, why: &str) {
        self.once(
            &format!("math:{format}:{id}:{latex}"),
            format!(
                "{format}：元素 `{id}` 里的公式 `{latex}` 转不了（{why}），改成显示 LaTeX 原文"
            ),
        );
    }

    /// 公式里有认不出的结构，退成了纯文字。
    pub fn math_partial(&mut self, format: &str, id: &str, latex: &str, tags: &[String]) {
        if tags.is_empty() {
            return;
        }
        self.once(
            &format!("mathpart:{format}:{id}:{latex}"),
            format!(
                "{format}：元素 `{id}` 里的公式 `{latex}` 有 {} 排不了，这部分改成纯文字",
                tags.join("、")
            ),
        );
    }

    /// 某个格式不支持某个字段：每个字段只报一次。
    pub fn unsupported(&mut self, format: &str, field: &str, id: &str) {
        let key = format!("{format}:{field}");
        if self.seen.insert(key) {
            self.list.push(format!(
                "{format} 表达不了样式 `{field}`（第一次出现在 `{id}`），已忽略"
            ));
        }
    }
}

// ---------- 列表 ----------

/// 一级列表的样式。
///
/// 规则：元素样式管所有级别，但「编号长什么样」的字段（`listStyleType`、`listMarkerFormat`、
/// `listMarkerImage`、`listMarkerFont`、`listStart`）写在元素样式上时只算第一级；
/// 第二级起只认自己的 `levelN` 部位，没写就按级别取默认。这样元素样式上写一个 `decimal`，
/// 下级不会也变成数字，更不会把 `%1.` 这类模板套到下级上。
pub fn list_level_style(n: &Node, level: usize, ordered: bool) -> Style {
    let mut s = n.style.clone();
    if level > 1 {
        clear_marker_fields(&mut s);
    }
    s.merge(&n.part(&format!("level{level}")));
    if s.list_style_type.is_none() {
        s.list_style_type = Some(default_type(level, ordered));
    }
    s
}

/// 只管第一级的那几个列表字段清空。
pub fn clear_marker_fields(s: &mut Style) {
    s.list_style_type = None;
    s.list_marker_format = None;
    s.list_marker_image = None;
    s.list_marker_font = None;
    s.list_start = None;
}

pub fn default_type(level: usize, ordered: bool) -> ListStyleType {
    let i = (level - 1) % 3;
    if ordered {
        [
            ListStyleType::Decimal,
            ListStyleType::LowerAlpha,
            ListStyleType::LowerRoman,
        ][i]
            .clone()
    } else {
        [
            ListStyleType::Disc,
            ListStyleType::Circle,
            ListStyleType::Square,
        ][i]
            .clone()
    }
}

/// 列表展开成一行一项。
pub struct ListLine<'a> {
    pub level: usize,
    pub item: &'a ListItem,
    /// 编号或符号的文字（已按 listMarkerFormat 拼好）
    pub marker: String,
    pub style: Style,
    /// 这一项属于哪一串子列表：同一串的项编号相同，第一级是 0
    pub group: usize,
}

/// 把嵌套列表摊平，算好每项的编号。`continueFrom` 接续时第一级从 `start_at` 起数。
pub fn flatten_list<'a>(n: &Node, p: &'a ListProps) -> Vec<ListLine<'a>> {
    let mut out = Vec::new();
    let mut w = Walk {
        counters: Vec::new(),
        types: Vec::new(),
        groups: 0,
    };
    w.walk(n, &p.items, p.ordered, 1, p.start_at, 0, &mut out);
    out
}

/// 第一级的起始号：接续时是算好的号，不然是 listStart，都没有是 1。
pub fn list_first_start(n: &Node, p: &ListProps) -> u32 {
    p.start_at
        .or(list_level_style(n, 1, p.ordered).list_start)
        .unwrap_or(1)
}

struct Walk {
    counters: Vec<u32>,
    types: Vec<ListStyleType>,
    groups: usize,
}

impl Walk {
    #[allow(clippy::too_many_arguments)]
    fn walk<'a>(
        &mut self,
        n: &Node,
        items: &'a [ListItem],
        ordered: bool,
        level: usize,
        start_at: Option<u32>,
        group: usize,
        out: &mut Vec<ListLine<'a>>,
    ) {
        let style = list_level_style(n, level, ordered);
        let ty = style.list_style_type.clone().unwrap();
        let start = start_at.or(style.list_start).unwrap_or(1);
        self.counters.truncate(level - 1);
        self.types.truncate(level - 1);
        self.counters.push(start.saturating_sub(1));
        self.types.push(ty.clone());
        for it in items {
            self.counters[level - 1] += 1;
            // 符号类的级别不用 listMarkerFormat，只出符号
            let marker = match ty.bullet().filter(|_| !ty.ordered()) {
                Some(b) => b,
                None => {
                    let f = style
                        .list_marker_format
                        .clone()
                        .unwrap_or_else(|| format!("%{level}."));
                    fill_format(&f, &self.counters, &self.types, legal(&ty))
                }
            };
            out.push(ListLine {
                level,
                item: it,
                marker,
                style: style.clone(),
                group,
            });
            if !it.children().is_empty() {
                let sub_ordered = it.ordered().unwrap_or(ordered);
                self.groups += 1;
                let g = self.groups;
                self.walk(n, it.children(), sub_ordered, level + 1, None, g, out);
                self.counters.truncate(level);
                self.types.truncate(level);
            }
        }
    }
}

/// 这一级列表项文字的左边位置（离列表左边），pt：listIndent × 级数。
/// 编号在它左边 listHanging 处；项里的续块（children）左边对齐到这里。
pub fn item_text_indent(s: &Style, level: usize) -> f32 {
    let fs = s.size_pt();
    s.list_indent.map(|x| x.pt(fs, 0.0)).unwrap_or(18.0) * level as f32
}

/// 列表项续块：在块自己的左缩进上加 `indent` pt，对齐到这一项的文字。
pub fn indented_block(b: &Node, indent: f32) -> Node {
    let mut b = b.clone();
    let fs = b.style.size_pt();
    let ml = b.style.margin_left.map_or(0.0, |l| l.pt(fs, 0.0));
    b.style.margin_left = Some(Len::Pt(ml + indent));
    b
}

/// 相邻两项之间按 itemSpacing 算的距离：第 i 个是第 i 项和第 i+1 项之间。
/// 用两项里层级深的那一项的 itemSpacing（子列表首项和父项之间、末项和下一个父项之间都算子列表那一层）；
/// 那一层没写 itemSpacing 时是 None，照旧用 item 部位的段前段后。
pub fn item_gaps(lines: &[ListLine]) -> Vec<Option<Len>> {
    lines
        .windows(2)
        .map(|w| {
            let deeper = if w[1].level >= w[0].level {
                &w[1]
            } else {
                &w[0]
            };
            deeper.style.item_spacing
        })
        .collect()
}

/// 这一级用阿拉伯数字时，`%1.%2` 里上几级的编号也写成阿拉伯数字（Word 叫 isLgl），
/// 上一级是「一、」，这一级出来是「2.1」而不是「二.1」。
pub fn legal(ty: &ListStyleType) -> bool {
    matches!(
        ty,
        ListStyleType::Decimal | ListStyleType::DecimalLeadingZero
    )
}

/// `%1.%2` 里的 %k 换成第 k 级的编号（和 Word 的 lvlText 一样，%1 是第一级）。
/// `legal` 为真时上几级一律用这一级的数字写法。%k 指向比本级还深的级别时没有编号可填，直接去掉，
/// 不会把模板文字原样留下。
pub fn fill_format(f: &str, counters: &[u32], types: &[ListStyleType], legal: bool) -> String {
    let own = types.last();
    let mut out = String::new();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(k) = chars
                .peek()
                .and_then(|d| d.to_digit(10))
                .filter(|k| *k >= 1)
        {
            chars.next();
            let k = k as usize;
            if k <= counters.len() {
                let ty = match own {
                    Some(t) if legal => t,
                    _ => &types[k - 1],
                };
                out.push_str(&ty.number(counters[k - 1]));
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// 把模板里指向比 `level` 深的 %k 去掉（Word 的 lvlText 只能引用本级和上级）。
pub fn clamp_format(f: &str, level: usize) -> String {
    let mut out = String::new();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(k) = chars
                .peek()
                .and_then(|d| d.to_digit(10))
                .filter(|k| *k >= 1)
        {
            chars.next();
            if k as usize <= level {
                out.push('%');
                out.push_str(&k.to_string());
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Word lvlText 的写法：符号级别直接写符号（不用 listMarkerFormat），编号级别用模板，
/// 没写模板时是 `%N.`。
pub fn word_level_text(style: &Style, level: usize) -> String {
    let ty = style.list_style_type.clone().unwrap_or(ListStyleType::Disc);
    match ty.bullet().filter(|_| !ty.ordered()) {
        Some(b) => b,
        None => match &style.list_marker_format {
            Some(f) => clamp_format(f, level),
            None => format!("%{level}."),
        },
    }
}

// ---------- 表格 ----------

/// 网格上的一个位置：属于哪一格（左上角的行列号）。被合并掉的位置 origin 指向左上角那格。
#[derive(Clone, Copy)]
pub struct Slot<'a> {
    pub origin: (usize, usize),
    pub row_span: usize,
    pub col_span: usize,
    /// None 是补齐出来的空格
    pub cell: Option<&'a Cell>,
}

impl Slot<'_> {
    pub fn text(&self) -> &Inline {
        static EMPTY: Inline = Inline::Plain(String::new());
        self.cell.map(Cell::text).unwrap_or(&EMPTY)
    }
}

/// 表格展开成网格：合并单元格按 HTML 的规则摆（每行只写从这一行开始的格子，
/// 被上面 rowSpan 占掉的位置跳过），行数按写的行数，列数取最宽的一行。
pub struct Grid<'a> {
    pub rows: usize,
    pub cols: usize,
    pub has_header: bool,
    pub slots: Vec<Vec<Slot<'a>>>,
    /// (行号, 实际占的列数)：和列数对不上、补了空格的行，给渲染器报警告
    pub ragged: Vec<(usize, usize)>,
}

impl<'a> Grid<'a> {
    pub fn of(p: &'a TableProps) -> Grid<'a> {
        let has_header = !p.columns.is_empty();
        let src: Vec<&'a [Cell]> = std::iter::once(p.columns.as_slice())
            .filter(|_| has_header)
            .chain(p.rows.iter().map(Vec::as_slice))
            .collect();
        let rows = src.len();
        let mut grid: Vec<Vec<Option<Slot<'a>>>> = vec![Vec::new(); rows];
        for (r, cells) in src.iter().enumerate() {
            let mut c = 0;
            for cell in cells.iter() {
                while grid[r].get(c).is_some_and(Option::is_some) {
                    c += 1;
                }
                let rs = cell.row_span().min(rows - r);
                let cs = cell.col_span();
                let slot = Slot {
                    origin: (r, c),
                    row_span: rs,
                    col_span: cs,
                    cell: Some(cell),
                };
                for row in grid.iter_mut().skip(r).take(rs) {
                    if row.len() < c + cs {
                        row.resize(c + cs, None);
                    }
                    for x in row.iter_mut().skip(c).take(cs) {
                        // 两格重叠时先写的算数
                        if x.is_none() {
                            *x = Some(slot);
                        }
                    }
                }
                c += cs;
            }
        }
        let cols = grid.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut ragged = Vec::new();
        let slots = grid
            .into_iter()
            .enumerate()
            .map(|(r, row)| {
                let used = row.iter().filter(|x| x.is_some()).count();
                if used != cols && rows > 0 {
                    ragged.push((r, used));
                }
                (0..cols)
                    .map(|c| {
                        row.get(c).copied().flatten().unwrap_or(Slot {
                            origin: (r, c),
                            row_span: 1,
                            col_span: 1,
                            cell: None,
                        })
                    })
                    .collect()
            })
            .collect();
        Grid {
            rows,
            cols,
            has_header,
            slots,
            ragged,
        }
    }

    pub fn slot(&self, r: usize, c: usize) -> &Slot<'a> {
        &self.slots[r][c]
    }

    /// 这个位置是不是一格的左上角（要输出的格子）
    pub fn is_origin(&self, r: usize, c: usize) -> bool {
        self.slots[r][c].origin == (r, c)
    }

    /// 补齐的行报警告
    pub fn warn_ragged(&self, warn: &mut Warn, id: &str) {
        for (r, used) in &self.ragged {
            warn.push(format!(
                "表格 `{id}` 第 {} 行占 {used} 列，按 {} 列补齐",
                r + 1,
                self.cols
            ));
        }
    }

    /// 第 r 行的行高
    pub fn row_height(&self, p: &TableProps, r: usize) -> Option<RowHeight> {
        p.row_heights
            .as_ref()
            .and_then(|v| v.get(r).copied().flatten())
    }
}

/// 合并后的一格四条边：上、左取左上角，下取最后一行，右取最后一列。返回 上右下左。
pub fn span_edges(n: &Node, look: &TableLook, g: &Grid, r: usize, c: usize) -> [Option<Border>; 4] {
    let sl = g.slot(r, c);
    let (r1, c1) = (r + sl.row_span - 1, c + sl.col_span - 1);
    let first = cell_edges(n, look, g, r, c);
    if (r1, c1) == (r, c) {
        return first;
    }
    [
        first[0].clone(),
        cell_edges(n, look, g, r, c1)[1].clone(),
        cell_edges(n, look, g, r1, c)[2].clone(),
        first[3].clone(),
    ]
}

/// 各列宽度；没写或个数不对时平均分（个数不对会报警告）。
pub fn col_widths(p: &TableProps, g: &Grid, warn: &mut Warn, id: &str) -> Vec<ColWidth> {
    match &p.widths {
        Some(ws) if ws.len() == g.cols => ws.clone(),
        Some(_) => {
            warn.push(format!(
                "表格 `{id}` 的 widths 个数和列数对不上，改成平均分"
            ));
            vec![ColWidth::Ratio(1.0); g.cols]
        }
        None => vec![ColWidth::Ratio(1.0); g.cols],
    }
}

/// 有比例列时表格撑满（或撑到 style.width）；全是固定列和 auto 列时表格按内容收窄。
pub fn fills_width(ws: &[ColWidth]) -> bool {
    ws.iter().any(|w| matches!(w, ColWidth::Ratio(_)))
}

/// auto 列的自然宽度（pt）：按最宽的格子粗估，加左右 padding。非 auto 列是 0。
pub fn auto_natural(n: &Node, p: &TableProps, g: &Grid, ws: &[ColWidth], pad_pt: f32) -> Vec<f32> {
    let s = &n.style;
    (0..g.cols)
        .map(|c| {
            if ws[c] != ColWidth::Auto {
                return 0.0;
            }
            (0..g.rows)
                .filter(|r| g.is_origin(*r, c) && g.slot(*r, c).col_span == 1)
                .map(|r| {
                    let part = cell_parts(n, &p.look, g, r, c);
                    let mut cs = s.inherited();
                    cs.merge(&part);
                    let side = |e| part.padding_side(e).unwrap_or(pad_pt);
                    estimate_text_width(g.slot(r, c).text(), &cs)
                        + side(Edge::Left)
                        + side(Edge::Right)
                        + 3.0
                })
                .fold(0.0, f32::max)
        })
        .collect()
}

/// 比例列和 auto 列混写时，auto 列按内容撑开，但最多占一份 1fr：
/// 把每个 auto 列当成 1fr 算出一份的宽度，内容比这窄就用内容宽（变成固定列），
/// 比这宽就变成 1fr。这样比例列不会被长内容挤没。没有混写时原样返回。
/// `avail` 是表格可用总宽（pt）。三种格式用同一个结果，排出来的列宽一致。
pub fn resolve_auto(ws: &[ColWidth], natural: &[f32], avail: f32) -> Vec<ColWidth> {
    let ratio: f32 = ws
        .iter()
        .map(|w| if let ColWidth::Ratio(x) = w { *x } else { 0.0 })
        .sum();
    let autos = ws.iter().filter(|w| **w == ColWidth::Auto).count();
    if ratio <= 0.0 || autos == 0 {
        return ws.to_vec();
    }
    let fixed: f32 = ws
        .iter()
        .map(|w| if let ColWidth::Fixed(x) = w { *x } else { 0.0 })
        .sum();
    let share = ((avail - fixed) / (ratio + autos as f32)).max(0.0);
    ws.iter()
        .zip(natural)
        .map(|(w, a)| match w {
            ColWidth::Auto if *a < share => ColWidth::Fixed(*a),
            ColWidth::Auto => ColWidth::Ratio(1.0),
            w => *w,
        })
        .collect()
}

/// 粗估一段文字排成一行要多宽（pt），给 docx 的 auto 列算初始列宽用：
/// 全角字算 1em，半角算 0.6em（宁宽勿窄，窄了 Word 会折行），再加字间距。有换行时取最长的一行。
pub fn estimate_text_width(t: &Inline, cs: &Style) -> f32 {
    let size = cs.size_pt();
    let spacing = cs.letter_spacing.map(|l| l.pt(size, 0.0)).unwrap_or(0.0);
    t.plain_text()
        .split('\n')
        .map(|line| {
            line.chars()
                .map(|c| if (c as u32) < 0x2E80 { 0.6 * size } else { size } + spacing)
                .sum::<f32>()
        })
        .fold(0.0, f32::max)
}

/// 第 r 行第 c 列（都从 0 起，表头算第 0 行）的部位样式，按 Word 的顺序叠：
/// table → cell → 奇偶列 → 奇偶行 → 首末列 → 首末行 → 四角。
pub fn cell_parts(n: &Node, look: &TableLook, g: &Grid, r: usize, c: usize) -> Style {
    let band = n.style.band_size.unwrap_or(1).max(1) as usize;
    let first_row = r == 0 && (look.first_row || g.has_header);
    let last_row = look.last_row && r + 1 == g.rows && g.rows > 1;
    let first_col = look.first_column && c == 0;
    let last_col = look.last_column && c + 1 == g.cols && g.cols > 1;
    let mut s = Style::default();
    s.merge(&n.part("table"));
    s.merge(&n.part("cell"));
    if look.banded_columns && !first_col && !last_col {
        let i = c - usize::from(look.first_column);
        s.merge(&n.part(if (i / band).is_multiple_of(2) {
            "oddColumn"
        } else {
            "evenColumn"
        }));
    }
    if look.banded_rows && !first_row && !last_row {
        let i = r - usize::from(g.has_header || look.first_row);
        s.merge(&n.part(if (i / band).is_multiple_of(2) {
            "oddRow"
        } else {
            "evenRow"
        }));
    }
    if first_col {
        s.merge(&n.part("firstColumn"));
    }
    if last_col {
        s.merge(&n.part("lastColumn"));
    }
    if first_row {
        s.merge(&n.part("firstRow"));
        if g.has_header {
            s.merge(&n.part("header"));
        }
    }
    if last_row {
        s.merge(&n.part("lastRow"));
    }
    let top = r == 0;
    let bottom = r + 1 == g.rows;
    let left = c == 0;
    let right = c + 1 == g.cols;
    for (on, k) in [
        (top && left, "topLeftCell"),
        (top && right, "topRightCell"),
        (bottom && left, "bottomLeftCell"),
        (bottom && right, "bottomRightCell"),
    ] {
        if on {
            s.merge(&n.part(k));
        }
    }
    // 单格自己写的样式最后叠；合并掉的位置用左上角那格的
    if let Some(cs) = g
        .slots
        .get(r)
        .and_then(|x| x.get(c))
        .and_then(|x| x.cell)
        .and_then(Cell::style)
    {
        s.merge(cs);
    }
    s
}

/// 一格四条边，连同相邻格子写的边一起考虑：下面一格写了 borderTop，这一格的下边也用它，
/// 免得两条线叠在一起。返回 上右下左。
pub fn cell_edges(n: &Node, look: &TableLook, g: &Grid, r: usize, c: usize) -> [Option<Border>; 4] {
    let part = cell_parts(n, look, g, r, c);
    let mut b = cell_borders(&n.style, &part, g, r, c);
    // (这一格的边, 邻格位置, 邻格的哪条边)
    let neighbours = [
        (0, r.checked_sub(1).map(|r| (r, c)), Edge::Bottom),
        (1, (c + 1 < g.cols).then_some((r, c + 1)), Edge::Left),
        (2, (r + 1 < g.rows).then_some((r + 1, c)), Edge::Top),
        (3, c.checked_sub(1).map(|c| (r, c)), Edge::Right),
    ];
    for (i, at, theirs) in neighbours {
        let Some((nr, nc)) = at else { continue };
        if part.border_side(Edge::ALL[i]).is_some() {
            continue;
        }
        if let Some(t) = cell_parts(n, look, g, nr, nc).border_side(theirs) {
            b[i] = Some(t.clone());
        }
    }
    b
}

/// 一格四条边：表格的外框和内线，再被格子自己的边框覆盖。返回 上右下左。
pub fn cell_borders(
    table: &Style,
    cell: &Style,
    g: &Grid,
    r: usize,
    c: usize,
) -> [Option<Border>; 4] {
    let outer = |e: Edge| table.border_side(e).cloned();
    let inside_h = table
        .border_inside_h
        .clone()
        .or_else(|| table.border.clone());
    let inside_v = table
        .border_inside_v
        .clone()
        .or_else(|| table.border.clone());
    let base = [
        if r == 0 {
            outer(Edge::Top)
        } else {
            inside_h.clone()
        },
        if c + 1 == g.cols {
            outer(Edge::Right)
        } else {
            inside_v.clone()
        },
        if r + 1 == g.rows {
            outer(Edge::Bottom)
        } else {
            inside_h
        },
        if c == 0 { outer(Edge::Left) } else { inside_v },
    ];
    let mut out = base;
    for (i, e) in Edge::ALL.iter().enumerate() {
        if let Some(b) = cell.border_side(*e) {
            out[i] = Some(b.clone());
        }
    }
    out
}

/// 把文字放进另一个样式环境（表格格子、列表项）：每个 run 的样式按新环境重算。
pub fn rebase_runs(t: &Inline, old: &Style, new: &Style) -> Vec<Run> {
    t.runs()
        .map(|r| {
            let mut r = r.clone();
            r.computed = r.computed.rebase(old, new);
            r
        })
        .collect()
}

/// 目录里的一项。
pub struct TocEntry {
    pub level: u8,
    /// 标题文字；标题有编号时前面带上编号和一个空格（分隔写 none 时不加空格）
    pub text: String,
    pub id: String,
}

/// 文档的标题编号定义（建树时放在根 Document 上）。
pub fn heading_numbering(root: &Node) -> Option<&crate::doc::HeadingNumbering> {
    match &root.component {
        crate::components::Component::Document(d) => d.heading_numbering.as_deref(),
        _ => None,
    }
}

/// 标题编号后面跟的文字：空格分隔是一个空格，none 是空串；制表符在 html、md、目录里也写成空格。
pub fn heading_sep(hn: Option<&crate::doc::HeadingNumbering>, level: u8) -> &'static str {
    match hn.map(|h| h.gap(level)) {
        Some(crate::style::MarkerGap::None) => "",
        _ => " ",
    }
}

/// 按文档顺序收 1 到 depth 级标题，给 docx、html 的目录用（PDF 交给 typst 的 outline）。
pub fn toc_entries(root: &Node, depth: u8) -> Vec<TocEntry> {
    fn walk(
        n: &Node,
        depth: u8,
        hn: Option<&crate::doc::HeadingNumbering>,
        out: &mut Vec<TocEntry>,
    ) {
        if let crate::components::Component::Heading(h) = &n.component
            && h.level <= depth
        {
            let text = match &h.number {
                Some(no) => format!("{no}{}{}", heading_sep(hn, h.level), h.text.plain_text()),
                None => h.text.plain_text(),
            };
            out.push(TocEntry {
                level: h.level,
                text,
                id: n.id.clone(),
            });
        }
        for c in &n.children {
            walk(c, depth, hn, out);
        }
    }
    let mut out = Vec::new();
    walk(root, depth, heading_numbering(root), &mut out);
    out
}

/// 目录标题的样式：目录的可继承字段加 title 部位。
pub fn toc_title_style(n: &Node) -> Style {
    let mut s = n.style.inherited();
    s.merge(&n.part("title"));
    s
}

/// 目录项的样式：目录的可继承字段加 entry 部位，再按级缩进 listIndent × (级数 - 1)。
pub fn toc_entry_style(n: &Node, level: u8) -> Style {
    let mut s = n.style.inherited();
    s.merge(&n.part("entry"));
    let fs = s.size_pt();
    let step = s.list_indent.map(|x| x.pt(fs, 0.0)).unwrap_or(fs * 1.5);
    let base = s.margin_left.map(|x| x.pt(fs, 0.0)).unwrap_or(0.0);
    s.margin_left = Some(crate::style::Len::Pt(base + step * (level as f32 - 1.0)));
    s.list_indent = None;
    s
}

// ---------- 图案边框 ----------

/// CSS 和 Typst 画不出来的线型，用一小块 SVG 图案沿边铺开。HTML 当背景贴，PDF 当 tiling 填充，
/// 两边用的是同一份 SVG，长得一样。
///
/// 图案按「横边」画：x 沿着边，y 是线宽方向（0 到 `width`）。竖边把 x、y 对调。
/// y = 0 一律在上边或左边，不随边翻转，这样相邻两格共用的线叠在一起也是同一个图案。
pub struct Tile {
    /// 图案沿边方向的长度，pt
    pub period: f32,
    /// 线宽，pt
    pub width: f32,
    body: String,
    /// 沿边不断的线（双线、三线、粗细线、立体线）：每条是 (离上边或左边的距离, 粗细, 颜色)。
    /// PDF 按这个拆成几条实线画（块套几层 stroke，表格格子用硬边渐变），免得 tiling 拼缝；虚线、波浪线这里是空的。
    pub stripes: Vec<(f32, f32, String)>,
}

impl Tile {
    /// 铺这条边用的 SVG。`vertical` 为真时是左右两条竖边。
    pub fn svg(&self, vertical: bool) -> String {
        let (p, w) = (self.period, self.width);
        let (sw, sh, g) = if vertical {
            (w, p, "<g transform='matrix(0 1 1 0 0 0)'>")
        } else {
            (p, w, "<g>")
        };
        format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{sw:.3}' height='{sh:.3}' viewBox='0 0 {sw:.3} {sh:.3}' preserveAspectRatio='none'>{g}{}</g></svg>",
            self.body
        )
    }

    /// 和 `svg` 一样，只是图案在线宽方向上挪半个线宽、首尾接上：给 Typst 的 stroke 当 paint 用，
    /// stroke 的图案以线的中线为原点。
    pub fn svg_centered(&self, vertical: bool) -> String {
        let (p, w) = (self.period, self.width);
        let h = w / 2.0;
        let (sw, sh) = if vertical { (w, p) } else { (p, w) };
        let g = |dy: f32| {
            if vertical {
                format!("<g transform='matrix(0 1 1 0 {dy:.3} 0)'>{}</g>", self.body)
            } else {
                format!("<g transform='translate(0 {dy:.3})'>{}</g>", self.body)
            }
        };
        format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{sw:.3}' height='{sh:.3}' viewBox='0 0 {sw:.3} {sh:.3}' preserveAspectRatio='none'>{}{}</svg>",
            g(h),
            g(-h)
        )
    }

    /// 当 CSS url() 用的 data URI。
    pub fn data_uri(&self, vertical: bool) -> String {
        let mut o = String::from("data:image/svg+xml,");
        for ch in self.svg(vertical).chars() {
            match ch {
                '%' => o.push_str("%25"),
                '#' => o.push_str("%23"),
                '<' => o.push_str("%3C"),
                '>' => o.push_str("%3E"),
                '"' => o.push_str("%22"),
                c => o.push(c),
            }
        }
        o
    }
}

/// 这条边要不要用图案画；实线、虚线、点线不用（两边都能原生画）。
/// HTML 另外还能原生画 double 和四种立体线，调用前自己挑掉。
pub fn border_tile(b: &Border, side: Edge) -> Option<Tile> {
    use crate::style::BorderStyle as S;
    if b.is_none() {
        return None;
    }
    let w = b.width;
    let c = b.color.css();
    let rect = |x: f32, y: f32, rw: f32, rh: f32, fill: &str| {
        format!("<rect x='{x:.3}' y='{y:.3}' width='{rw:.3}' height='{rh:.3}' fill='{fill}'/>")
    };
    // 横条纹：单位长度依次是 线、空、线……
    let stripes = |units: &[f32]| {
        let k = w / units.iter().sum::<f32>();
        let (mut y, mut o, mut st) = (0.0, String::new(), Vec::new());
        for (i, u) in units.iter().enumerate() {
            if i % 2 == 0 {
                o.push_str(&rect(0.0, y, 4.0, u * k, &c));
                st.push((y, u * k, c.clone()));
            }
            y += u * k;
        }
        Tile {
            period: 4.0,
            width: w,
            body: o,
            stripes: st,
        }
    };
    // 沿边的虚线段：长度依次是 画、空、画……
    let u = w.max(0.75);
    let dashes = |segs: &[f32]| {
        let (mut x, mut o) = (0.0, String::new());
        for (i, s) in segs.iter().enumerate() {
            if i % 2 == 0 {
                o.push_str(&rect(x, 0.0, s * u, w, &c));
            }
            x += s * u;
        }
        Tile {
            period: x,
            width: w,
            body: o,
            stripes: Vec::new(),
        }
    };
    let wave = |y0: f32, h: f32, p: f32| {
        let sw = h / 3.0;
        let mid = y0 + h / 2.0;
        format!(
            "<path d='M0 {mid:.3} Q{:.3} {:.3} {:.3} {mid:.3} T{p:.3} {mid:.3}' fill='none' stroke='{c}' stroke-width='{sw:.3}'/>",
            p / 4.0,
            // 二次曲线最高点只走到控制点的一半，控制点要放到两倍振幅处
            mid - (h - sw),
            p / 2.0
        )
    };
    let light = mix_white(&b.color, 0.6);
    let halves = |a: &str, z: &str| Tile {
        period: 4.0,
        width: w,
        body: format!(
            "{}{}",
            rect(0.0, 0.0, 4.0, w / 2.0, a),
            rect(0.0, w / 2.0, 4.0, w / 2.0, z)
        ),
        stripes: vec![
            (0.0, w / 2.0, a.to_string()),
            (w / 2.0, w / 2.0, z.to_string()),
        ],
    };
    let top_left = matches!(side, Edge::Top | Edge::Left);
    Some(match b.style {
        S::None | S::Solid | S::Thick | S::Dashed | S::Dotted => return None,
        S::Double => stripes(&[1.0, 1.0, 1.0]),
        S::Triple => stripes(&[1.0, 1.0, 1.0, 1.0, 1.0]),
        S::ThinThickSmallGap => stripes(&[1.0, 1.0, 3.0]),
        S::ThickThinSmallGap => stripes(&[3.0, 1.0, 1.0]),
        S::ThinThickThinSmallGap => stripes(&[1.0, 1.0, 3.0, 1.0, 1.0]),
        S::ThinThickMediumGap => stripes(&[1.0, 2.0, 3.0]),
        S::ThickThinMediumGap => stripes(&[3.0, 2.0, 1.0]),
        S::ThinThickThinMediumGap => stripes(&[1.0, 2.0, 3.0, 2.0, 1.0]),
        S::ThinThickLargeGap => stripes(&[1.0, 4.0, 3.0]),
        S::ThickThinLargeGap => stripes(&[3.0, 4.0, 1.0]),
        S::ThinThickThinLargeGap => stripes(&[1.0, 4.0, 3.0, 4.0, 1.0]),
        S::DashDot => dashes(&[3.0, 1.0, 1.0, 1.0]),
        S::DashDotDot => dashes(&[3.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
        S::DashSmallGap => dashes(&[3.0, 0.6]),
        S::DashDotStroked => {
            // 斜着的划和点：/// 样子
            let s = w / 2.0;
            let poly = |x: f32, len: f32| {
                format!(
                    "<polygon points='{:.3},{w:.3} {:.3},{w:.3} {:.3},0 {:.3},0' fill='{c}'/>",
                    x,
                    x + len,
                    x + len + s,
                    x + s
                )
            };
            Tile {
                period: 6.0 * u,
                width: w,
                body: format!("{}{}", poly(0.0, 3.0 * u), poly(4.0 * u, u)),
                stripes: Vec::new(),
            }
        }
        S::Wave => {
            let p = (w * 4.0).max(4.0);
            Tile {
                period: p,
                width: w,
                body: wave(0.0, w, p),
                stripes: Vec::new(),
            }
        }
        S::DoubleWave => {
            let p = (w * 3.0).max(4.0);
            Tile {
                period: p,
                width: w,
                body: format!("{}{}", wave(0.0, w / 2.0, p), wave(w / 2.0, w / 2.0, p)),
                stripes: Vec::new(),
            }
        }
        // 立体线：凸起是外亮内暗，凹陷反过来；y = 0 不随边翻转，所以四边都是同一个上下顺序
        S::ThreeDEmboss => halves(&light, &c),
        S::ThreeDEngrave => halves(&c, &light),
        S::Inset | S::Outset => {
            let dark = (b.style == S::Inset) == top_left;
            let fill = if dark { c.clone() } else { light.clone() };
            Tile {
                period: 4.0,
                width: w,
                body: rect(0.0, 0.0, 4.0, w, &fill),
                stripes: vec![(0.0, w, fill)],
            }
        }
    })
}

/// 颜色往白色里混 `t`（0–1）。
fn mix_white(c: &crate::style::Color, t: f32) -> String {
    let h = c.rgb();
    let ch = |i: usize| {
        let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32;
        (v + (255.0 - v) * t).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
}

// ---------- 页眉页脚 ----------

/// 页眉页脚开头第一段的段前（`first`）或结尾最后一段的段后，pt。Word 排页眉页脚时这两段间距都算：
/// 页眉第一段的段前加在页眉距下面，页脚最后一段的段后垫在页脚距上面（2026-11-22 Word 2024 实测，
/// 以前 pdf 少了段后，Text 默认段后 6pt 时页脚比 Word 低 6pt）。容器往里找第一个、最后一个子元素；
/// 不是段落的（表格、列表、图片）算 0。
pub(crate) fn hf_edge_space(n: &Node, first: bool) -> f32 {
    let Some(p) = hf_edge_para(n, first) else {
        return 0.0;
    };
    let s = &p.style;
    let v = if first { s.space_before } else { s.space_after };
    v.map_or(0.0, |x| x.pt(s.size_pt(), 0.0)).max(0.0)
}

/// 页眉页脚里第一个（`first`）或最后一个段落；容器往里找，碰到表格、列表、图片这类不是段落的是 None。
pub(crate) fn hf_edge_para(n: &Node, first: bool) -> Option<&Node> {
    if matches!(n.component, crate::components::Component::Document(_)) {
        let kid = if first {
            n.children.first()
        } else {
            n.children.last()
        };
        return kid.and_then(|k| hf_edge_para(k, first));
    }
    pbdr_para(n).map(|_| n)
}

// ---------- 段落边框的位置 ----------

/// docx 里写成一个带 w:pBdr 段落的组件：Some(true) 是 Quote、Callout（docx 把线宽和内边距加进了
/// 左右缩进，见 docx.rs 的 boxed），Some(false) 是 Text、Heading、Divider、Code、Math。
/// 编号标题的缩进在编号定义里，不算。
pub(crate) fn pbdr_para(n: &Node) -> Option<bool> {
    use crate::components::Component as C;
    match &n.component {
        C::Quote(_) | C::Callout(_) => Some(true),
        C::Text(_) | C::Divider(_) | C::Code(_) | C::Math(_) => Some(false),
        C::Heading(h) if h.number.is_none() => Some(false),
        _ => None,
    }
}

/// Word 画段落左右边框时，线的里沿和内边距（w:space）之外再往外让出的距离，pt。
/// Windows Word 2024 实测（2026-11-22）：线宽 0.5～3pt、内边距 0～10pt、左右缩进 0 和 36pt，
/// 左边都是 1.42、右边都是 1.58（导出 PDF 整体偏右 0.08），取 1.5。上下没有这一段。
pub(crate) const PBDR_GAP: f32 = 1.5;

/// 按 Word 的画法摆一段带边框的段落：文字留在 docx 写的缩进处，内边距和线都画在缩进外面。
/// 返回改过的样式：左右缩进挪到线的外沿，内边距（有线的边）是 Word 的 w:space（整数 pt），
/// 左右再加 [`PBDR_GAP`]；没有线的边 Word 不留内边距，这里也是 0。线要画在盒子里面
/// （pdf 用 outset 把 Typst 压在边上的线挪进去，html 的 border 本来就在盒子里）。
/// `boxed` 见 [`pbdr_para`]。四边都没有线时返回 None，照原来的画法。
pub(crate) fn word_pbdr(s: &Style, boxed: bool) -> Option<Style> {
    let line = |e: Edge| {
        s.border_side(e)
            .filter(|b| !b.is_none() && b.width > 0.0)
            .map(|b| b.width)
    };
    if Edge::ALL.iter().all(|e| line(*e).is_none()) {
        return None;
    }
    let fs = s.size_pt();
    let pt = |l: Option<Len>| l.map_or(0.0, |x| x.pt(fs, 0.0));
    // docx 写的内边距：四舍五入到整数 pt
    let space = |e: Edge| s.padding_side(e).unwrap_or(0.0).round().max(0.0);
    let mut out = s.clone();
    out.padding = None;
    for (e, m, pad) in [
        (Edge::Left, &mut out.margin_left, &mut out.padding_left),
        (Edge::Right, &mut out.margin_right, &mut out.padding_right),
    ] {
        let base = pt(*m);
        match line(e) {
            Some(w) => {
                let indent = if boxed {
                    base + s.padding_side(e).unwrap_or(0.0) + w
                } else {
                    base
                };
                *m = Some(Len::Pt(indent - w - space(e) - PBDR_GAP));
                *pad = Some(Len::Pt(space(e) + PBDR_GAP));
            }
            None => {
                if m.is_some() {
                    *m = Some(Len::Pt(base));
                }
                *pad = Some(Len::Pt(0.0));
            }
        }
    }
    for (e, pad) in [
        (Edge::Top, &mut out.padding_top),
        (Edge::Bottom, &mut out.padding_bottom),
    ] {
        *pad = Some(Len::Pt(if line(e).is_some() { space(e) } else { 0.0 }));
    }
    Some(out)
}

// ---------- 段落边框合框 ----------

/// 相邻几段边框设置相同的段落，Word 画成一个框（2026-11-21 在 Windows Word 2024 上实测）：
/// 上边只画在第一段上面，下边只画在最后一段下面，左右边连成一条；段与段之间画上面那段的
/// borderBetween，没写就不画线。pdf、html 照这个规则分组，docx 交给 Word 自己合。
///
/// 合不合只看两样：四条边（线型、粗细、颜色、内边距）一样，左右缩进一样。
/// 段前段后、底色、对齐、首行缩进、borderBetween、段落样式不一样都照样合；
/// 中间夹着表格、列表、没边框的段落（空段落也算）就断开。
pub(crate) fn pbdr_key(n: &Node) -> Option<String> {
    let boxed = pbdr_para(n)?;
    let s = &n.style;
    let live = |b: Option<&Border>| b.filter(|b| !b.is_none()).cloned();
    let sides: Vec<Option<Border>> = Edge::ALL.iter().map(|e| live(s.border_side(*e))).collect();
    if sides.iter().all(Option::is_none) && live(s.border_between.as_ref()).is_none() {
        return None;
    }
    let fs = s.size_pt();
    let pt = |l: Option<Len>| l.map_or(0.0, |x| x.pt(fs, 0.0));
    let mut key = String::new();
    for (e, b) in Edge::ALL.iter().zip(&sides) {
        match b {
            // docx 里线宽是 1/8 pt、内边距（w:space）是整数 pt
            Some(b) => key.push_str(&format!(
                "{:?}/{}/{}/{};",
                b.style,
                (b.width * 8.0).round(),
                s.padding_side(*e).unwrap_or(0.0).round(),
                b.color.css()
            )),
            None => key.push_str("-;"),
        }
    }
    // 左右缩进按 docx 写出来的 w:ind 比（Quote、Callout 的缩进加上了线宽和内边距，Word 的左缩进含悬挂量）
    let mut left = pt(s.margin_left) + pt(s.hanging_indent);
    let mut right = pt(s.margin_right);
    if boxed {
        for (i, e, m) in [(3, Edge::Left, &mut left), (1, Edge::Right, &mut right)] {
            if let Some(b) = sides[i].as_ref().filter(|b| b.width > 0.0) {
                *m += s.padding_side(e).unwrap_or(0.0) + b.width;
            }
        }
    }
    key.push_str(&format!(
        "{}|{}",
        (left * 20.0).round(),
        (right * 20.0).round()
    ));
    Some(key)
}

/// children 里要合成一个框的段落：每组是 [起, 止) 区间，至少两段。
pub(crate) fn pbdr_groups(kids: &[Node]) -> Vec<std::ops::Range<usize>> {
    let keys: Vec<Option<String>> = kids.iter().map(pbdr_key).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < kids.len() {
        let mut j = i + 1;
        if keys[i].is_some() {
            while j < kids.len() && keys[j] == keys[i] {
                j += 1;
            }
        }
        if j - i >= 2 {
            out.push(i..j);
        }
        i = j;
    }
    out
}

/// 一组合框段落怎么画。
pub(crate) struct PbdrGroup {
    /// 外框：四条边、左右缩进、第一段的段前和段前分页、最后一段的段后和与下段同页。没有内边距
    pub outer: Style,
    /// 各段改过的样式：去掉边框、左右缩进、段前段后，左右内边距是框的内边距，
    /// 上下内边距是这一段在框里占的距离，所以底色能连成一片
    pub members: Vec<Style>,
    /// 第 i 段和第 i+1 段之间的线：上面那段的 borderBetween
    pub lines: Vec<Option<Border>>,
}

/// 按 Word 的规则排一组合框段落（实测见 docs/STYLE.md 第四节「相邻段落合成一个框」）：
/// - 框的上下内边距只在第一段上面、最后一段下面；第一段的段前、最后一段的段后在框外。
/// - 段与段之间空段前段后里大的那个：上面那段的段后归它自己，线（有的话）画在下面，
///   下一段段前比上一段段后多出来的部分归下一段。
/// - 每段的底色铺满自己那一块，左右铺到边框里沿，所以底色连成一片。
pub(crate) fn pbdr_group(kids: &[Node]) -> PbdrGroup {
    let first = &kids[0].style;
    let last = &kids[kids.len() - 1].style;
    let pt = |s: &Style, l: Option<Len>| l.map_or(0.0, |x| x.pt(s.size_pt(), 0.0));
    let live = |b: Option<&Border>| b.filter(|b| !b.is_none()).cloned();
    let mut outer = Style {
        border_top: live(first.border_side(Edge::Top)),
        border_right: live(first.border_side(Edge::Right)),
        border_bottom: live(first.border_side(Edge::Bottom)),
        border_left: live(first.border_side(Edge::Left)),
        space_before: Some(Len::Pt(pt(first, first.space_before))),
        space_after: Some(Len::Pt(pt(last, last.space_after))),
        page_break_before: first.page_break_before,
        keep_with_next: last.keep_with_next,
        ..Style::default()
    };
    // 框按 Word 的画法摆（见 word_pbdr）：外框的缩进是线的外沿，内边距交给各段
    let boxed = pbdr_para(&kids[0]).unwrap_or(false);
    let wf = word_pbdr(first, boxed).unwrap_or_else(|| first.clone());
    let wl = word_pbdr(last, boxed).unwrap_or_else(|| last.clone());
    if wf.margin_left.is_some() {
        outer.margin_left = Some(Len::Pt(pt(&wf, wf.margin_left)));
    }
    if wf.margin_right.is_some() {
        outer.margin_right = Some(Len::Pt(pt(&wf, wf.margin_right)));
    }
    let pad = |e: Edge| wf.padding_side(e).unwrap_or(0.0);
    let (pad_l, pad_r) = (pad(Edge::Left), pad(Edge::Right));
    let lines: Vec<Option<Border>> = kids[..kids.len() - 1]
        .iter()
        .map(|k| live(k.style.border_between.as_ref()))
        .collect();
    let n = kids.len();
    let mut tops = vec![0.0f32; n];
    let mut bottoms = vec![0.0f32; n];
    tops[0] = pad(Edge::Top);
    bottoms[n - 1] = wl.padding_side(Edge::Bottom).unwrap_or(0.0);
    for i in 0..n - 1 {
        let (a, b) = (&kids[i].style, &kids[i + 1].style);
        let after = pt(a, a.space_after);
        let before = pt(b, b.space_before);
        // 有没有线都一样：上一段的段后归上面那段（线画在它下面），段前比段后多出来的归下面那段
        bottoms[i] = after;
        tops[i + 1] = (before - after).max(0.0);
    }
    let members = kids
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let mut s = k.style.clone();
            s.border = None;
            s.border_top = None;
            s.border_right = None;
            s.border_bottom = None;
            s.border_left = None;
            s.border_between = None;
            s.margin_left = None;
            s.margin_right = None;
            s.page_break_before = None;
            s.padding = None;
            s.padding_top = Some(Len::Pt(tops[i]));
            s.padding_bottom = Some(Len::Pt(bottoms[i]));
            s.padding_left = Some(Len::Pt(pad_l));
            s.padding_right = Some(Len::Pt(pad_r));
            s.space_before = Some(Len::Pt(0.0));
            s.space_after = Some(Len::Pt(0.0));
            s
        })
        .collect();
    PbdrGroup {
        outer,
        members,
        lines,
    }
}
