//! Subtitle database for the narration voice-overs (25th Anniversary data).
//!
//! The remaster ships transcripts for the classic voice triggers - the
//! training narrations on earth/station, the Polito/SHODAN story triggers -
//! as KEX subtitle files (`sshock2-subtitles.kpf`, `*.sub`) whose text lines
//! are `$key` tokens resolved through the KEX localization table
//! (`base.kpf:localization/loc_english.txt`). The classic 1999 data has no
//! transcript for these sounds at all, so on a classic install this database
//! is simply empty and the narrations stay audio-only, exactly as they were
//! in 1999.
//!
//! A `.sub` file is a brace-structured list of speaker blocks:
//!
//! ```text
//! SUB1
//! {
//!     type "convo"
//!     descr "$name_PA"
//!     singleton
//!     multisub trg0001 {
//!         { time 00 length 3000 text "$trg0001_1" }
//!         { time 3000 length 2200 text "$trg0001_2" }
//!     }
//!     sub trg0002 { text "$trg0002" }
//! }
//! ```
//!
//! `multisub` entries carry explicit millisecond timings; a bare `sub` shows
//! its single line for the length of the clip (which we approximate from the
//! text, see [`SubtitleCue::display_length`]). Cues are keyed by the **sample
//! name** the sound system plays (`trg0001` -> `trg0001.wav`), matched
//! case-insensitively.

use std::collections::HashMap;
use std::time::Duration;

/// The KEX localization table: `$key = "value"` lines, `//` comments and
/// `[section]` headers.
#[derive(Debug, Default)]
pub struct Localization {
    entries: HashMap<String, String>,
}

impl Localization {
    pub fn parse(source: &str) -> Self {
        let mut entries = HashMap::new();
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let Some(key) = key.strip_prefix('$') else {
                continue;
            };
            let value = value.trim();
            // The value is everything between the first and last quote, with
            // `\"` escapes unescaped.
            let Some(first) = value.find('"') else {
                continue;
            };
            let Some(last) = value.rfind('"') else {
                continue;
            };
            if last <= first {
                continue;
            }
            let raw = &value[first + 1..last];
            entries.insert(key.to_lowercase(), raw.replace("\\\"", "\""));
        }
        Self { entries }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(&key.to_lowercase()).map(String::as_str)
    }

    /// Resolve a `.sub` text token: `$key` looks up the table (`None` when the
    /// key is unknown - a raw `$trg0001_1` on screen is worse than no line),
    /// anything else is literal text.
    fn resolve(&self, token: &str) -> Option<String> {
        match token.strip_prefix('$') {
            Some(key) => self.get(key).map(str::to_owned),
            None => Some(token.to_owned()),
        }
    }
}

/// One line of a sample's subtitle track.
#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleCue {
    /// When the line appears, from the start of the sample.
    pub start: Duration,
    /// How long it stays, when the file says (`multisub`). `None` for a bare
    /// `sub` line, whose duration tracks the clip we cannot measure here.
    pub length: Option<Duration>,
    pub text: String,
}

impl SubtitleCue {
    /// How long the line is on screen: the authored length, or a reading-time
    /// estimate for untimed lines (bare `sub` entries).
    pub fn display_length(&self) -> Duration {
        self.length.unwrap_or_else(|| {
            let reading = Duration::from_millis(60 * self.text.chars().count() as u64);
            reading.max(Duration::from_millis(2500))
        })
    }

    pub fn end(&self) -> Duration {
        self.start + self.display_length()
    }
}

/// Subtitle cues per sound sample, lowercased sample name as the key.
#[derive(Debug, Default)]
pub struct SubtitleDb {
    by_sample: HashMap<String, Vec<SubtitleCue>>,
}

impl SubtitleDb {
    /// Parse one `.sub` source and merge its cues, resolving text through
    /// `loc`. Malformed trailing input is ignored rather than an error: a
    /// subtitle file must never take the game down.
    pub fn add_sub_source(&mut self, source: &str, loc: &Localization) {
        let mut tokens = Tokenizer::new(source);
        while let Some(token) = tokens.next() {
            match token {
                Token::Word(word) if word.eq_ignore_ascii_case("sub") => {
                    self.parse_sub(&mut tokens, loc);
                }
                Token::Word(word) if word.eq_ignore_ascii_case("multisub") => {
                    self.parse_multisub(&mut tokens, loc);
                }
                // Header (`SUB1`), block braces, `type`/`descr`/`singleton`
                // metadata and their values: skipped. Only the cue-bearing
                // entries matter here.
                _ => {}
            }
        }
    }

    /// `sub NAME { text "$key" }`
    fn parse_sub(&mut self, tokens: &mut Tokenizer, loc: &Localization) {
        let Some(Token::Word(name)) = tokens.next() else {
            return;
        };
        let Some(fields) = parse_fields(tokens) else {
            return;
        };
        let Some(text) = fields.text.as_deref().and_then(|t| loc.resolve(t)) else {
            return;
        };
        self.by_sample.insert(
            name.to_lowercase(),
            vec![SubtitleCue {
                start: Duration::ZERO,
                length: None,
                text,
            }],
        );
    }

    /// `multisub NAME { { time N length N text "$key" } ... }`
    fn parse_multisub(&mut self, tokens: &mut Tokenizer, loc: &Localization) {
        let Some(Token::Word(name)) = tokens.next() else {
            return;
        };
        if tokens.next() != Some(Token::Open) {
            return;
        }
        let mut cues = Vec::new();
        loop {
            match tokens.next() {
                Some(Token::Open) => {
                    let Some(fields) = parse_fields_open(tokens) else {
                        return;
                    };
                    if let Some(text) = fields.text.as_deref().and_then(|t| loc.resolve(t)) {
                        cues.push(SubtitleCue {
                            start: Duration::from_millis(fields.time.unwrap_or(0)),
                            length: fields.length.map(Duration::from_millis),
                            text,
                        });
                    }
                }
                Some(Token::Close) => break,
                Some(_) => {}
                None => return,
            }
        }
        if !cues.is_empty() {
            cues.sort_by_key(|cue| cue.start);
            self.by_sample.insert(name.to_lowercase(), cues);
        }
    }

    pub fn cues(&self, sample: &str) -> Option<&[SubtitleCue]> {
        self.by_sample
            .get(&sample.to_lowercase())
            .map(Vec::as_slice)
    }

    /// Number of samples with a transcript (the startup log line).
    pub fn len(&self) -> usize {
        self.by_sample.len()
    }
}

/// The recognized fields of a cue entry, in any order.
#[derive(Default)]
struct CueFields {
    time: Option<u64>,
    length: Option<u64>,
    text: Option<String>,
}

/// Parse `{ field value ... }` where the opening brace is still pending.
fn parse_fields(tokens: &mut Tokenizer) -> Option<CueFields> {
    if tokens.next() != Some(Token::Open) {
        return None;
    }
    parse_fields_open(tokens)
}

/// Parse field/value pairs until the matching `}`; the `{` has been consumed.
fn parse_fields_open(tokens: &mut Tokenizer) -> Option<CueFields> {
    let mut fields = CueFields::default();
    loop {
        match tokens.next()? {
            Token::Close => return Some(fields),
            Token::Word(word) if word.eq_ignore_ascii_case("time") => {
                fields.time = tokens.next_number();
            }
            Token::Word(word) if word.eq_ignore_ascii_case("length") => {
                fields.length = tokens.next_number();
            }
            Token::Word(word) if word.eq_ignore_ascii_case("text") => {
                if let Some(Token::Str(text) | Token::Word(text)) = tokens.next() {
                    fields.text = Some(text);
                }
            }
            // Unknown field or stray value: skip.
            _ => {}
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    Word(String),
    Str(String),
}

struct Tokenizer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Tokenizer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().peekable(),
        }
    }

    fn next_number(&mut self) -> Option<u64> {
        match self.next()? {
            Token::Word(word) | Token::Str(word) => word.parse().ok(),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<Token> {
        loop {
            let c = *self.chars.peek()?;
            if c.is_whitespace() {
                self.chars.next();
                continue;
            }
            if c == '/' {
                // `//` comment to end of line (a lone `/` cannot start
                // anything meaningful in this format either).
                for c in self.chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
        let c = self.chars.next()?;
        match c {
            '{' => Some(Token::Open),
            '}' => Some(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = self.chars.next() {
                    match c {
                        '\\' => {
                            if let Some(escaped) = self.chars.next() {
                                s.push(escaped);
                            }
                        }
                        '"' => break,
                        _ => s.push(c),
                    }
                }
                Some(Token::Str(s))
            }
            _ => {
                let mut word = String::new();
                word.push(c);
                while let Some(&c) = self.chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    word.push(c);
                    self.chars.next();
                }
                Some(Token::Word(word))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOC: &str = r#"
// Stringtable strings
[rmlui]
$back = "Back"
$trg0001_1 = "Welcome to the Ramsey Center UNN Recruitment Facility."
$trg0001_2 = "Please watch your step when leaving the train."
$trg0002 = "Step into the gravshafts."
$quoted = "He said \"hello\" twice."
"#;

    const SUB: &str = r#"SUB1
{
	type "convo"
	descr "$name_PA"
	singleton
	multisub trg0001 {
		{ time 00 length 3000 text "$trg0001_1" }
		{ time 3000 length 2200 text "$trg0001_2" }
	}
	sub trg0002 { text "$trg0002" }
}
"#;

    fn db() -> SubtitleDb {
        let loc = Localization::parse(LOC);
        let mut db = SubtitleDb::default();
        db.add_sub_source(SUB, &loc);
        db
    }

    #[test]
    fn localization_parses_keys_comments_and_escapes() {
        let loc = Localization::parse(LOC);
        assert_eq!(loc.get("back"), Some("Back"));
        assert_eq!(
            loc.get("TRG0001_1").map(|s| s.split(' ').next().unwrap()),
            Some("Welcome")
        );
        assert_eq!(loc.get("quoted"), Some("He said \"hello\" twice."));
        assert_eq!(loc.get("rmlui"), None, "section headers are not entries");
    }

    #[test]
    fn multisub_yields_timed_cues() {
        let db = db();
        let cues = db.cues("trg0001").expect("trg0001 parsed");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Duration::ZERO);
        assert_eq!(cues[0].length, Some(Duration::from_millis(3000)));
        assert_eq!(
            cues[0].text,
            "Welcome to the Ramsey Center UNN Recruitment Facility."
        );
        assert_eq!(cues[1].start, Duration::from_millis(3000));
    }

    #[test]
    fn sample_lookup_is_case_insensitive() {
        let db = db();
        assert!(db.cues("TRG0001").is_some());
    }

    #[test]
    fn bare_sub_yields_untimed_cue_with_reading_time() {
        let db = db();
        let cues = db.cues("trg0002").expect("trg0002 parsed");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].length, None);
        assert!(cues[0].display_length() >= Duration::from_millis(2500));
    }

    #[test]
    fn unknown_loc_key_drops_the_cue_not_the_track() {
        let loc = Localization::parse(LOC);
        let mut db = SubtitleDb::default();
        db.add_sub_source(
            r#"SUB1 {
                multisub half {
                    { time 0 length 1000 text "$trg0001_1" }
                    { time 1000 length 1000 text "$missing_key" }
                }
            }"#,
            &loc,
        );
        let cues = db.cues("half").expect("known cue kept");
        assert_eq!(cues.len(), 1, "a raw $token must never reach the screen");
    }

    #[test]
    fn malformed_input_is_ignored_without_panicking() {
        let loc = Localization::parse(LOC);
        let mut db = SubtitleDb::default();
        db.add_sub_source("SUB1 { multisub broken { { time 5 ", &loc);
        db.add_sub_source("{}}}{{ sub } garbage \"", &loc);
        assert!(db.cues("broken").is_none());
    }

    #[test]
    fn literal_text_without_dollar_is_kept_verbatim() {
        let loc = Localization::parse("");
        let mut db = SubtitleDb::default();
        db.add_sub_source(r#"SUB1 { sub plain { text "Just words" } }"#, &loc);
        assert_eq!(db.cues("plain").unwrap()[0].text, "Just words");
    }
}
