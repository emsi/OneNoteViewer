use crate::{Error, Result};

pub(crate) struct PreparedQuery {
    pub(crate) fts: String,
    pub(crate) plain_terms: Vec<String>,
}

pub(crate) fn prepare_query(input: &str) -> Result<PreparedQuery> {
    if input.len() > 4096 {
        return Err(Error::InvalidQuery {
            message: "query text exceeds 4096 bytes".to_owned(),
        });
    }
    let mut tokens = Vec::new();
    let mut terms = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        if character.is_whitespace() {
            continue;
        }
        if character == '"' {
            let mut phrase = String::new();
            let mut closed = false;
            for (_, inner) in chars.by_ref() {
                if inner == '"' {
                    closed = true;
                    break;
                }
                if inner.is_control() {
                    return invalid_character();
                }
                phrase.push(inner);
            }
            if !closed {
                return Err(Error::InvalidQuery {
                    message: "quoted phrase is not closed".to_owned(),
                });
            }
            let phrase = phrase.trim();
            if phrase.is_empty() {
                return Err(Error::InvalidQuery {
                    message: "quoted phrase cannot be empty".to_owned(),
                });
            }
            terms.extend(phrase.split_whitespace().map(str::to_owned));
            tokens.push(format!("\"{}\"", phrase.replace('"', "\"\"")));
            continue;
        }

        let mut token = String::from(character);
        while let Some((_, next)) = chars.peek() {
            if next.is_whitespace() {
                break;
            }
            token.push(*next);
            chars.next();
        }
        if token.chars().any(char::is_control) {
            return invalid_character();
        }
        let prefix = token.ends_with('*');
        let term = token.strip_suffix('*').unwrap_or(&token);
        if term.is_empty() || term.contains('*') || term.contains('"') {
            return Err(Error::InvalidQuery {
                message: "only one trailing * prefix marker is supported".to_owned(),
            });
        }
        terms.push(term.to_owned());
        tokens.push(format!(
            "\"{}\"{}",
            term.replace('"', "\"\""),
            if prefix { "*" } else { "" }
        ));
    }
    if tokens.is_empty() {
        return Err(Error::InvalidQuery {
            message: "query text cannot be empty".to_owned(),
        });
    }
    Ok(PreparedQuery {
        fts: tokens.join(" AND "),
        plain_terms: terms,
    })
}

fn invalid_character<T>() -> Result<T> {
    Err(Error::InvalidQuery {
        message: "query contains a control character".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::prepare_query;

    #[test]
    fn quotes_all_terms_and_preserves_valid_prefixes() {
        let query = prepare_query("alpha \"two words\" pref*").expect("valid query");
        assert_eq!(query.fts, "\"alpha\" AND \"two words\" AND \"pref\"*");
        assert_eq!(query.plain_terms, ["alpha", "two", "words", "pref"]);
    }

    #[test]
    fn rejects_unclosed_phrases_and_infix_wildcards() {
        assert!(prepare_query("\"open").is_err());
        assert!(prepare_query("a*b").is_err());
    }
}
