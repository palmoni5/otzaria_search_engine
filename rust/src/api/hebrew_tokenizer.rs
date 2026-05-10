use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

/// טוקנייזר עברי שמתנהג כמו SimpleTokenizer, אבל:
///   • שומר גרש (' או ׳) בסוף טוקן.
///   • שומר גרש (' או ׳) בתוך טוקן כשהוא מוקף בתווי-מילה משני הצדדים
///     (למשל תעתיקים כגון "ג'ורג'", "צ'יפס", או קידומות לועזיות "ד'אש").
///   • שומר גרשיים (" או ״) בתוך טוקן כשהם מוקפים בתווי-מילה משני הצדדים
///     (למשל ראשי-תיבות עבריים כגון "רמב״ם", "ד״ה").
///
/// נורמליזציה של תווים עבריים לתואמים הלועזיים בטוקן הסופי, כדי שיתאים
/// ל-`sanitizeQuery` בצד הדארט (`׳`→`'`, `״`→`"`):
///
/// דוגמאות:
///   "תוס'"   → ["תוס'"]      (גרש בסוף — נשמר)
///   "תוס׳"   → ["תוס'"]      (גרש עברי בסוף — מנורמל לגרש לועזי)
///   "ג'ורג'" → ["ג'ורג'"]    (גרש בין אותיות + גרש סופי — שניהם נשמרים)
///   "רמב\"ם" → ["רמב\"ם"]    (גרשיים בין אותיות — חלק מהטוקן)
///   "רמב״ם"  → ["רמב\"ם"]    (גרשיים עבריים בין אותיות — מנורמל ל-")
///   "תוס' ד\"ה" → ["תוס'", "ד\"ה"]
#[derive(Clone, Default)]
pub struct HebrewTokenizer;

pub struct HebrewTokenStream<'a> {
    text: &'a str,
    token: Token,
    byte_pos: usize,
    token_count: usize,
}

impl Tokenizer for HebrewTokenizer {
    type TokenStream<'a> = HebrewTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        HebrewTokenStream {
            text,
            token: Token::default(),
            byte_pos: 0,
            token_count: 0,
        }
    }
}

#[inline]
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

#[inline]
fn is_geresh(c: char) -> bool {
    c == '\'' || c == '\u{05F3}'
}

#[inline]
fn is_gershayim(c: char) -> bool {
    c == '"' || c == '\u{05F4}'
}

impl<'a> HebrewTokenStream<'a> {
    /// מחזיר את גבולות הטוקן הבא (byte offsets בטקסט המקורי).
    /// טקסט הטוקן (עם נורמליזציה) נבנה על ידי הקורא.
    fn find_next_token(text: &str, start_byte: usize) -> Option<(usize, usize)> {
        let slice = &text[start_byte..];

        // מצא את תחילת הטוקן (תו-מילה ראשון)
        let tok_start_rel = slice
            .char_indices()
            .find(|(_, c)| is_word_char(*c))
            .map(|(i, _)| i)?;

        let tok_start = start_byte + tok_start_rel;
        let mut byte_pos = tok_start;
        let mut tok_end = tok_start;

        while byte_pos < text.len() {
            let c = text[byte_pos..].chars().next().unwrap();
            let c_len = c.len_utf8();

            if is_word_char(c) {
                byte_pos += c_len;
                tok_end = byte_pos;
                continue;
            }

            // עבור גרשיים וגרש: בודקים מה התו הבא כדי להחליט אם לכלול אותו.
            let next_is_word = text[byte_pos + c_len..]
                .chars()
                .next()
                .map(is_word_char)
                .unwrap_or(false);

            if is_gershayim(c) {
                if next_is_word {
                    // גרשיים בין אותיות — חלק מהטוקן
                    byte_pos += c_len;
                    tok_end = byte_pos;
                    continue;
                }
                break; // גרשיים בסוף או לפני לא-מילה — מפריד
            }

            if is_geresh(c) {
                // גרש בין אותיות — חלק מהטוקן (תעתיקים: ג'ורג', צ'יפס, ד'אש).
                if next_is_word {
                    byte_pos += c_len;
                    tok_end = byte_pos;
                    continue;
                }
                // גרש סופי (לפני רווח/EOF) — נכלל ומסיים את הטוקן.
                byte_pos += c_len;
                tok_end = byte_pos;
                break;
            }

            // כל מפריד אחר
            break;
        }

        Some((tok_start, tok_end))
    }

    /// כותב את טקסט הטוקן ל-`out` עם נורמליזציה של ׳→' ו-״→".
    /// במסלול המהיר (אין תווים שדורשים נורמליזציה — הרוב המכריע של הטוקנים)
    /// מתבצע `push_str` יחיד מה-slice של המקור, בלי הקצאות וללא לולאת char.
    fn append_token_text(out: &mut String, slice: &str) {
        if slice.contains(['\u{05F3}', '\u{05F4}']) {
            for c in slice.chars() {
                match c {
                    '\u{05F3}' => out.push('\''),
                    '\u{05F4}' => out.push('"'),
                    _ => out.push(c),
                }
            }
        } else {
            out.push_str(slice);
        }
    }
}

impl<'a> TokenStream for HebrewTokenStream<'a> {
    fn advance(&mut self) -> bool {
        match Self::find_next_token(self.text, self.byte_pos) {
            None => false,
            Some((tok_start, tok_end)) => {
                self.token.text.clear();
                Self::append_token_text(
                    &mut self.token.text,
                    &self.text[tok_start..tok_end],
                );
                self.token.offset_from = tok_start;
                self.token.offset_to = tok_end;
                self.token.position = self.token_count;
                self.token_count += 1;
                self.byte_pos = tok_end;
                true
            }
        }
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(text: &str) -> Vec<String> {
        let mut tokenizer = HebrewTokenizer;
        let mut stream = tokenizer.token_stream(text);
        let mut tokens = Vec::new();
        while stream.advance() {
            tokens.push(stream.token().text.clone());
        }
        tokens
    }

    // ── גרש (' / ׳) ─────────────────────────────────────────────────────────

    #[test]
    fn test_trailing_geresh_kept() {
        assert_eq!(tokenize("תוס'"), vec!["תוס'"]);
    }

    #[test]
    fn test_hebrew_geresh_normalized_to_ascii() {
        assert_eq!(tokenize("תוס\u{05F3}"), vec!["תוס'"]);
    }

    #[test]
    fn test_hebrew_and_ascii_geresh_produce_same_token() {
        assert_eq!(tokenize("תוס\u{05F3}"), tokenize("תוס'"));
    }

    #[test]
    fn test_interior_geresh_kept() {
        // גרש בין אותיות נשמר כחלק מהטוקן (תעתיקים: ג'ורג', צ'יפס, ד'אש).
        assert_eq!(tokenize("ד'אש"), vec!["ד'אש"]);
        assert_eq!(tokenize("ג'ורג'"), vec!["ג'ורג'"]);
        assert_eq!(tokenize("צ'יפס"), vec!["צ'יפס"]);
    }

    #[test]
    fn test_hebrew_interior_geresh_normalized() {
        // ׳ עברי בין אותיות נכלל ומנורמל ל-' לועזי.
        assert_eq!(tokenize("ד\u{05F3}אש"), vec!["ד'אש"]);
    }

    #[test]
    fn test_hebrew_and_ascii_interior_geresh_produce_same_token() {
        assert_eq!(tokenize("ג\u{05F3}ורג\u{05F3}"), tokenize("ג'ורג'"));
    }

    #[test]
    fn test_double_geresh_splits() {
        // '' אינו "גרש פנימי" (התו אחריו לא תו-מילה) — נכלל הראשון כסופי
        // והשני נדלג, ואז המילה הבאה מתפצלת לטוקן נפרד.
        assert_eq!(tokenize("רמב''ם"), vec!["רמב'", "ם"]);
    }

    // ── גרשיים (" / ״) ──────────────────────────────────────────────────────

    #[test]
    fn test_interior_gershayim_kept() {
        assert_eq!(tokenize("רמב\"ם"), vec!["רמב\"ם"]);
    }

    #[test]
    fn test_hebrew_interior_gershayim_normalized() {
        // ״ עבריים מנורמלים ל-" לועזיים בטוקן הסופי.
        assert_eq!(tokenize("רמב\u{05F4}ם"), vec!["רמב\"ם"]);
    }

    #[test]
    fn test_hebrew_and_ascii_gershayim_produce_same_token() {
        assert_eq!(tokenize("רמב\u{05F4}ם"), tokenize("רמב\"ם"));
    }

    #[test]
    fn test_trailing_gershayim_dropped() {
        // גרשיים בסוף (ללא תו-מילה אחרי) אינם חלק מהטוקן.
        assert_eq!(tokenize("רמב\""), vec!["רמב"]);
        assert_eq!(tokenize("רמב\u{05F4}"), vec!["רמב"]);
    }

    #[test]
    fn test_leading_gershayim_dropped() {
        assert_eq!(tokenize("\"רמב"), vec!["רמב"]);
        assert_eq!(tokenize("\u{05F4}רמב"), vec!["רמב"]);
    }

    #[test]
    fn test_double_gershayim_splits() {
        // ""  אינו "גרשיים פנימיים" (התו אחריו לא תו-מילה) — מפצל.
        assert_eq!(tokenize("רמב\"\"ם"), vec!["רמב", "ם"]);
    }

    #[test]
    fn test_multiple_interior_gershayim() {
        assert_eq!(tokenize("א\"ב\"ג"), vec!["א\"ב\"ג"]);
    }

    #[test]
    fn test_interior_gershayim_with_trailing_geresh() {
        assert_eq!(tokenize("רמב\"ם'"), vec!["רמב\"ם'"]);
    }

    // ── ביטויים מורכבים ───────────────────────────────────────────────────

    #[test]
    fn test_phrase_with_trailing_geresh_and_gershayim() {
        // ד"ה הופך לטוקן יחיד עם הגרשיים הפנימיים.
        assert_eq!(tokenize("תוס' ד\"ה"), vec!["תוס'", "ד\"ה"]);
    }

    #[test]
    fn test_hebrew_chars_normalized_inside_token_end_to_end() {
        // כל קלט עם תווים עבריים (׳/״) — אחרי הטוקניזציה הטוקן מכיל רק
        // את התווים הלועזיים (' ו-"). זה מבטיח התאמה מלאה לפלט של
        // sanitizeQuery בצד הדארט שמקדים את ההמרה לפני הרגקס.
        assert_eq!(tokenize("תוס\u{05F3}"), vec!["תוס'"]);
        assert_eq!(tokenize("רמב\u{05F4}ם"), vec!["רמב\"ם"]);
        assert_eq!(tokenize("ג\u{05F3}ורג\u{05F3}"), vec!["ג'ורג'"]);
        assert_eq!(
            tokenize("רמב\u{05F4}ם תוס\u{05F3}"),
            vec!["רמב\"ם", "תוס'"]
        );
        assert_eq!(
            tokenize("הרב פלוני ז\u{05F4}ל"),
            vec!["הרב", "פלוני", "ז\"ל"]
        );
    }

    #[test]
    fn test_no_hebrew_geresh_or_gershayim_in_output() {
        // ערובה: הטוקנים לעולם לא יכילו ׳ (U+05F3) או ״ (U+05F4).
        let inputs = [
            "ג\u{05F3}ורג\u{05F3}",
            "רמב\u{05F4}ם",
            "תוס\u{05F3} ד\u{05F4}ה",
            "א\u{05F4}ב\u{05F3}ג",
        ];
        for input in inputs {
            for tok in tokenize(input) {
                assert!(
                    !tok.contains('\u{05F3}'),
                    "טוקן `{tok}` (קלט: `{input}`) מכיל ׳ עברי",
                );
                assert!(
                    !tok.contains('\u{05F4}'),
                    "טוקן `{tok}` (קלט: `{input}`) מכיל ״ עברי",
                );
            }
        }
    }

    #[test]
    fn test_plain_words() {
        assert_eq!(tokenize("שלום עולם"), vec!["שלום", "עולם"]);
    }

    #[test]
    fn test_standalone_word_no_geresh() {
        assert_eq!(tokenize("תוס"), vec!["תוס"]);
    }
}
