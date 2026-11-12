//! VT100/xterm-compatible terminal state machine and screen buffer.
//! Pure Rust, no platform dependencies — fully unit-testable on any host.

use std::collections::VecDeque;

pub const ATTR_BOLD: u8 = 1;
pub const ATTR_DIM: u8 = 2;
pub const ATTR_ITALIC: u8 = 4;
pub const ATTR_UNDERLINE: u8 = 8;
pub const ATTR_BLINK: u8 = 16;
pub const ATTR_INVERSE: u8 = 32;
pub const ATTR_HIDDEN: u8 = 64;
pub const ATTR_STRIKE: u8 = 128;

pub const DEFAULT_FG: u32 = 0xFFE4E4EF;
pub const DEFAULT_BG: u32 = 0xFF0E0E16;
pub const CURSOR_COLOR: u32 = 0xFFB98CFF;

const MAX_SCROLLBACK: usize = 5000;
const MAX_PARAMS: usize = 32;
const MAX_OSC: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Default,
    Idx(u8),
    Rgb(u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub attr: u8,
    /// 1 = normal, 2 = first half of a wide glyph, 0 = continuation of a wide glyph
    pub width: u8,
}

impl Cell {
    pub const BLANK: Cell = Cell { ch: ' ', fg: Color::Default, bg: Color::Default, attr: 0, width: 1 };
    fn blank_bg(bg: Color) -> Cell {
        Cell { bg, ..Cell::BLANK }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Pen {
    fg: Color,
    bg: Color,
    attr: u8,
}

impl Pen {
    const DEFAULT: Pen = Pen { fg: Color::Default, bg: Color::Default, attr: 0 };
}

#[derive(Clone, Copy)]
struct SavedCursor {
    x: usize,
    y: usize,
    pen: Pen,
    dec_graphics: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ground,
    Escape,
    EscIntermediate,
    Charset(u8),
    Csi,
    Osc,
    OscEsc,
    StringIgnore,
    StringIgnoreEsc,
}

type Row = Vec<Cell>;

pub struct Terminal {
    rows: usize,
    cols: usize,
    main: Vec<Row>,
    alt: Vec<Row>,
    using_alt: bool,
    scrollback: VecDeque<Row>,

    cx: usize,
    cy: usize,
    wrap_pending: bool,
    pen: Pen,
    saved_main: SavedCursor,
    saved_alt: SavedCursor,
    top: usize,
    bottom: usize,
    tabs: Vec<bool>,
    last_char: Option<char>,

    // modes
    pub app_cursor: bool,
    pub app_keypad: bool,
    autowrap: bool,
    pub cursor_visible: bool,
    pub bracketed_paste: bool,
    insert_mode: bool,
    origin_mode: bool,
    g0_dec: bool,
    g1_dec: bool,
    shift_out: bool,

    // parser
    state: State,
    params: Vec<u32>,
    cur_param: Option<u32>,
    private: Option<char>,
    intermediate: Option<char>,
    osc: String,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,

    pub title: String,
    pub theme: Theme,
    /// Bytes the terminal must send back to the application (DSR, DA replies).
    pub responses: Vec<u8>,
}

impl Terminal {
    pub fn new(rows: usize, cols: usize) -> Terminal {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let saved = SavedCursor { x: 0, y: 0, pen: Pen::DEFAULT, dec_graphics: false };
        Terminal {
            rows,
            cols,
            main: vec![vec![Cell::BLANK; cols]; rows],
            alt: vec![vec![Cell::BLANK; cols]; rows],
            using_alt: false,
            scrollback: VecDeque::new(),
            cx: 0,
            cy: 0,
            wrap_pending: false,
            pen: Pen::DEFAULT,
            saved_main: saved,
            saved_alt: saved,
            top: 0,
            bottom: rows - 1,
            tabs: default_tabs(cols),
            last_char: None,
            app_cursor: false,
            app_keypad: false,
            autowrap: true,
            cursor_visible: true,
            bracketed_paste: false,
            insert_mode: false,
            origin_mode: false,
            g0_dec: false,
            g1_dec: false,
            shift_out: false,
            state: State::Ground,
            params: Vec::new(),
            cur_param: None,
            private: None,
            intermediate: None,
            osc: String::new(),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            title: String::new(),
            theme: Theme::DEFAULT,
            responses: Vec::new(),
        }
    }

    pub fn rows(&self) -> usize { self.rows }
    pub fn cols(&self) -> usize { self.cols }
    pub fn cursor(&self) -> (usize, usize) { (self.cy, self.cx) }
    pub fn using_alt(&self) -> bool { self.using_alt }
    pub fn scrollback_len(&self) -> usize {
        if self.using_alt { 0 } else { self.scrollback.len() }
    }

    fn screen(&mut self) -> &mut Vec<Row> {
        if self.using_alt { &mut self.alt } else { &mut self.main }
    }

    pub fn screen_ref(&self) -> &Vec<Row> {
        if self.using_alt { &self.alt } else { &self.main }
    }

    /// Text of one screen row (trailing spaces trimmed). Handy for tests.
    pub fn row_text(&self, y: usize) -> String {
        let s: String = self.screen_ref()[y].iter().filter(|c| c.width != 0).map(|c| c.ch).collect();
        s.trim_end().to_string()
    }

    pub fn cell(&self, y: usize, x: usize) -> Cell { self.screen_ref()[y][x] }

    // ───────────────────────── input bytes ─────────────────────────

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.utf8_need > 0 {
                if b & 0xC0 == 0x80 {
                    self.utf8_buf[self.utf8_len] = b;
                    self.utf8_len += 1;
                    if self.utf8_len == self.utf8_need {
                        let c = std::str::from_utf8(&self.utf8_buf[..self.utf8_len])
                            .ok()
                            .and_then(|s| s.chars().next())
                            .unwrap_or('\u{FFFD}');
                        self.utf8_need = 0;
                        self.utf8_len = 0;
                        self.process(c);
                    }
                    continue;
                } else {
                    // truncated sequence
                    self.utf8_need = 0;
                    self.utf8_len = 0;
                    self.process('\u{FFFD}');
                }
            }
            match b {
                0x00..=0x7F => self.process(b as char),
                0xC2..=0xDF => self.start_utf8(b, 2),
                0xE0..=0xEF => self.start_utf8(b, 3),
                0xF0..=0xF4 => self.start_utf8(b, 4),
                _ => self.process('\u{FFFD}'),
            }
        }
    }

    fn start_utf8(&mut self, b: u8, need: usize) {
        self.utf8_buf[0] = b;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    fn process(&mut self, c: char) {
        let code = c as u32;
        match self.state {
            State::Ground => {
                if code < 0x20 || code == 0x7F {
                    self.control(c);
                } else {
                    self.print(c);
                }
            }
            State::Escape => self.escape(c),
            State::EscIntermediate => {
                // e.g. ESC # 8 (DECALN) — consume the final byte
                if code == 0x1B { self.begin_escape(); } else if code >= 0x30 { self.state = State::Ground; }
            }
            State::Charset(which) => {
                let dec = c == '0';
                match which {
                    0 => self.g0_dec = dec,
                    1 => self.g1_dec = dec,
                    _ => {}
                }
                self.state = State::Ground;
            }
            State::Csi => self.csi_char(c),
            State::Osc => match code {
                0x07 => self.finish_osc(),
                0x1B => self.state = State::OscEsc,
                0x18 | 0x1A => self.state = State::Ground,
                _ => {
                    if self.osc.len() < MAX_OSC {
                        self.osc.push(c);
                    }
                }
            },
            State::OscEsc => {
                if c == '\\' {
                    self.finish_osc();
                } else {
                    self.finish_osc();
                    self.begin_escape();
                    self.escape(c);
                }
            }
            State::StringIgnore => match code {
                0x07 | 0x18 | 0x1A => self.state = State::Ground,
                0x1B => self.state = State::StringIgnoreEsc,
                _ => {}
            },
            State::StringIgnoreEsc => {
                self.state = if c == '\\' { State::Ground } else { State::StringIgnore };
            }
        }
    }

    fn begin_escape(&mut self) {
        self.state = State::Escape;
        self.params.clear();
        self.cur_param = None;
        self.private = None;
        self.intermediate = None;
    }

    fn control(&mut self, c: char) {
        match c as u32 {
            0x07 => {}
            0x08 => {
                if self.wrap_pending {
                    self.wrap_pending = false;
                } else if self.cx > 0 {
                    self.cx -= 1;
                }
            }
            0x09 => self.tab_forward(1),
            0x0A | 0x0B | 0x0C => self.linefeed(),
            0x0D => {
                self.cx = 0;
                self.wrap_pending = false;
            }
            0x0E => self.shift_out = true,
            0x0F => self.shift_out = false,
            0x18 | 0x1A => self.state = State::Ground,
            0x1B => self.begin_escape(),
            _ => {}
        }
    }

    fn escape(&mut self, c: char) {
        self.state = State::Ground;
        match c {
            '[' => {
                self.state = State::Csi;
                self.params.clear();
                self.cur_param = None;
                self.private = None;
                self.intermediate = None;
            }
            ']' => {
                self.state = State::Osc;
                self.osc.clear();
            }
            'P' | 'X' | '^' | '_' => self.state = State::StringIgnore,
            '(' => self.state = State::Charset(0),
            ')' => self.state = State::Charset(1),
            '*' | '+' | '-' | '.' | '/' => self.state = State::Charset(2),
            '#' | ' ' | '%' => self.state = State::EscIntermediate,
            '7' => self.save_cursor(),
            '8' => self.restore_cursor(),
            'D' => self.linefeed(),
            'E' => {
                self.cx = 0;
                self.wrap_pending = false;
                self.linefeed();
            }
            'M' => self.reverse_index(),
            'H' => {
                if self.cx < self.cols {
                    self.tabs[self.cx] = true;
                }
            }
            'c' => self.full_reset(),
            '=' => self.app_keypad = true,
            '>' => self.app_keypad = false,
            '\x1b' => self.begin_escape(),
            _ => {}
        }
    }

    fn csi_char(&mut self, c: char) {
        let code = c as u32;
        match c {
            '0'..='9' => {
                let d = code - '0' as u32;
                let v = self.cur_param.unwrap_or(0).saturating_mul(10).saturating_add(d);
                self.cur_param = Some(v.min(65535));
            }
            ';' | ':' => {
                if self.params.len() < MAX_PARAMS {
                    self.params.push(self.cur_param.unwrap_or(0));
                }
                self.cur_param = None;
            }
            '?' | '>' | '=' | '<' => {
                if self.params.is_empty() && self.cur_param.is_none() {
                    self.private = Some(c);
                }
            }
            ' '..='/' => self.intermediate = Some(c),
            '@'..='~' => {
                if let Some(v) = self.cur_param.take() {
                    if self.params.len() < MAX_PARAMS {
                        self.params.push(v);
                    }
                }
                self.state = State::Ground;
                self.dispatch_csi(c);
            }
            _ if code < 0x20 => {
                // C0 controls execute even inside a CSI sequence
                if code == 0x1B {
                    self.begin_escape();
                } else if code == 0x18 || code == 0x1A {
                    self.state = State::Ground;
                } else {
                    self.control(c);
                }
            }
            _ => {}
        }
    }

    fn param(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i) {
            Some(&0) | None => default,
            Some(&v) => v,
        }
    }

    fn dispatch_csi(&mut self, fin: char) {
        if self.intermediate.is_some() {
            // DECSCUSR (CSI Ps SP q), DECSTR (CSI ! p) etc.
            if self.intermediate == Some('!') && fin == 'p' {
                self.soft_reset();
            }
            return;
        }
        match self.private {
            Some('?') => {
                match fin {
                    'h' => self.set_private_modes(true),
                    'l' => self.set_private_modes(false),
                    _ => {}
                }
                return;
            }
            Some('>') => {
                if fin == 'c' {
                    self.responses.extend_from_slice(b"\x1b[>1;10;0c");
                }
                return;
            }
            Some(_) => return,
            None => {}
        }

        if fin != 'm' {
            self.wrap_pending = false;
        }
        let n = self.param(0, 1) as usize;
        match fin {
            '@' => self.insert_chars(n),
            'A' => {
                let min = if self.cy >= self.top { self.top } else { 0 };
                self.cy = self.cy.saturating_sub(n).max(min);
            }
            'B' | 'e' => {
                let max = if self.cy <= self.bottom { self.bottom } else { self.rows - 1 };
                self.cy = (self.cy + n).min(max);
            }
            'C' | 'a' => self.cx = (self.cx + n).min(self.cols - 1),
            'D' => self.cx = self.cx.saturating_sub(n),
            'E' => {
                let max = if self.cy <= self.bottom { self.bottom } else { self.rows - 1 };
                self.cy = (self.cy + n).min(max);
                self.cx = 0;
            }
            'F' => {
                let min = if self.cy >= self.top { self.top } else { 0 };
                self.cy = self.cy.saturating_sub(n).max(min);
                self.cx = 0;
            }
            'G' | '`' => self.cx = (n - 1).min(self.cols - 1),
            'H' | 'f' => {
                let row = self.param(0, 1) as usize - 1;
                let col = self.param(1, 1) as usize - 1;
                self.move_to(row, col);
            }
            'I' => self.tab_forward(n),
            'Z' => self.tab_backward(n),
            'J' => self.erase_display(self.param(0, 0)),
            'K' => self.erase_line(self.param(0, 0)),
            'L' => self.insert_lines(n),
            'M' => self.delete_lines(n),
            'P' => self.delete_chars(n),
            'X' => {
                let bg = self.pen.bg;
                let (cy, cx, cols) = (self.cy, self.cx, self.cols);
                let end = (cx + n).min(cols);
                let row = &mut self.screen()[cy];
                for c in &mut row[cx..end] {
                    *c = Cell::blank_bg(bg);
                }
            }
            'S' => self.scroll_up(n),
            'T' => self.scroll_down(n),
            'b' => {
                if let Some(ch) = self.last_char {
                    for _ in 0..n.min(65535) {
                        self.print(ch);
                    }
                }
            }
            'd' => {
                let row = n - 1;
                let col = self.cx;
                self.move_to(row, col);
            }
            'g' => match self.param(0, 0) {
                0 => {
                    if self.cx < self.cols {
                        self.tabs[self.cx] = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            'h' | 'l' => {
                let on = fin == 'h';
                for i in 0..self.params.len() {
                    if self.params[i] == 4 {
                        self.insert_mode = on;
                    }
                }
            }
            'm' => self.sgr(),
            'n' => match self.param(0, 0) {
                5 => self.responses.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let row = if self.origin_mode { self.cy.saturating_sub(self.top) } else { self.cy };
                    let s = format!("\x1b[{};{}R", row + 1, self.cx + 1);
                    self.responses.extend_from_slice(s.as_bytes());
                }
                _ => {}
            },
            'c' => {
                if self.param(0, 0) == 0 {
                    self.responses.extend_from_slice(b"\x1b[?62;22c");
                }
            }
            'r' => {
                let top = self.param(0, 1) as usize - 1;
                let bottom = (self.param(1, self.rows as u32) as usize).min(self.rows) - 1;
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            _ => {}
        }
    }

    fn set_private_modes(&mut self, on: bool) {
        let params = self.params.clone();
        for p in params {
            match p {
                1 => self.app_cursor = on,
                6 => {
                    self.origin_mode = on;
                    self.move_to(0, 0);
                }
                7 => self.autowrap = on,
                25 => self.cursor_visible = on,
                47 | 1047 => self.switch_alt(on, false),
                1048 => {
                    if on { self.save_cursor() } else { self.restore_cursor() }
                }
                1049 => self.switch_alt(on, true),
                2004 => self.bracketed_paste = on,
                _ => {}
            }
        }
    }

    fn switch_alt(&mut self, on: bool, save_cursor: bool) {
        if on == self.using_alt {
            return;
        }
        if on {
            if save_cursor {
                self.save_cursor();
            }
            self.using_alt = true;
            let cols = self.cols;
            for row in self.alt.iter_mut() {
                *row = vec![Cell::BLANK; cols];
            }
        } else {
            self.using_alt = false;
            if save_cursor {
                self.restore_cursor();
            }
        }
        self.top = 0;
        self.bottom = self.rows - 1;
        self.wrap_pending = false;
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.pen = Pen::DEFAULT;
            return;
        }
        let p = self.params.clone();
        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => self.pen = Pen::DEFAULT,
                1 => self.pen.attr |= ATTR_BOLD,
                2 => self.pen.attr |= ATTR_DIM,
                3 => self.pen.attr |= ATTR_ITALIC,
                4 => self.pen.attr |= ATTR_UNDERLINE,
                5 | 6 => self.pen.attr |= ATTR_BLINK,
                7 => self.pen.attr |= ATTR_INVERSE,
                8 => self.pen.attr |= ATTR_HIDDEN,
                9 => self.pen.attr |= ATTR_STRIKE,
                21 | 22 => self.pen.attr &= !(ATTR_BOLD | ATTR_DIM),
                23 => self.pen.attr &= !ATTR_ITALIC,
                24 => self.pen.attr &= !ATTR_UNDERLINE,
                25 => self.pen.attr &= !ATTR_BLINK,
                27 => self.pen.attr &= !ATTR_INVERSE,
                28 => self.pen.attr &= !ATTR_HIDDEN,
                29 => self.pen.attr &= !ATTR_STRIKE,
                v @ 30..=37 => self.pen.fg = Color::Idx((v - 30) as u8),
                38 | 48 => {
                    let (color, used) = parse_extended_color(&p[i + 1..]);
                    if let Some(c) = color {
                        if p[i] == 38 { self.pen.fg = c } else { self.pen.bg = c }
                    }
                    i += used;
                }
                39 => self.pen.fg = Color::Default,
                v @ 40..=47 => self.pen.bg = Color::Idx((v - 40) as u8),
                49 => self.pen.bg = Color::Default,
                v @ 90..=97 => self.pen.fg = Color::Idx((v - 90 + 8) as u8),
                v @ 100..=107 => self.pen.bg = Color::Idx((v - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }

    fn finish_osc(&mut self) {
        self.state = State::Ground;
        if let Some((code, rest)) = self.osc.split_once(';') {
            if code == "0" || code == "2" {
                self.title = rest.chars().take(256).collect();
            }
        }
        self.osc.clear();
    }

    // ───────────────────────── printing ─────────────────────────

    fn print(&mut self, mut c: char) {
        let dec = if self.shift_out { self.g1_dec } else { self.g0_dec };
        if dec {
            c = dec_special(c);
        }
        let w = char_width(c);
        if w == 0 {
            return; // combining marks / zero-width: not rendered separately
        }
        if self.wrap_pending && self.autowrap {
            self.cx = 0;
            self.linefeed();
        }
        self.wrap_pending = false;

        if w == 2 && self.cx + 1 >= self.cols {
            if self.autowrap && self.cols > 1 {
                let (cy, cx, bg) = (self.cy, self.cx, self.pen.bg);
                self.screen()[cy][cx] = Cell::blank_bg(bg);
                self.cx = 0;
                self.linefeed();
            } else {
                return;
            }
        }

        if self.insert_mode {
            self.insert_chars(w);
        }

        let pen = self.pen;
        let (cy, cx, cols) = (self.cy, self.cx, self.cols);
        let row = &mut self.screen()[cy];
        // Don't leave half of a wide glyph behind.
        if row[cx].width == 0 && cx > 0 {
            row[cx - 1] = Cell::blank_bg(row[cx - 1].bg);
        }
        let end = cx + w;
        if end < cols && row[end].width == 0 {
            row[end] = Cell::blank_bg(row[end].bg);
        }
        row[cx] = Cell { ch: c, fg: pen.fg, bg: pen.bg, attr: pen.attr, width: w as u8 };
        if w == 2 {
            row[cx + 1] = Cell { ch: ' ', fg: pen.fg, bg: pen.bg, attr: pen.attr, width: 0 };
        }
        self.last_char = Some(c);

        let next = cx + w;
        if next >= cols {
            self.cx = cols - 1;
            self.wrap_pending = self.autowrap;
        } else {
            self.cx = next;
        }
    }

    // ───────────────────────── cursor & scrolling ─────────────────────────

    fn move_to(&mut self, row: usize, col: usize) {
        let (min, max) = if self.origin_mode { (self.top, self.bottom) } else { (0, self.rows - 1) };
        self.cy = (row + min).min(max);
        self.cx = col.min(self.cols - 1);
        self.wrap_pending = false;
    }

    fn linefeed(&mut self) {
        if self.cy == self.bottom {
            self.scroll_up(1);
        } else if self.cy < self.rows - 1 {
            self.cy += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.cy == self.top {
            self.scroll_down(1);
        } else if self.cy > 0 {
            self.cy -= 1;
        }
    }

    fn scroll_up(&mut self, n: usize) {
        let (top, bottom, cols, bg) = (self.top, self.bottom, self.cols, self.pen.bg);
        let n = n.min(bottom - top + 1);
        let to_scrollback = !self.using_alt && top == 0;
        for _ in 0..n {
            let row = self.screen().remove(top);
            self.screen().insert(bottom, vec![Cell::blank_bg(bg); cols]);
            if to_scrollback {
                self.scrollback.push_back(row);
                if self.scrollback.len() > MAX_SCROLLBACK {
                    self.scrollback.pop_front();
                }
            }
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let (top, bottom, cols, bg) = (self.top, self.bottom, self.cols, self.pen.bg);
        let n = n.min(bottom - top + 1);
        for _ in 0..n {
            self.screen().remove(bottom);
            self.screen().insert(top, vec![Cell::blank_bg(bg); cols]);
        }
    }

    fn insert_lines(&mut self, n: usize) {
        if self.cy < self.top || self.cy > self.bottom {
            return;
        }
        let (cy, bottom, cols, bg) = (self.cy, self.bottom, self.cols, self.pen.bg);
        let n = n.min(bottom - cy + 1);
        for _ in 0..n {
            self.screen().remove(bottom);
            self.screen().insert(cy, vec![Cell::blank_bg(bg); cols]);
        }
        self.cx = 0;
    }

    fn delete_lines(&mut self, n: usize) {
        if self.cy < self.top || self.cy > self.bottom {
            return;
        }
        let (cy, bottom, cols, bg) = (self.cy, self.bottom, self.cols, self.pen.bg);
        let n = n.min(bottom - cy + 1);
        for _ in 0..n {
            self.screen().remove(cy);
            self.screen().insert(bottom, vec![Cell::blank_bg(bg); cols]);
        }
        self.cx = 0;
    }

    fn insert_chars(&mut self, n: usize) {
        let (cy, cx, cols, bg) = (self.cy, self.cx, self.cols, self.pen.bg);
        let n = n.min(cols - cx);
        let row = &mut self.screen()[cy];
        for _ in 0..n {
            row.pop();
            row.insert(cx, Cell::blank_bg(bg));
        }
    }

    fn delete_chars(&mut self, n: usize) {
        let (cy, cx, cols, bg) = (self.cy, self.cx, self.cols, self.pen.bg);
        let n = n.min(cols - cx);
        let row = &mut self.screen()[cy];
        for _ in 0..n {
            row.remove(cx);
            row.push(Cell::blank_bg(bg));
        }
    }

    fn erase_display(&mut self, mode: u32) {
        let (cy, rows, cols, bg) = (self.cy, self.rows, self.cols, self.pen.bg);
        match mode {
            0 => {
                self.erase_line(0);
                for y in cy + 1..rows {
                    self.screen()[y] = vec![Cell::blank_bg(bg); cols];
                }
            }
            1 => {
                self.erase_line(1);
                for y in 0..cy {
                    self.screen()[y] = vec![Cell::blank_bg(bg); cols];
                }
            }
            2 => {
                for y in 0..rows {
                    self.screen()[y] = vec![Cell::blank_bg(bg); cols];
                }
            }
            3 => self.scrollback.clear(),
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u32) {
        let (cy, cx, cols, bg) = (self.cy, self.cx, self.cols, self.pen.bg);
        let row = &mut self.screen()[cy];
        let range = match mode {
            0 => cx..cols,
            1 => 0..(cx + 1).min(cols),
            2 => 0..cols,
            _ => return,
        };
        for c in &mut row[range] {
            *c = Cell::blank_bg(bg);
        }
    }

    fn tab_forward(&mut self, n: usize) {
        for _ in 0..n {
            let mut x = self.cx + 1;
            while x < self.cols - 1 && !self.tabs[x] {
                x += 1;
            }
            self.cx = x.min(self.cols - 1);
        }
        self.wrap_pending = false;
    }

    fn tab_backward(&mut self, n: usize) {
        for _ in 0..n {
            if self.cx == 0 {
                break;
            }
            let mut x = self.cx - 1;
            while x > 0 && !self.tabs[x] {
                x -= 1;
            }
            self.cx = x;
        }
    }

    fn save_cursor(&mut self) {
        let s = SavedCursor { x: self.cx, y: self.cy, pen: self.pen, dec_graphics: self.g0_dec };
        if self.using_alt { self.saved_alt = s } else { self.saved_main = s }
    }

    fn restore_cursor(&mut self) {
        let s = if self.using_alt { self.saved_alt } else { self.saved_main };
        self.cx = s.x.min(self.cols - 1);
        self.cy = s.y.min(self.rows - 1);
        self.pen = s.pen;
        self.g0_dec = s.dec_graphics;
        self.wrap_pending = false;
    }

    fn soft_reset(&mut self) {
        self.pen = Pen::DEFAULT;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.app_cursor = false;
        self.app_keypad = false;
        self.autowrap = true;
        self.cursor_visible = true;
        self.insert_mode = false;
        self.origin_mode = false;
        self.g0_dec = false;
        self.g1_dec = false;
        self.shift_out = false;
        self.wrap_pending = false;
    }

    fn full_reset(&mut self) {
        let (rows, cols) = (self.rows, self.cols);
        let title = std::mem::take(&mut self.title);
        let responses = std::mem::take(&mut self.responses);
        let theme = self.theme;
        *self = Terminal::new(rows, cols);
        self.theme = theme;
        self.title = title;
        self.responses = responses;
    }

    // ───────────────────────── resize ─────────────────────────

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(2);
        if rows == self.rows && cols == self.cols {
            return;
        }
        // Columns: truncate or pad every row (no reflow).
        for grid in [&mut self.main, &mut self.alt] {
            for row in grid.iter_mut() {
                row.resize(cols, Cell::BLANK);
                if let Some(last) = row.last_mut() {
                    if last.width == 2 {
                        *last = Cell::BLANK;
                    }
                }
            }
        }
        // Rows on the main screen: keep the cursor line visible, push
        // lines that fall off the top into scrollback.
        if rows < self.rows {
            let excess = self.rows - rows;
            let main_cy = if self.using_alt { self.saved_main.y } else { self.cy };
            let drop_top = excess.min(main_cy);
            for _ in 0..drop_top {
                let r = self.main.remove(0);
                self.scrollback.push_back(r);
            }
            self.main.truncate(rows);
            if self.using_alt {
                self.saved_main.y = self.saved_main.y.saturating_sub(drop_top);
                let alt_drop = excess.min(self.cy);
                self.alt.drain(0..alt_drop);
                self.alt.truncate(rows);
                self.cy -= alt_drop;
            } else {
                self.cy -= drop_top;
            }
            while self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.pop_front();
            }
        } else {
            // Growing: pull lines back from scrollback first (like xterm).
            let mut grow = rows - self.rows;
            while grow > 0 && !self.using_alt {
                match self.scrollback.pop_back() {
                    Some(mut r) => {
                        r.resize(cols, Cell::BLANK);
                        self.main.insert(0, r);
                        self.cy += 1;
                        grow -= 1;
                    }
                    None => break,
                }
            }
            while self.main.len() < rows {
                self.main.push(vec![Cell::BLANK; cols]);
            }
            while self.alt.len() < rows {
                self.alt.push(vec![Cell::BLANK; cols]);
            }
        }
        self.rows = rows;
        self.cols = cols;
        self.top = 0;
        self.bottom = rows - 1;
        self.cx = self.cx.min(cols - 1);
        self.cy = self.cy.min(rows - 1);
        self.saved_main.y = self.saved_main.y.min(rows - 1);
        self.saved_alt.y = self.saved_alt.y.min(rows - 1);
        self.wrap_pending = false;
        self.tabs = default_tabs(cols);
    }

    // ───────────────────────── rendering snapshot ─────────────────────────

    pub const HEADER_LEN: usize = 8;
    pub const INTS_PER_CELL: usize = 4;

    /// Flattened view for the UI layer.
    ///
    /// Header: [rows, cols, cursorRow (-1 = hidden), cursorCol, scrollbackLen, flags, defaultBg, cursorColor]
    /// flags: bit0 app cursor keys, bit1 bracketed paste, bit2 alt screen
    /// Then rows*cols cells, each [codepoint (0 = wide continuation), fgARGB, bgARGB, attr]
    /// with colors fully resolved (palette, bold-bright, inverse, dim, hidden).
    pub fn snapshot(&self, scroll: usize) -> Vec<i32> {
        let sb_len = self.scrollback_len();
        let scroll = scroll.min(sb_len);
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.rows * self.cols * Self::INTS_PER_CELL);
        let cursor_row = if self.cursor_visible && self.cy + scroll < self.rows {
            (self.cy + scroll) as i32
        } else {
            -1
        };
        let flags = (self.app_cursor as i32) | ((self.bracketed_paste as i32) << 1) | ((self.using_alt as i32) << 2);
        out.extend_from_slice(&[
            self.rows as i32,
            self.cols as i32,
            cursor_row,
            self.cx as i32,
            sb_len as i32,
            flags,
            self.theme.bg as i32,
            self.theme.cursor as i32,
        ]);
        let screen = self.screen_ref();
        let start = sb_len - scroll;
        for line in 0..self.rows {
            let idx = start + line;
            let row: &Row = if idx < sb_len { &self.scrollback[idx] } else { &screen[idx - sb_len] };
            for x in 0..self.cols {
                let cell = row.get(x).copied().unwrap_or(Cell::BLANK);
                let (fg, bg) = resolve_colors(&cell, &self.theme);
                let cp = if cell.width == 0 { 0 } else { cell.ch as i32 };
                out.extend_from_slice(&[cp, fg as i32, bg as i32, cell.attr as i32]);
            }
        }
        out
    }
}

fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|i| i > 0 && i % 8 == 0).collect()
}

fn parse_extended_color(p: &[u32]) -> (Option<Color>, usize) {
    match p.first() {
        Some(5) => match p.get(1) {
            Some(&n) => (Some(Color::Idx(n.min(255) as u8)), 2),
            None => (None, 1),
        },
        Some(2) => {
            if p.len() >= 4 {
                let (r, g, b) = (p[1].min(255), p[2].min(255), p[3].min(255));
                (Some(Color::Rgb((r << 16) | (g << 8) | b)), 4)
            } else {
                (None, p.len())
            }
        }
        _ => (None, 0),
    }
}

/// Colors the UI can swap at runtime: default fg/bg, cursor, ANSI 0–15.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Theme {
    pub fg: u32,
    pub bg: u32,
    pub cursor: u32,
    pub base: [u32; 16],
}

impl Theme {
    pub const DEFAULT: Theme = Theme {
        fg: DEFAULT_FG,
        bg: DEFAULT_BG,
        cursor: CURSOR_COLOR,
        base: [
            0x1B1B26, 0xF2617A, 0x7CD992, 0xF4C96B, 0x6FA3F7, 0xC48CFF, 0x5FD7D7, 0xC8C8D8,
            0x5A5A72, 0xFF8197, 0x9CF0B0, 0xFFDD8E, 0x93BCFF, 0xDAB0FF, 0x8EF0F0, 0xFFFFFF,
        ],
    };

    /// From [fg, bg, cursor, c0..c15] (alpha ignored).
    pub fn from_ints(v: &[i32]) -> Option<Theme> {
        if v.len() < 19 {
            return None;
        }
        let mut base = [0u32; 16];
        for i in 0..16 {
            base[i] = v[3 + i] as u32 & 0xFFFFFF;
        }
        Some(Theme {
            fg: 0xFF000000 | v[0] as u32,
            bg: 0xFF000000 | v[1] as u32,
            cursor: 0xFF000000 | v[2] as u32,
            base,
        })
    }

    /// 256-color palette: 0–15 from the theme, 16–255 xterm standard.
    pub fn color(&self, i: u8) -> u32 {
        let i = i as u32;
        let rgb = if i < 16 {
            self.base[i as usize]
        } else if i < 232 {
            let v = i - 16;
            let lvl = |c: u32| if c == 0 { 0 } else { 55 + c * 40 };
            (lvl(v / 36) << 16) | (lvl((v / 6) % 6) << 8) | lvl(v % 6)
        } else {
            let g = 8 + (i - 232) * 10;
            (g << 16) | (g << 8) | g
        };
        0xFF000000 | rgb
    }
}

/// Palette lookup in the default theme.
pub fn palette(i: u8) -> u32 {
    Theme::DEFAULT.color(i)
}

fn resolve_colors(cell: &Cell, th: &Theme) -> (u32, u32) {
    let mut fg = match cell.fg {
        Color::Default => th.fg,
        Color::Idx(i) if i < 8 && cell.attr & ATTR_BOLD != 0 => th.color(i + 8),
        Color::Idx(i) => th.color(i),
        Color::Rgb(v) => 0xFF000000 | v,
    };
    let mut bg = match cell.bg {
        Color::Default => th.bg,
        Color::Idx(i) => th.color(i),
        Color::Rgb(v) => 0xFF000000 | v,
    };
    if cell.attr & ATTR_DIM != 0 {
        let half = |c: u32, s: u32| (((c >> s) & 0xFF) * 2 / 3) << s;
        fg = 0xFF000000 | half(fg, 16) | half(fg, 8) | half(fg, 0);
    }
    if cell.attr & ATTR_INVERSE != 0 {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.attr & ATTR_HIDDEN != 0 {
        fg = bg;
    }
    (fg, bg)
}

/// DEC Special Graphics (line drawing) for ESC ( 0.
fn dec_special(c: char) -> char {
    match c {
        '`' => '◆', 'a' => '▒', 'f' => '°', 'g' => '±', 'j' => '┘', 'k' => '┐',
        'l' => '┌', 'm' => '└', 'n' => '┼', 'o' => '⎺', 'p' => '⎻', 'q' => '─',
        'r' => '⎼', 's' => '⎽', 't' => '├', 'u' => '┤', 'v' => '┴', 'w' => '┬',
        'x' => '│', 'y' => '≤', 'z' => '≥', '{' => 'π', '|' => '≠', '}' => '£',
        '~' => '·', _ => c,
    }
}

/// Approximate wcwidth: 0 for combining/zero-width, 2 for East Asian wide and emoji.
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x300 {
        return 1;
    }
    const ZERO: &[(u32, u32)] = &[
        (0x0300, 0x036F), (0x0483, 0x0489), (0x0591, 0x05BD), (0x0610, 0x061A),
        (0x064B, 0x065F), (0x0E31, 0x0E31), (0x0E34, 0x0E3A), (0x0E47, 0x0E4E),
        (0x1AB0, 0x1AFF), (0x1DC0, 0x1DFF), (0x200B, 0x200F), (0x2028, 0x202E),
        (0x2060, 0x2064), (0x20D0, 0x20FF), (0xFE00, 0xFE0F), (0xFE20, 0xFE2F),
        (0xFEFF, 0xFEFF), (0x1F3FB, 0x1F3FF), (0xE0000, 0xE007F),
    ];
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115F), (0x231A, 0x231B), (0x2329, 0x232A), (0x23E9, 0x23EC),
        (0x23F0, 0x23F0), (0x23F3, 0x23F3), (0x25FD, 0x25FE), (0x2614, 0x2615),
        (0x2648, 0x2653), (0x267F, 0x267F), (0x2693, 0x2693), (0x26A1, 0x26A1),
        (0x26AA, 0x26AB), (0x26BD, 0x26BE), (0x26C4, 0x26C5), (0x26CE, 0x26CE),
        (0x26D4, 0x26D4), (0x26EA, 0x26EA), (0x26F2, 0x26F3), (0x26F5, 0x26F5),
        (0x26FA, 0x26FA), (0x26FD, 0x26FD), (0x2705, 0x2705), (0x270A, 0x270B),
        (0x2728, 0x2728), (0x274C, 0x274C), (0x274E, 0x274E), (0x2753, 0x2755),
        (0x2757, 0x2757), (0x2795, 0x2797), (0x27B0, 0x27B0), (0x27BF, 0x27BF),
        (0x2B1B, 0x2B1C), (0x2B50, 0x2B50), (0x2B55, 0x2B55), (0x2E80, 0x303E),
        (0x3041, 0x33FF), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xA000, 0xA4CF),
        (0xA960, 0xA97F), (0xAC00, 0xD7A3), (0xF900, 0xFAFF), (0xFE10, 0xFE19),
        (0xFE30, 0xFE6F), (0xFF00, 0xFF60), (0xFFE0, 0xFFE6), (0x16FE0, 0x16FE4),
        (0x17000, 0x18CFF), (0x1B000, 0x1B2FF), (0x1F004, 0x1F004), (0x1F0CF, 0x1F0CF),
        (0x1F18E, 0x1F18E), (0x1F191, 0x1F19A), (0x1F200, 0x1F251), (0x1F300, 0x1F320),
        (0x1F32D, 0x1F335), (0x1F337, 0x1F37C), (0x1F37E, 0x1F393), (0x1F3A0, 0x1F3CA),
        (0x1F3CF, 0x1F3D3), (0x1F3E0, 0x1F3F0), (0x1F3F4, 0x1F3F4), (0x1F3F8, 0x1F43E),
        (0x1F440, 0x1F440), (0x1F442, 0x1F4FC), (0x1F4FF, 0x1F53D), (0x1F54B, 0x1F54E),
        (0x1F550, 0x1F567), (0x1F57A, 0x1F57A), (0x1F595, 0x1F596), (0x1F5A4, 0x1F5A4),
        (0x1F5FB, 0x1F64F), (0x1F680, 0x1F6C5), (0x1F6CC, 0x1F6CC), (0x1F6D0, 0x1F6D2),
        (0x1F6D5, 0x1F6D7), (0x1F6DC, 0x1F6DF), (0x1F6EB, 0x1F6EC), (0x1F6F4, 0x1F6FC),
        (0x1F7E0, 0x1F7EB), (0x1F7F0, 0x1F7F0), (0x1F90C, 0x1F93A), (0x1F93C, 0x1F945),
        (0x1F947, 0x1F9FF), (0x1FA70, 0x1FAFF), (0x20000, 0x2FFFD), (0x30000, 0x3FFFD),
    ];
    let hit = |t: &[(u32, u32)]| {
        t.binary_search_by(|&(lo, hi)| {
            if u < lo { std::cmp::Ordering::Greater } else if u > hi { std::cmp::Ordering::Less } else { std::cmp::Ordering::Equal }
        })
        .is_ok()
    };
    if hit(ZERO) {
        0
    } else if hit(WIDE) {
        2
    } else {
        1
    }
}
