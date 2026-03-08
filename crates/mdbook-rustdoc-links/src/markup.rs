use std::{collections::HashMap, fmt::Write, hash::Hash};

use lsp_types::Position;

use mdbookkit::error::ExpectFmt;

#[derive(Debug, Clone)]
pub struct AttributedString<K> {
    text: String,
    places: HashMap<K, Vec<Position>>,
    cursor: Position,
}

impl<K> AttributedString<K> {
    pub fn new() -> Self {
        Self::from("".to_owned())
    }

    pub fn map<K2>(self, mut f: impl FnMut(K) -> K2) -> AttributedString<K2>
    where
        K2: Eq + Hash,
    {
        let Self {
            text,
            places,
            cursor,
        } = self;
        let places = places.into_iter().map(|(k, v)| (f(k), v)).collect();
        AttributedString {
            text,
            places,
            cursor,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn into_parts(self) -> (String, HashMap<K, Vec<Position>>) {
        (self.text, self.places)
    }
}

impl<K> From<String> for AttributedString<K> {
    fn from(text: String) -> Self {
        let line = text.chars().filter(|&c| c == '\n').count() as _;
        let character = text
            .rsplit_once('\n')
            .map(|(_, last)| last.len())
            .unwrap_or(text.len()) as _;
        Self {
            text,
            cursor: Position { line, character },
            places: Default::default(),
        }
    }
}

impl<K: Eq + Hash> AttributedString<K> {
    pub fn markup(&mut self, key: K) {
        self.places.entry(key).or_default().push(self.cursor);
    }

    pub fn append(&mut self, other: Self) {
        let base = self.cursor;
        self.write_str(&other.text).expect_fmt();
        for (key, positions) in other.places {
            let offset = positions.into_iter().map(|pos| {
                if pos.line == 0 {
                    Position {
                        line: base.line,
                        character: base.character + pos.character,
                    }
                } else {
                    Position {
                        line: base.line + pos.line,
                        character: pos.character,
                    }
                }
            });
            self.places.entry(key).or_default().extend(offset);
        }
    }
}

impl<K: Eq + Hash, S: AsRef<str>> Extend<(S, Option<K>)> for AttributedString<K> {
    fn extend<T: IntoIterator<Item = (S, Option<K>)>>(&mut self, iter: T) {
        for (text, key) in iter {
            if let Some(key) = key {
                self.markup(key);
            }
            self.write_str(text.as_ref()).expect_fmt();
        }
    }
}

impl<K> Write for AttributedString<K> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.text.write_str(s)?;
        let newlines = s.chars().filter(|&c| c == '\n').count() as u32;
        if newlines > 0 {
            self.cursor.line += newlines;
            self.cursor.character =
                s.rsplit_once('\n').map(|(_, last)| last.len()).unwrap_or(0) as _;
        } else {
            self.cursor.character += s.len() as u32;
        }
        Ok(())
    }
}
