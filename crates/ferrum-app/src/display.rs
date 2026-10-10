use std::borrow::Cow;

use ferrum_core::is_untrusted_display_character;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
const MAX_DISPLAY_TEXT_BYTES: usize = 4 * 1024;

/// Returns borrowed safe text or visible escapes for selected untrusted formatting.
/// Safe text is borrowed; only escaped text allocates, and the output is capped
/// at 4 KiB. Text whose raw or escaped representation exceeds the bound is omitted.
pub(super) fn escape_untrusted_display_text(text: &str) -> Cow<'_, str> {
    if text.len() > MAX_DISPLAY_TEXT_BYTES {
        return Cow::Borrowed("[display text omitted]");
    }
    let Some(escaped_len) = escaped_text_len(text) else {
        return Cow::Borrowed("[display text omitted]");
    };
    if escaped_len > MAX_DISPLAY_TEXT_BYTES {
        return Cow::Borrowed("[display text omitted]");
    }
    if escaped_len == text.len() {
        return Cow::Borrowed(text);
    }
    Cow::Owned(escape_text(text, escaped_len))
}

/// Escapes text only if the escaped representation fits the caller bound.
pub(super) fn escape_untrusted_display_text_with_limit(
    text: String,
    maximum_bytes: usize,
) -> Option<String> {
    if text.len() > maximum_bytes {
        return None;
    }
    let escaped_len = escaped_text_len(&text)?;
    if escaped_len > maximum_bytes {
        return None;
    }
    if escaped_len == text.len() {
        return Some(text);
    }
    Some(escape_text(&text, escaped_len))
}

fn escaped_text_len(text: &str) -> Option<usize> {
    text.chars().try_fold(text.len(), |length, character| {
        if !is_untrusted_display_character(character) {
            return Some(length);
        }
        let replacement_len = if character.is_control() {
            4
        } else {
            4 + unicode_hex_digit_count(character as u32)
        };
        length.checked_add(replacement_len.checked_sub(character.len_utf8())?)
    })
}

fn unicode_hex_digit_count(mut value: u32) -> usize {
    let mut digits = 1;
    while value >= 16 {
        value >>= 4;
        digits += 1;
    }
    digits
}

fn push_escaped_character(output: &mut String, character: char) {
    let value = character as u32;
    if character.is_control() {
        output.push_str("\\x");
        output.push(char::from(HEX_DIGITS[((value >> 4) & 0xf) as usize]));
        output.push(char::from(HEX_DIGITS[(value & 0xf) as usize]));
        return;
    }

    output.push_str("\\u{");
    let mut digits = [0; 6];
    let mut start = digits.len();
    let mut remaining = value;
    loop {
        start -= 1;
        digits[start] = HEX_DIGITS[(remaining & 0xf) as usize];
        remaining >>= 4;
        if remaining == 0 {
            break;
        }
    }
    for digit in &digits[start..] {
        output.push(char::from(*digit));
    }
    output.push('}');
}

fn escape_text(text: &str, escaped_len: usize) -> String {
    let mut escaped = String::with_capacity(escaped_len);
    for character in text.chars() {
        if is_untrusted_display_character(character) {
            push_escaped_character(&mut escaped, character);
        } else {
            escaped.push(character);
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_hangul_fillers_are_escaped_for_display() {
        let text = "\u{115f}\u{1160}\u{3164}\u{ffa0}";
        assert_eq!(
            escape_untrusted_display_text(text),
            "\\u{115f}\\u{1160}\\u{3164}\\u{ffa0}"
        );
    }

    #[test]
    fn visible_display_text_is_bounded_by_escaped_bytes() {
        let exact_limit = "\n".repeat(1_024);
        assert_eq!(escape_untrusted_display_text(&exact_limit).len(), 4_096);

        let escaped_over_limit = "\n".repeat(1_025);
        assert_eq!(
            escape_untrusted_display_text(&escaped_over_limit),
            "[display text omitted]"
        );

        let raw_over_limit = "x".repeat(4_097);
        assert_eq!(
            escape_untrusted_display_text(&raw_over_limit),
            "[display text omitted]"
        );
    }
}
