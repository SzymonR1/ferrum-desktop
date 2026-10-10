use ferrum_core::is_untrusted_display_character;

#[test]
fn unicode_tag_format_characters_are_untrusted_for_display() {
    for character in ['\u{e0001}', '\u{e0020}', '\u{e007e}', '\u{e007f}'] {
        assert!(is_untrusted_display_character(character));
    }
}

#[test]
fn default_ignorable_hangul_fillers_are_untrusted_for_display() {
    for character in ['\u{115f}', '\u{1160}', '\u{3164}', '\u{ffa0}'] {
        assert!(is_untrusted_display_character(character));
    }
}
