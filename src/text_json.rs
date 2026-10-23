use std::error::Error;
use std::fmt;

pub const MAX_UTILITY_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_UTILITY_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonKind {
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

impl JsonKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Boolean => "BOOLEAN",
            Self::Number => "NUMBER",
            Self::String => "STRING",
            Self::Array => "ARRAY",
            Self::Object => "OBJECT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextJsonError {
    InputTooLarge,
    OutputTooLarge,
    InvalidJson { line: usize, column: usize },
}

impl fmt::Display for TextJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(
                formatter,
                "utility input exceeds the {} byte limit",
                MAX_UTILITY_INPUT_BYTES
            ),
            Self::OutputTooLarge => write!(
                formatter,
                "utility output exceeds the {} byte limit",
                MAX_UTILITY_OUTPUT_BYTES
            ),
            Self::InvalidJson { line, column } => {
                write!(formatter, "invalid JSON at line {line} column {column}")
            }
        }
    }
}

impl Error for TextJsonError {}

pub fn normalize_line_endings(input: &str) -> Result<String, TextJsonError> {
    validate_input_size(input)?;

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            push_char_checked(&mut output, '\n')?;
        } else {
            push_char_checked(&mut output, ch)?;
        }
    }

    Ok(output)
}

pub fn clean_text_lines(input: &str) -> Result<String, TextJsonError> {
    let normalized = normalize_line_endings(input)?;
    let mut output = String::with_capacity(normalized.len());

    for line in normalized.split_inclusive('\n') {
        let has_newline = line.ends_with('\n');
        let body = if has_newline {
            &line[..line.len() - 1]
        } else {
            line
        };
        let trimmed = body.trim_end_matches([' ', '\t']);
        push_str_checked(&mut output, trimmed)?;
        if has_newline {
            push_char_checked(&mut output, '\n')?;
        }
    }

    Ok(output)
}

pub fn validate_json(input: &str) -> Result<JsonKind, TextJsonError> {
    validate_input_size(input)?;
    let value: serde_json::Value = serde_json::from_str(input).map_err(json_error)?;
    Ok(match value {
        serde_json::Value::Null => JsonKind::Null,
        serde_json::Value::Bool(_) => JsonKind::Boolean,
        serde_json::Value::Number(_) => JsonKind::Number,
        serde_json::Value::String(_) => JsonKind::String,
        serde_json::Value::Array(_) => JsonKind::Array,
        serde_json::Value::Object(_) => JsonKind::Object,
    })
}

pub fn pretty_json(input: &str) -> Result<String, TextJsonError> {
    validate_json(input)?;
    format_json_whitespace(input, true)
}

pub fn compact_json(input: &str) -> Result<String, TextJsonError> {
    validate_json(input)?;
    format_json_whitespace(input, false)
}

fn validate_input_size(input: &str) -> Result<(), TextJsonError> {
    if input.len() > MAX_UTILITY_INPUT_BYTES {
        Err(TextJsonError::InputTooLarge)
    } else {
        Ok(())
    }
}

fn json_error(error: serde_json::Error) -> TextJsonError {
    TextJsonError::InvalidJson {
        line: error.line(),
        column: error.column(),
    }
}

fn format_json_whitespace(input: &str, pretty: bool) -> Result<String, TextJsonError> {
    let mut output = String::with_capacity(input.len().min(MAX_UTILITY_OUTPUT_BYTES));
    let mut in_string = false;
    let mut escaped = false;
    let mut indent = 0_usize;
    let mut last_significant = None;

    let mut chars = input.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if in_string {
            push_char_checked(&mut output, ch)?;

            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                last_significant = Some('"');
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                escaped = false;
                push_char_checked(&mut output, ch)?;
            }
            ' ' | '\n' | '\r' | '\t' => {}
            '{' | '[' => {
                push_char_checked(&mut output, ch)?;
                last_significant = Some(ch);

                if pretty {
                    indent = indent.saturating_add(1);
                    let closing = if ch == '{' { '}' } else { ']' };
                    if next_non_json_whitespace(&input[index + ch.len_utf8()..]) != Some(closing) {
                        push_char_checked(&mut output, '\n')?;
                        push_indent(&mut output, indent)?;
                    }
                }
            }
            '}' | ']' => {
                if pretty {
                    indent = indent.saturating_sub(1);
                    let opening = if ch == '}' { '{' } else { '[' };
                    if last_significant != Some(opening) {
                        if !output.ends_with('\n') {
                            push_char_checked(&mut output, '\n')?;
                        }
                        push_indent(&mut output, indent)?;
                    }
                }

                push_char_checked(&mut output, ch)?;
                last_significant = Some(ch);
            }
            ',' => {
                push_char_checked(&mut output, ch)?;
                last_significant = Some(ch);
                if pretty {
                    push_char_checked(&mut output, '\n')?;
                    push_indent(&mut output, indent)?;
                }
            }
            ':' => {
                push_char_checked(&mut output, ch)?;
                last_significant = Some(ch);
                if pretty {
                    push_char_checked(&mut output, ' ')?;
                }
            }
            _ => {
                push_char_checked(&mut output, ch)?;
                last_significant = Some(ch);
            }
        }
    }

    Ok(output)
}

fn next_non_json_whitespace(input: &str) -> Option<char> {
    input.chars().find(|ch| !matches!(ch, ' ' | '\n' | '\r' | '\t'))
}

fn push_indent(output: &mut String, indent: usize) -> Result<(), TextJsonError> {
    for _ in 0..indent {
        push_str_checked(output, "  ")?;
    }
    Ok(())
}

fn push_char_checked(output: &mut String, ch: char) -> Result<(), TextJsonError> {
    if output.len().saturating_add(ch.len_utf8()) > MAX_UTILITY_OUTPUT_BYTES {
        return Err(TextJsonError::OutputTooLarge);
    }
    output.push(ch);
    Ok(())
}

fn push_str_checked(output: &mut String, value: &str) -> Result<(), TextJsonError> {
    if output.len().saturating_add(value.len()) > MAX_UTILITY_OUTPUT_BYTES {
        return Err(TextJsonError::OutputTooLarge);
    }
    output.push_str(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_crlf_and_bare_cr_without_touching_other_text() {
        assert_eq!(
            normalize_line_endings("alpha\r\nbeta\rgamma\n").expect("normalize"),
            "alpha\nbeta\ngamma\n"
        );
    }

    #[test]
    fn clean_text_normalizes_lines_and_removes_only_trailing_space_and_tab() {
        assert_eq!(
            clean_text_lines(" alpha  \r\n\tbeta\t\rfinal  ").expect("clean"),
            " alpha\n\tbeta\nfinal"
        );
    }

    #[test]
    fn validates_json_and_reports_root_kind() {
        assert_eq!(
            validate_json(r#"{"b":2,"a":1}"#).expect("object"),
            JsonKind::Object
        );
        assert_eq!(validate_json("[1,2]").expect("array"), JsonKind::Array);
        assert_eq!(validate_json("true").expect("bool"), JsonKind::Boolean);
    }

    #[test]
    fn invalid_json_reports_location_without_echoing_input() {
        let error = validate_json("{\n  \"secret\": }").unwrap_err();
        assert!(matches!(error, TextJsonError::InvalidJson { .. }));
        let message = error.to_string();
        assert!(message.contains("invalid JSON at line"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn pretty_json_preserves_key_order_duplicates_and_number_spelling() {
        let input = r#"{"z":1e2,"a":1,"a":2,"nested":{"b":true}}"#;
        let pretty = pretty_json(input).expect("pretty");

        assert_eq!(
            pretty,
            "{\n  \"z\": 1e2,\n  \"a\": 1,\n  \"a\": 2,\n  \"nested\": {\n    \"b\": true\n  }\n}"
        );
    }

    #[test]
    fn compact_json_removes_only_insignificant_whitespace() {
        let input = "{ \n \"text\" : \"a b\\n c\" , \"n\" : 1.00 }";
        assert_eq!(
            compact_json(input).expect("compact"),
            "{\"text\":\"a b\\n c\",\"n\":1.00}"
        );
    }

    #[test]
    fn pretty_and_compact_round_trip_as_valid_json() {
        let input = r#"{"items":[1,{"x":"y"}],"empty":{},"list":[]}"#;
        let pretty = pretty_json(input).expect("pretty");
        let compact = compact_json(&pretty).expect("compact");

        assert!(validate_json(&pretty).is_ok());
        assert!(validate_json(&compact).is_ok());
        assert_eq!(compact, input);
    }

    #[test]
    fn rejects_input_over_limit_before_json_parse() {
        let input = " ".repeat(MAX_UTILITY_INPUT_BYTES + 1);
        assert_eq!(
            validate_json(&input).unwrap_err(),
            TextJsonError::InputTooLarge
        );
    }

    #[test]
    fn empty_objects_and_arrays_stay_compact_in_pretty_output() {
        assert_eq!(
            pretty_json(r#"{"object":{},"array":[]}"#).expect("pretty"),
            "{\n  \"object\": {},\n  \"array\": []\n}"
        );
    }
}
