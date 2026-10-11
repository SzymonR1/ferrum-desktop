use eframe::egui::{
    self, Color32, FontId, Key, Modifiers, Stroke, TextFormat, TextStyle,
    text::{ByteIndex, LayoutJob, LayoutSection},
};
use vt100::{Callbacks, Color, Parser, Screen};

const SCROLLBACK_ROWS: usize = 10_000;
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLUMNS: u16 = 80;
// Match vte's fixed no-std OSC budget even though its default std parser uses an unbounded Vec.
const MAX_OSC_SEQUENCE_BYTES: usize = 1024;

#[derive(Clone, Copy, Default)]
enum OscState {
    #[default]
    Ground,
    Escape,
    InOsc {
        bytes: usize,
    },
    EscapeAfterOsc,
    DiscardOsc,
}

#[derive(Default)]
struct OscStringLimiter {
    state: OscState,
}

impl OscStringLimiter {
    fn feed<CB: Callbacks>(&mut self, parser: &mut Parser<CB>, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match self.state {
                OscState::Ground => {
                    let Some(escape) = bytes.iter().position(|byte| *byte == 0x1b) else {
                        parser.process(bytes);
                        return;
                    };
                    if escape > 0 {
                        parser.process(&bytes[..escape]);
                    }
                    bytes = &bytes[escape + 1..];
                    self.state = OscState::Escape;
                }
                OscState::Escape => {
                    let byte = bytes[0];
                    if byte == b']' {
                        parser.process(b"\x1b]");
                        bytes = &bytes[1..];
                        self.state = OscState::InOsc { bytes: 0 };
                    } else {
                        let sequence = [0x1b, byte];
                        parser.process(&sequence);
                        bytes = &bytes[1..];
                        self.state = if escape_keeps_pending(byte) {
                            OscState::EscapeAfterOsc
                        } else {
                            OscState::Ground
                        };
                    }
                }
                OscState::InOsc { bytes: seen } => {
                    if is_osc_terminator(bytes[0]) {
                        let terminator = bytes[0];
                        parser.process(&bytes[..1]);
                        bytes = &bytes[1..];
                        self.state = if terminator == 0x1b {
                            OscState::EscapeAfterOsc
                        } else {
                            OscState::Ground
                        };
                        continue;
                    }

                    let data_length = bytes
                        .iter()
                        .position(|byte| is_osc_terminator(*byte))
                        .unwrap_or(bytes.len());
                    let remaining = MAX_OSC_SEQUENCE_BYTES - seen;
                    if data_length > remaining {
                        if remaining > 0 {
                            parser.process(&bytes[..remaining]);
                        }
                        // Keep the parser's buffer at the limit; do not feed the string tail.
                        bytes = &bytes[remaining..];
                        self.state = OscState::DiscardOsc;
                    } else {
                        if data_length > 0 {
                            parser.process(&bytes[..data_length]);
                        }
                        bytes = &bytes[data_length..];
                        self.state = OscState::InOsc {
                            bytes: seen + data_length,
                        };
                    }
                }
                OscState::EscapeAfterOsc => {
                    let byte = bytes[0];
                    parser.process(&bytes[..1]);
                    bytes = &bytes[1..];
                    self.state = if byte == b']' {
                        OscState::InOsc { bytes: 0 }
                    } else if escape_keeps_pending(byte) {
                        OscState::EscapeAfterOsc
                    } else {
                        OscState::Ground
                    };
                }
                OscState::DiscardOsc => {
                    let Some(terminator_index) =
                        bytes.iter().position(|byte| is_osc_terminator(*byte))
                    else {
                        return;
                    };
                    let terminator = bytes[terminator_index];
                    parser.process(&bytes[terminator_index..terminator_index + 1]);
                    bytes = &bytes[terminator_index + 1..];
                    self.state = if terminator == 0x1b {
                        OscState::EscapeAfterOsc
                    } else {
                        OscState::Ground
                    };
                }
            }
        }
    }
}

fn is_osc_terminator(byte: u8) -> bool {
    matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b)
}

fn escape_keeps_pending(byte: u8) -> bool {
    matches!(
        byte,
        0x00..=0x17 | 0x19 | 0x1b | 0x1c..=0x1f | 0x7f..=0xff
    )
}

#[derive(Default)]
struct NoopCallbacks;

impl Callbacks for NoopCallbacks {
    fn copy_to_clipboard(&mut self, _: &mut Screen, _: &[u8], _: &[u8]) {}
    fn paste_from_clipboard(&mut self, _: &mut Screen, _: &[u8]) {}
    fn set_window_icon_name(&mut self, _: &mut Screen, _: &[u8]) {}
    fn set_window_title(&mut self, _: &mut Screen, _: &[u8]) {}
}

pub(crate) struct TerminalModel {
    parser: Parser<NoopCallbacks>,
    osc_limiter: OscStringLimiter,
    rows: u16,
    columns: u16,
    revision: u64,
    cached_rows: Vec<Option<CachedRow>>,
}

struct CachedRow {
    revision: u64,
    font_id: FontId,
    galley: std::sync::Arc<egui::Galley>,
}

#[derive(Clone, Copy, PartialEq)]
struct CellStyle {
    foreground: Color32,
    background: Color32,
    italic: bool,
    underline: bool,
}

impl Default for TerminalModel {
    fn default() -> Self {
        Self::new(DEFAULT_ROWS, DEFAULT_COLUMNS)
    }
}

impl TerminalModel {
    pub(crate) fn new(rows: u16, columns: u16) -> Self {
        let rows = rows.max(1);
        let columns = columns.max(1);
        Self {
            parser: Parser::new_with_callbacks(rows, columns, SCROLLBACK_ROWS, NoopCallbacks),
            osc_limiter: OscStringLimiter::default(),
            rows,
            columns,
            revision: 0,
            cached_rows: (0..rows).map(|_| None).collect(),
        }
    }
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        // Clipboard and title requests are explicitly discarded by NoopCallbacks.
        self.osc_limiter.feed(&mut self.parser, bytes);
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn resize(&mut self, rows: u16, columns: u16) {
        self.rows = rows.max(1);
        self.columns = columns.max(1);
        self.parser.screen_mut().set_size(self.rows, self.columns);
        self.cached_rows = (0..self.rows).map(|_| None).collect();
        self.bump_revision();
    }

    pub(crate) fn dimensions(&self) -> (u16, u16) {
        (self.rows, self.columns)
    }

    pub(crate) fn contents(&self) -> String {
        self.parser.screen().contents()
    }
    pub(crate) fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }
    pub(crate) fn application_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }
    pub(crate) fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }
    pub(crate) fn scrollback_by(&mut self, rows: isize) {
        let current = self.parser.screen().scrollback();
        let position = if rows.is_negative() {
            current.saturating_sub(rows.unsigned_abs())
        } else {
            current.saturating_add(rows as usize).min(SCROLLBACK_ROWS)
        };
        if position != current {
            self.parser.screen_mut().set_scrollback(position);
            self.bump_revision();
        }
    }
    pub(crate) fn scrollback_position(&self) -> usize {
        self.parser.screen().scrollback()
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        if self.parser.screen().scrollback() > 0 {
            self.parser.screen_mut().set_scrollback(0);
            self.bump_revision();
        }
    }

    pub(crate) fn render(&mut self, ui: &mut egui::Ui) {
        let screen = self.parser.screen();
        let (cursor_row, cursor_column) = screen.cursor_position();
        let hide_cursor = screen.hide_cursor() || screen.scrollback() > 0;
        let font_id = TextStyle::Monospace.resolve(ui.style());
        let default_fg = Color32::from_rgb(220, 220, 220);
        let default_bg = Color32::from_rgb(20, 28, 36);

        for row in 0..self.rows as usize {
            let cached = self.cached_rows[row]
                .as_ref()
                .filter(|cached| cached.revision == self.revision && cached.font_id == font_id);
            let galley = if let Some(cached) = cached {
                std::sync::Arc::clone(&cached.galley)
            } else {
                let mut job = LayoutJob::default();
                job.text.reserve(self.columns as usize * 4);
                let mut active_style = None;
                let mut section_start = 0;

                for column in 0..self.columns {
                    let Some(cell) = screen.cell(row as u16, column) else {
                        continue;
                    };
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let (mut foreground, background) = if cell.inverse() {
                        (
                            color(cell.bgcolor(), default_fg),
                            color(cell.fgcolor(), default_bg),
                        )
                    } else {
                        (
                            color(cell.fgcolor(), default_fg),
                            color(cell.bgcolor(), default_bg),
                        )
                    };
                    if cell.bold() {
                        foreground = emphasize(foreground);
                    }
                    let style = CellStyle {
                        foreground,
                        background,
                        italic: cell.italic(),
                        underline: cell.underline()
                            || (!hide_cursor
                                && cursor_row == row as u16
                                && cursor_column == column),
                    };
                    if active_style != Some(style) {
                        if let Some(previous) = active_style {
                            job.sections.push(layout_section(
                                section_start..job.text.len(),
                                previous,
                                font_id.clone(),
                            ));
                        }
                        active_style = Some(style);
                        section_start = job.text.len();
                    }
                    let contents = cell.contents();
                    job.text
                        .push_str(if contents.is_empty() { " " } else { contents });
                }

                if let Some(style) = active_style {
                    job.sections.push(layout_section(
                        section_start..job.text.len(),
                        style,
                        font_id.clone(),
                    ));
                }
                let galley = ui.painter().layout_job(job);
                self.cached_rows[row] = Some(CachedRow {
                    revision: self.revision,
                    font_id: font_id.clone(),
                    galley: std::sync::Arc::clone(&galley),
                });
                galley
            };
            ui.add(egui::Label::new(galley).extend());
        }
    }
}

fn layout_section(
    byte_range: std::ops::Range<usize>,
    style: CellStyle,
    font_id: FontId,
) -> LayoutSection {
    let mut format = TextFormat::simple(font_id, style.foreground);
    format.background = style.background;
    format.italics = style.italic;
    format.underline = if style.underline {
        Stroke::new(1.0, style.foreground)
    } else {
        Stroke::NONE
    };
    LayoutSection {
        leading_space: 0.0,
        byte_range: ByteIndex::from(byte_range.start)..ByteIndex::from(byte_range.end),
        format,
    }
}

fn color(color: Color, default: Color32) -> Color32 {
    match color {
        Color::Default => default,
        Color::Idx(index) => indexed_color(index),
        Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}
fn emphasize(color: Color32) -> Color32 {
    if color == Color32::BLACK {
        return Color32::from_gray(96);
    }
    let brighten = |channel: u8| channel.saturating_add((255 - channel) / 2);
    Color32::from_rgb(
        brighten(color.r()),
        brighten(color.g()),
        brighten(color.b()),
    )
}

fn indexed_color(index: u8) -> Color32 {
    const BASIC: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let (r, g, b) = match index {
        0..=15 => BASIC[index as usize],
        16..=231 => {
            let n = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            (level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        232..=255 => {
            let gray = 8 + 10 * (index - 232);
            (gray, gray, gray)
        }
    };
    Color32::from_rgb(r, g, b)
}

pub(crate) fn key_bytes(
    key: Key,
    modifiers: Modifiers,
    application_cursor: bool,
) -> Option<Vec<u8>> {
    if modifiers.ctrl {
        let byte = match key {
            Key::A => 0x01,
            Key::B => 0x02,
            Key::C => 0x03,
            Key::D => 0x04,
            Key::E => 0x05,
            Key::F => 0x06,
            Key::G => 0x07,
            Key::H => 0x08,
            Key::I => 0x09,
            Key::J | Key::Enter => 0x0a,
            Key::K => 0x0b,
            Key::L => 0x0c,
            Key::M => 0x0d,
            Key::N => 0x0e,
            Key::O => 0x0f,
            Key::P => 0x10,
            Key::Q => 0x11,
            Key::R => 0x12,
            Key::S => 0x13,
            Key::T => 0x14,
            Key::U => 0x15,
            Key::V => 0x16,
            Key::W => 0x17,
            Key::X => 0x18,
            Key::Y => 0x19,
            Key::Z => 0x1a,
            Key::Space => 0,
            Key::Backspace => 0x7f,
            _ => return None,
        };
        return Some(vec![byte]);
    }
    let bytes: &[u8] = match key {
        Key::Enter => b"\r",
        Key::Backspace => b"\x7f",
        Key::Tab => b"\t",
        Key::Escape => b"\x1b",
        Key::ArrowUp => {
            if application_cursor {
                b"\x1bOA"
            } else {
                b"\x1b[A"
            }
        }
        Key::ArrowDown => {
            if application_cursor {
                b"\x1bOB"
            } else {
                b"\x1b[B"
            }
        }
        Key::ArrowRight => {
            if application_cursor {
                b"\x1bOC"
            } else {
                b"\x1b[C"
            }
        }
        Key::ArrowLeft => {
            if application_cursor {
                b"\x1bOD"
            } else {
                b"\x1b[D"
            }
        }
        Key::Home => {
            if application_cursor {
                b"\x1bOH"
            } else {
                b"\x1b[H"
            }
        }
        Key::End => {
            if application_cursor {
                b"\x1bOF"
            } else {
                b"\x1b[F"
            }
        }
        Key::Delete => b"\x1b[3~",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        _ => return None,
    };
    Some(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unicode_and_ansi_foreground_colors() {
        let mut terminal = TerminalModel::default();
        terminal.feed("\x1b[31;1mλ猫".as_bytes());
        assert!(terminal.contents().contains("λ猫"));
        assert_eq!(
            terminal.parser.screen().cell(0, 0).unwrap().fgcolor(),
            Color::Idx(1)
        );
        assert!(terminal.parser.screen().cell(0, 0).unwrap().bold());
        assert_eq!(
            color(Color::Idx(1), Color32::WHITE),
            Color32::from_rgb(205, 0, 0)
        );
        assert_ne!(
            emphasize(Color32::from_rgb(205, 0, 0)),
            Color32::from_rgb(205, 0, 0)
        );
    }
    #[test]
    fn tracks_bracketed_paste_mode() {
        let mut terminal = TerminalModel::default();
        assert!(!terminal.bracketed_paste());
        terminal.feed(b"\x1b[?2004h");
        assert!(terminal.bracketed_paste());
    }
    #[test]
    fn resizes_and_tracks_cursor_and_alternate_screen() {
        let mut terminal = TerminalModel::default();
        terminal.feed(b"\x1b[?1049h\x1b[3;5H");
        assert!(terminal.parser.screen().alternate_screen());
        assert_eq!(terminal.parser.screen().cursor_position(), (2, 4));
        terminal.resize(30, 90);
        assert_eq!(terminal.parser.screen().size(), (30, 90));
    }

    #[test]
    fn encodes_control_c_and_navigation_keys() {
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![0x03]));
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, true),
            Some(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn scrollback_selection_changes_the_visible_terminal_rows() {
        let mut terminal = TerminalModel::new(2, 12);
        terminal.feed(b"one\r\ntwo\r\nthree\r\nlast");
        let current = terminal.contents();
        terminal.scrollback_by(2);
        assert!(terminal.scrollback_position() > 0);
        assert_ne!(terminal.contents(), current);
        terminal.scroll_to_bottom();
        assert_eq!(terminal.scrollback_position(), 0);
        assert_eq!(terminal.contents(), current);
    }
    #[derive(Default)]
    struct ClipboardProbe {
        max_payload_bytes: usize,
        copy_calls: usize,
    }

    impl Callbacks for ClipboardProbe {
        fn copy_to_clipboard(&mut self, _: &mut Screen, _: &[u8], data: &[u8]) {
            self.max_payload_bytes = self.max_payload_bytes.max(data.len());
            self.copy_calls += 1;
        }
    }

    #[test]
    fn osc52_payload_is_capped_at_the_terminal_budget() {
        let header_len = b"52;c;".len();
        let prefixes: [&[u8]; 4] = [b"\x1b]", b"\x1b\x07]", b"\x1b\x7f]", b"\x1b\x9d]"];
        for prefix in prefixes {
            for payload_len in [
                MAX_OSC_SEQUENCE_BYTES - header_len,
                MAX_OSC_SEQUENCE_BYTES * 8,
            ] {
                let mut parser = Parser::new_with_callbacks(2, 20, 0, ClipboardProbe::default());
                let mut limiter = OscStringLimiter::default();
                let mut sequence = prefix.to_vec();
                sequence.extend_from_slice(b"52;c;");
                sequence.resize(sequence.len() + payload_len, b'A');
                sequence.push(b'\x07');

                limiter.feed(&mut parser, &sequence);

                assert_eq!(parser.callbacks().copy_calls, 1);
                assert_eq!(
                    parser.callbacks().max_payload_bytes,
                    payload_len.min(MAX_OSC_SEQUENCE_BYTES - header_len),
                );
            }
        }
    }

    #[test]
    fn oversized_osc_resynchronizes_after_bel_across_io_chunks() {
        let mut terminal = TerminalModel::new(2, 40);
        terminal.feed(b"before\x1b");
        terminal.feed(b"]52;c;");
        let payload = vec![b'A'; MAX_OSC_SEQUENCE_BYTES * 8];
        for chunk in payload.chunks(127) {
            terminal.feed(chunk);
        }
        terminal.feed(b"\x07after");

        let contents = terminal.contents();
        assert!(contents.contains("before"));
        assert!(contents.contains("after"));
        assert!(!contents.contains("AAAA"));
    }

    #[test]
    fn oversized_osc_resynchronizes_after_split_string_terminator() {
        let mut terminal = TerminalModel::new(2, 40);
        terminal.feed(b"\x1b]52;c;");
        let payload = vec![b'A'; MAX_OSC_SEQUENCE_BYTES * 8];
        for chunk in payload.chunks(311) {
            terminal.feed(chunk);
        }
        terminal.feed(b"\x1b");
        terminal.feed(b"\\resumed");

        assert!(terminal.contents().contains("resumed"));
    }

    #[test]
    fn oversized_osc_resynchronizes_after_cancel_controls() {
        let payload = vec![b'A'; MAX_OSC_SEQUENCE_BYTES * 8];
        for terminator in [b"\x18".as_slice(), b"\x1a".as_slice()] {
            let mut terminal = TerminalModel::new(2, 40);
            terminal.feed(b"\x1b]52;c;");
            terminal.feed(&payload);
            terminal.feed(terminator);
            terminal.feed(b"resumed");
            assert!(terminal.contents().contains("resumed"));
        }
    }
    #[test]
    fn scrollback_is_bounded_and_osc52_is_ignored() {
        let mut terminal = TerminalModel::new(2, 20);
        for _ in 0..11_000 {
            terminal.feed(b"x\r\n");
        }
        terminal.parser.screen_mut().set_scrollback(usize::MAX);
        assert!(terminal.parser.screen().scrollback() <= SCROLLBACK_ROWS);
        terminal.parser.screen_mut().set_scrollback(0);
        terminal.feed(b"\x1b]52;c;Y2xpcGJvYXJk\x07safe");
        assert!(terminal.contents().contains("safe"));
        assert!(!terminal.contents().contains("clipboard"));
    }
}
