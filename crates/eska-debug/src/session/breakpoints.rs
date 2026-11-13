//! Translate breakpoint arguments into the platform's conditional stop and output fields.
use serde_json::Value;

use super::{Failure, INVALID, optional_expression};
use crate::dbgs::Breakpoint;

const BAD_HIT_COUNT: Failure = "debug-invalid-hit-count";
const BAD_LOG_TEMPLATE: Failure = "debug-invalid-log-template";

pub(super) fn parse(entry: &Value, line: u32) -> Result<Breakpoint, Failure> {
    let condition = optional_expression(entry, "condition")?.map(str::to_owned);
    let hit_count = optional_expression(entry, "hitCondition")?
        .map(|text| {
            text.trim()
                .parse::<i32>()
                .ok()
                .filter(|count| *count > 0)
                .ok_or(BAD_HIT_COUNT)
        })
        .transpose()?;
    let log_expression = optional_expression(entry, "logMessage")?
        .map(log_expression)
        .transpose()?;
    Ok(Breakpoint {
        line,
        condition,
        hit_count,
        log_expression,
    })
}

fn log_expression(template: &str) -> Result<String, Failure> {
    let mut chars = template.char_indices().peekable();
    let mut parts = Vec::new();
    let mut literal = String::new();
    while let Some((index, character)) = chars.next() {
        match character {
            '{' | '}' if chars.next_if(|&(_, next)| next == character).is_some() => {
                literal.push(character);
            }
            '{' => {
                push_literal(&mut parts, &mut literal);
                let mut in_string = false;
                let mut end = None;
                for (position, symbol) in chars.by_ref() {
                    match symbol {
                        '"' => in_string = !in_string,
                        '{' if !in_string => return Err(BAD_LOG_TEMPLATE),
                        '}' if !in_string => {
                            end = Some(position);
                            break;
                        }
                        _ => {}
                    }
                }
                let expression = template[index + 1..end.ok_or(BAD_LOG_TEMPLATE)?].trim();
                if expression.is_empty() || expression.chars().any(char::is_control) {
                    return Err(BAD_LOG_TEMPLATE);
                }
                parts.push(format!("Строка(({expression}))"));
            }
            '}' => return Err(BAD_LOG_TEMPLATE),
            control if control.is_control() => {
                push_literal(&mut parts, &mut literal);
                parts.push(format!("Символ({})", u32::from(control)));
            }
            _ => literal.push(character),
        }
    }
    push_literal(&mut parts, &mut literal);
    let result = parts.join(" + ");
    if result.is_empty() || result.len() > 65536 {
        return Err(INVALID);
    }
    Ok(result)
}

fn push_literal(parts: &mut Vec<String>, literal: &mut String) {
    if !literal.is_empty() {
        parts.push(format!("\"{}\"", literal.replace('"', "\"\"")));
        literal.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_template_preserves_text_and_compiles_bsl_interpolations() {
        assert_eq!(
            log_expression("текст \"целиком\"").unwrap(),
            "\"текст \"\"целиком\"\"\""
        );
        assert_eq!(
            log_expression("{{Сумма}} = {Сумма + 1}\n{\"}\"}").unwrap(),
            "\"{Сумма} = \" + Строка((Сумма + 1)) + Символ(10) + Строка((\"}\"))",
        );
        for invalid in ["text {", "{}", "{A{B}}", "text }", "{\"unfinished}"] {
            assert_eq!(log_expression(invalid), Err(BAD_LOG_TEMPLATE));
        }
    }
}
