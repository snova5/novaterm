use novaterm::session::Session;
use novaterm::term::*;
use std::time::{Duration, Instant};

fn mk(rows: usize, cols: usize, s: &str) -> Terminal {
    let mut t = Terminal::new(rows, cols);
    t.feed(s.as_bytes());
    t
}

#[test]
fn prints_and_newlines() {
    let t = mk(3, 10, "salut\r\nlume");
    assert_eq!(t.row_text(0), "salut");
    assert_eq!(t.row_text(1), "lume");
    assert_eq!(t.cursor(), (1, 4));
}

#[test]
fn utf8_split_across_feeds() {
    let mut term = Terminal::new(2, 10);
    let bytes = "ăîșț─".as_bytes();
    for b in bytes {
        term.feed(&[*b]);
    }
    assert_eq!(term.row_text(0), "ăîșț─");
}

#[test]
fn autowrap_and_scrollback() {
    let t = mk(2, 4, "abcdefghij");
    // abcd / efgh scrolled, ij on last line
    assert_eq!(t.row_text(1), "ij");
    assert_eq!(t.row_text(0), "efgh");
    assert_eq!(t.scrollback_len(), 1);
}

#[test]
fn pending_wrap_then_cr_does_not_wrap() {
    let t = mk(2, 4, "abcd\rX");
    assert_eq!(t.row_text(0), "Xbcd");
    assert_eq!(t.row_text(1), "");
}

#[test]
fn cursor_movement_and_erase() {
    let t = mk(3, 10, "0123456789\x1b[1;4H\x1b[K");
    assert_eq!(t.row_text(0), "012");
    let t2 = mk(3, 10, "abc\x1b[2J\x1b[H\x1b[3;5Hx");
    assert_eq!(t2.row_text(0), "");
    assert_eq!(t2.row_text(2), "    x");
}

#[test]
fn sgr_colors_resolve() {
    let t = mk(1, 10, "\x1b[1;31mA\x1b[0;38;5;196mB\x1b[38;2;10;20;30;48;5;21mC\x1b[7mD");
    let snap = t.snapshot(0);
    let cell = |x: usize| &snap[Terminal::HEADER_LEN + x * 4..Terminal::HEADER_LEN + x * 4 + 4];
    // bold red → bright red (palette 9)
    assert_eq!(cell(0)[1] as u32, palette(9));
    assert_eq!(cell(0)[3] as u8 & ATTR_BOLD, ATTR_BOLD);
    assert_eq!(cell(1)[1] as u32, palette(196));
    assert_eq!(cell(2)[1] as u32, 0xFF0A141E);
    assert_eq!(cell(2)[2] as u32, palette(21));
    // inverse swaps fg/bg
    assert_eq!(cell(3)[1] as u32, palette(21));
    assert_eq!(cell(3)[2] as u32, 0xFF0A141E);
}

#[test]
fn scroll_region_and_insert_delete_lines() {
    let mut term = Terminal::new(5, 5);
    term.feed(b"1\r\n2\r\n3\r\n4\r\n5");
    term.feed(b"\x1b[2;4r\x1b[4;1H\n"); // scroll inside rows 2..4
    assert_eq!(term.row_text(0), "1");
    assert_eq!(term.row_text(1), "3");
    assert_eq!(term.row_text(2), "4");
    assert_eq!(term.row_text(3), "");
    assert_eq!(term.row_text(4), "5");
    assert_eq!(term.scrollback_len(), 0);
    term.feed(b"\x1b[r\x1b[2;1H\x1b[L");
    assert_eq!(term.row_text(1), "");
    assert_eq!(term.row_text(2), "3");
}

#[test]
fn alt_screen_restores_main() {
    let mut term = mk(3, 10, "shell$ ");
    term.feed(b"\x1b[?1049h\x1b[2J\x1b[Hvim stuff");
    assert!(term.using_alt());
    assert_eq!(term.row_text(0), "vim stuff");
    term.feed(b"\x1b[?1049l");
    assert!(!term.using_alt());
    assert_eq!(term.row_text(0), "shell$");
    assert_eq!(term.cursor(), (0, 7));
}

#[test]
fn wide_chars_take_two_cells() {
    let t = mk(1, 6, "a漢b");
    assert_eq!(t.cell(0, 1).width, 2);
    assert_eq!(t.cell(0, 2).width, 0);
    assert_eq!(t.cell(0, 3).ch, 'b');
    assert_eq!(t.cursor(), (0, 4));
}

#[test]
fn dsr_and_da_replies() {
    let mut term = mk(5, 10, "ab\x1b[6n\x1b[c");
    let r = String::from_utf8(std::mem::take(&mut term.responses)).unwrap();
    assert_eq!(r, "\x1b[1;3R\x1b[?62;22c");
}

#[test]
fn osc_title_and_dec_graphics() {
    let t = mk(1, 10, "\x1b]0;NovaTerm\x07\x1b(0lqk\x1b(B");
    assert_eq!(t.title, "NovaTerm");
    assert_eq!(t.row_text(0), "┌─┐");
}

#[test]
fn delete_and_insert_chars() {
    let t = mk(1, 10, "abcdef\x1b[1;2H\x1b[2P");
    assert_eq!(t.row_text(0), "adef");
    let t2 = mk(1, 10, "abcdef\x1b[1;2H\x1b[2@");
    assert_eq!(t2.row_text(0), "a  bcdef");
}

#[test]
fn resize_keeps_cursor_line() {
    let mut term = mk(5, 10, "1\r\n2\r\n3\r\n4\r\n5");
    term.resize(3, 10);
    assert_eq!(term.row_text(2), "5");
    assert_eq!(term.cursor().0, 2);
    term.resize(5, 12);
    assert_eq!(term.row_text(4), "5");
    assert_eq!(term.row_text(0), "1");
}

#[test]
fn garbage_never_panics() {
    let mut term = Terminal::new(4, 7);
    let mut x: u32 = 12345;
    let mut buf = Vec::new();
    for _ in 0..200_000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf.push((x & 0xFF) as u8);
        if x % 7 == 0 {
            buf.extend_from_slice(b"\x1b[");
        }
    }
    term.feed(&buf);
    term.resize(2, 3);
    term.feed(&buf[..5000]);
    term.resize(40, 120);
    term.feed(&buf);
    let _ = term.snapshot(1000);
}

#[test]
fn real_shell_through_pty() {
    let env = vec!["TERM=xterm-256color".to_string(), "PATH=/usr/bin:/bin".to_string()];
    let argv = vec!["sh".to_string()];
    let s = Session::start("/bin/sh", &argv, &env, "/", 10, 40).expect("spawn");
    s.write(b"stty size; printf 'ok-%s\\n' $((6*7)); exit 3\n");
    let deadline = Instant::now() + Duration::from_secs(10);
    let code = loop {
        let r = s.wait_update();
        if r >= 0 {
            break r;
        }
        assert!(Instant::now() < deadline, "timeout");
    };
    assert_eq!(code, 3);
    let term = s.term.lock().unwrap();
    let text: Vec<String> = (0..term.rows()).map(|y| term.row_text(y)).collect();
    let all = text.join("\n");
    assert!(all.contains("10 40"), "stty size missing:\n{all}");
    assert!(all.contains("ok-42"), "output missing:\n{all}");
}

#[test]
fn hangup_kills_running_job() {
    let env = vec!["PATH=/usr/bin:/bin".to_string()];
    let argv = vec!["sh".to_string(), "-c".to_string(), "sleep 30".to_string()];
    let s = Session::start("/bin/sh", &argv, &env, "/", 5, 20).expect("spawn");
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    s.hangup();
    let code = loop {
        let r = s.wait_update();
        if r >= 0 {
            break r;
        }
    };
    assert!(code >= 128, "expected signal exit, got {code}");
    assert!(t0.elapsed() < Duration::from_secs(5));
}

#[test]
fn theme_changes_resolved_colors_and_survives_reset() {
    let mut term = mk(1, 4, "\x1b[31mA\x1b[0mB");
    let mut ints = vec![0x112233, 0x445566, 0x778899];
    ints.extend((0..16).map(|i| 0x010101 * i));
    term.theme = Theme::from_ints(&ints).unwrap();
    term.feed(b"\x1bc\x1b[31mA\x1b[0mB"); // full reset keeps the theme
    let s = term.snapshot(0);
    assert_eq!(s[6] as u32, 0xFF445566);
    assert_eq!(s[7] as u32, 0xFF778899);
    assert_eq!(s[Terminal::HEADER_LEN + 1] as u32, 0xFF010101); // ansi red slot
    assert_eq!(s[Terminal::HEADER_LEN + 5] as u32, 0xFF112233); // default fg
    assert_eq!(s[Terminal::HEADER_LEN + 6] as u32, 0xFF445566); // default bg
    assert!(Theme::from_ints(&[1, 2, 3]).is_none());
}
