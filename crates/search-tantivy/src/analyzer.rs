//! Lexical analyzers. A generation's `analyzer_version` selects one, both when
//! it is built and when it is reopened, so generations built before a change
//! stay readable.
//!
//! `tantivy-0.26.2-cjk-bigram-v1` splits text into runs: each run of CJK
//! characters (Han, Hiragana, Katakana) becomes its overlapping character
//! bigrams (a single character stays one token), every other run of
//! alphanumeric characters becomes one lowercased word, and anything else
//! separates tokens. Positions are consecutive, so a phrase query over the
//! bigrams is a substring match within a CJK run. No dictionary is used.

use tantivy::Index;
use tantivy::tokenizer::{RemoveLongFilter, TextAnalyzer, Token, TokenStream, Tokenizer};

/// The analyzer of generations built before Japanese segmentation.
pub const LEGACY_ANALYZER_VERSION: &str = "tantivy-default-0.26.2";
/// Character-bigram CJK segmentation, Tantivy built-ins only.
pub const CJK_BIGRAM_ANALYZER_VERSION: &str = "tantivy-0.26.2-cjk-bigram-v1";

pub(crate) const CJK_BIGRAM_TOKENIZER: &str = "kp_cjk_bigram_v1";
/// Longer alphanumeric words are dropped, as Tantivy's default does at 40.
const MAX_TOKEN_BYTES: usize = 40;

/// The registered tokenizer name of a supported analyzer version.
pub(crate) fn tokenizer_name(analyzer_version: &str) -> Option<&'static str> {
    match analyzer_version {
        LEGACY_ANALYZER_VERSION => Some("default"),
        CJK_BIGRAM_ANALYZER_VERSION => Some(CJK_BIGRAM_TOKENIZER),
        _ => None,
    }
}

/// The supported version as a static string.
pub(crate) fn supported(analyzer_version: &str) -> Option<&'static str> {
    [LEGACY_ANALYZER_VERSION, CJK_BIGRAM_ANALYZER_VERSION]
        .into_iter()
        .find(|known| *known == analyzer_version)
}

/// Registers every non-default analyzer on a created or opened index.
pub(crate) fn register(index: &Index) {
    index.tokenizers().register(
        CJK_BIGRAM_TOKENIZER,
        TextAnalyzer::builder(CjkBigramTokenizer)
            .filter(RemoveLongFilter::limit(MAX_TOKEN_BYTES))
            .build(),
    );
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x309F      // Hiragana
        | 0x30A1..=0x30FA    // Katakana letters
        | 0x30FC..=0x30FF    // ー and iteration marks (・ U+30FB separates)
        | 0x31F0..=0x31FF    // Katakana phonetic extensions
        | 0x3005..=0x3007    // 々 〆 〇
        | 0x3400..=0x4DBF    // CJK extension A
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xF900..=0xFAFF    // CJK compatibility ideographs
        | 0xFF66..=0xFF9F    // Halfwidth Katakana
        | 0x20000..=0x2FA1F) // CJK extensions B.. and compatibility supplement
}

#[derive(Clone, Copy)]
pub(crate) struct CjkBigramTokenizer;

pub(crate) struct CjkBigramStream {
    tokens: Vec<Token>,
    next: usize,
}

impl Tokenizer for CjkBigramTokenizer {
    type TokenStream<'a> = CjkBigramStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> CjkBigramStream {
        CjkBigramStream {
            tokens: tokenize(text),
            next: 0,
        }
    }
}

impl TokenStream for CjkBigramStream {
    fn advance(&mut self) -> bool {
        if self.next < self.tokens.len() {
            self.next += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.next - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.next - 1]
    }
}

#[derive(PartialEq)]
enum Class {
    Cjk,
    Word,
    Other,
}

fn class(c: char) -> Class {
    if is_cjk(c) {
        Class::Cjk
    } else if c.is_alphanumeric() {
        Class::Word
    } else {
        Class::Other
    }
}

fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map_or(text.len(), |(offset, _)| *offset);
    let mut tokens = Vec::new();
    let mut position = 0;
    let mut push = |from: usize, to: usize, value: String, tokens: &mut Vec<Token>| {
        tokens.push(Token {
            offset_from: from,
            offset_to: to,
            position,
            text: value,
            position_length: 1,
        });
        position += 1;
    };
    let mut i = 0;
    while i < chars.len() {
        let kind = class(chars[i].1);
        let mut j = i + 1;
        while j < chars.len() && class(chars[j].1) == kind {
            j += 1;
        }
        match kind {
            Class::Cjk if j - i == 1 => {
                push(
                    chars[i].0,
                    end_of(i + 1),
                    chars[i].1.to_string(),
                    &mut tokens,
                );
            }
            Class::Cjk => {
                for k in i..j - 1 {
                    let value: String = [chars[k].1, chars[k + 1].1].iter().collect();
                    push(chars[k].0, end_of(k + 2), value, &mut tokens);
                }
            }
            Class::Word => {
                let value = text[chars[i].0..end_of(j)].to_lowercase();
                push(chars[i].0, end_of(j), value, &mut tokens);
            }
            Class::Other => {}
        }
        i = j;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str) -> Vec<String> {
        tokenize(text).into_iter().map(|token| token.text).collect()
    }

    #[test]
    fn cjk_runs_become_overlapping_bigrams_and_words_stay_words() {
        assert_eq!(texts("東京都"), ["東京", "京都"]);
        assert_eq!(
            texts("アンパサンドの語源"),
            [
                "アン", "ンパ", "パサ", "サン", "ンド", "ドの", "の語", "語源"
            ]
        );
        assert_eq!(texts("F1（エフ・ワン）"), ["f1", "エフ", "ワン"]);
        assert_eq!(texts("法第2条"), ["法第", "2", "条"]);
        assert_eq!(texts("Hello, World"), ["hello", "world"]);
    }

    #[test]
    fn positions_are_consecutive_and_offsets_cover_the_source() {
        let text = "金融商品 取引";
        let tokens = tokenize(text);
        for (index, token) in tokens.iter().enumerate() {
            assert_eq!(token.position, index);
            assert!(text[token.offset_from..token.offset_to].chars().count() <= 2);
        }
        assert_eq!(&text[tokens[0].offset_from..tokens[0].offset_to], "金融");
    }

    #[test]
    fn versions_map_to_tokenizers() {
        assert_eq!(tokenizer_name(LEGACY_ANALYZER_VERSION), Some("default"));
        assert_eq!(
            tokenizer_name(CJK_BIGRAM_ANALYZER_VERSION),
            Some(CJK_BIGRAM_TOKENIZER)
        );
        assert_eq!(tokenizer_name("unknown"), None);
    }
}
