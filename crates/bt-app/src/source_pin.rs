//! Reading this crate's own source, for the pins that are about what the code
//! *says* rather than about what it does.
//!
//! A pin of that kind is not a second-best test. Some of what this product
//! promises is a compile-time fact — which table a platform ships, which flag a
//! family of shells is told — and a runtime assertion can only ever check the
//! platform it is running on. `scripts/check-portable-core.ps1` makes the same
//! argument in its own header and reads source for the same reason: "a compile
//! failure names a symbol and a line, while a rule names the rule".
//!
//! Test-only, and the two functions here are the whole of it: find one item, and
//! take its comments out so that a paragraph *about* a thing is not read as a
//! line that names one.

/// The text of one item, from `header` to the line its body closes on.
///
/// Brace counting and not a parse, for `check-portable-core.ps1`'s own reason: a
/// gate that needed to build the crate in order to read it is a gate that cannot
/// run on a tree that does not build.
///
/// Panics when the header is not there, which is the point — an item renamed out
/// from under a pin must fail loudly rather than pass over nothing.
#[must_use]
pub fn source_region<'source>(source: &'source str, header: &str) -> &'source str {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("{header} is not in this file any more"));
    let rest = &source[start..];
    let mut depth = 0usize;
    let mut opened = false;
    for (at, character) in rest.char_indices() {
        match character {
            '{' => {
                depth += 1;
                opened = true;
            }
            '}' => {
                depth -= 1;
                if opened && depth == 0 {
                    return &rest[..=at];
                }
            }
            _ => {}
        }
    }
    panic!("{header} does not close");
}

/// The same text with its comments taken out.
///
/// A three-state walk — in a line comment, in a block comment, in a string — and
/// nothing more. It is deliberately not a lexer: the regions this is asked about
/// hold no raw string and no escaped quote, and a caller that needs those is a
/// caller that should be asking the compiler instead.
#[must_use]
pub fn code_of(region: &str) -> String {
    let mut code = String::with_capacity(region.len());
    let mut characters = region.chars().peekable();
    let mut in_string = false;
    while let Some(character) = characters.next() {
        if in_string {
            code.push(character);
            if character == '"' {
                in_string = false;
            }
            continue;
        }
        match (character, characters.peek()) {
            ('"', _) => {
                in_string = true;
                code.push(character);
            }
            ('/', Some('/')) => {
                for skipped in characters.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
                code.push('\n');
            }
            ('/', Some('*')) => {
                let mut previous = ' ';
                for skipped in characters.by_ref() {
                    if previous == '*' && skipped == '/' {
                        break;
                    }
                    previous = skipped;
                }
                code.push(' ');
            }
            _ => code.push(character),
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — the reader of the pins can itself be read wrong, so it is pinned
    /// too: an item is taken whole, and a comment about a word is not the word.
    #[test]
    fn one_item_is_taken_whole_and_its_comments_are_not_its_code() {
        let source = "\
fn before() { 1 }
/// A doc comment naming forbidden.
fn subject(a: u8) -> u8 {
    // a line comment naming forbidden
    let nested = { a + 1 }; /* a block naming forbidden */
    nested
}
fn after() { 2 }
";
        let region = source_region(source, "fn subject(");
        assert!(region.starts_with("fn subject(a: u8) -> u8 {"));
        assert!(region.ends_with('}'));
        assert!(
            region.contains("nested"),
            "the nested braces did not end it"
        );
        assert!(!region.contains("fn after"), "it ran past its own body");
        let code = code_of(region);
        assert!(!code.contains("forbidden"), "a comment was read as code");
        assert!(code.contains("let nested"));
        // A string holding the marks a comment is made of survives.
        assert_eq!(
            code_of(r#"let path = "//usr/*x*/";"#),
            r#"let path = "//usr/*x*/";"#
        );
    }
}
