//! Insert one checked event procedure without rewriting the surrounding module or its line endings.

use super::{FormHandlerEditSource, HandlerLookupError, authoring::HandlerTemplate, handler_range};
use crate::project::{
    metadata_edit::Replacement,
    query::{self, bsl},
};
use std::{fmt::Write, ops::Range};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerEditError {
    Lookup(HandlerLookupError),
    Unavailable,
    InvalidRegions,
}

#[derive(Debug)]
pub struct HandlerEditPlan {
    pub range: Range<usize>,
    pub change: Option<Replacement>,
}

/// Repeated requests only navigate once the procedure exists.
///
/// # Errors
/// Rejects incomplete/ambiguous BSL, unsafe region boundaries and unavailable signatures.
pub fn handler_edit_plan(
    input: &str,
    source: &FormHandlerEditSource,
) -> Result<HandlerEditPlan, HandlerEditError> {
    if let Some(range) =
        handler_range(input, &source.source.name).map_err(HandlerEditError::Lookup)?
    {
        if !source.bound {
            let template = source
                .template
                .as_ref()
                .ok_or(HandlerEditError::Unavailable)?;
            if !compatible_handler(input, &range, template.parameters) {
                return Err(HandlerEditError::Unavailable);
            }
        }
        return Ok(HandlerEditPlan {
            range,
            change: None,
        });
    }
    if !source.local {
        return Err(HandlerEditError::Unavailable);
    }
    let template = source
        .template
        .as_ref()
        .ok_or(HandlerEditError::Unavailable)?;
    insert(input, template)
}

fn compatible_handler(input: &str, range: &Range<usize>, parameters: &str) -> bool {
    let Ok(tokens) = bsl::tokens(input) else {
        return false;
    };
    let Some(index) = tokens.iter().position(|token| &token.range == range) else {
        return false;
    };
    if index == 0
        || !matches!(
            input[tokens[index - 1].range.clone()]
                .to_lowercase()
                .as_str(),
            "procedure" | "процедура"
        )
    {
        return false;
    }
    let mut depth = 0usize;
    let mut count = 0usize;
    let mut has_parameter = false;
    for token in &tokens[index + 1..] {
        match token.kind {
            bsl::Kind::Punctuation(b'(') => depth += 1,
            bsl::Kind::Punctuation(b')') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            bsl::Kind::Punctuation(b',') if depth == 1 => count += 1,
            _ if depth == 1 => has_parameter = true,
            _ => {}
        }
    }
    let expected = if parameters.is_empty() {
        0
    } else {
        parameters.split(',').count()
    };
    count + usize::from(has_parameter) == expected
}

fn insert(input: &str, template: &HandlerTemplate) -> Result<HandlerEditPlan, HandlerEditError> {
    let (at, found) = placement(input, template)?;
    let newline =
        if input[..at].contains("\r\n") || !input[..at].contains('\n') && input.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
    let (procedure, end, region, end_region) = if template.english {
        ("Procedure", "EndProcedure", "#Region", "#EndRegion")
    } else {
        ("Процедура", "КонецПроцедуры", "#Область", "#КонецОбласти")
    };
    let mut replacement = String::new();
    if at > 0 && !input[..at].ends_with(['\n', '\r', '\u{feff}']) {
        replacement.push_str(newline);
    }
    if !found {
        let _ = write!(replacement, "{region} {}{newline}", template.region);
    }
    replacement.push_str(newline);
    replacement.push_str(template.directive);
    replacement.push_str(newline);
    replacement.push_str(procedure);
    replacement.push(' ');
    let start = at + replacement.len();
    replacement.push_str(&template.name);
    let range = start..start + template.name.len();
    let _ = write!(
        replacement,
        "({}){newline}{newline}\t{newline}{newline}{end}{newline}{newline}",
        template.parameters
    );
    if !found {
        let _ = write!(replacement, "{end_region}{newline}{newline}");
    }
    if input.len() + replacement.len() > query::MAX_SNAPSHOT_BYTES {
        return Err(HandlerEditError::Lookup(HandlerLookupError::TooLarge));
    }
    Ok(HandlerEditPlan {
        range,
        change: Some(Replacement {
            range: at..at,
            text: replacement,
        }),
    })
}

/// Only structural directives emitted by the BSL scanner can delimit a region.
fn placement(input: &str, template: &HandlerTemplate) -> Result<(usize, bool), HandlerEditError> {
    let tokens = bsl::tokens(input)
        .map_err(|_| HandlerEditError::Lookup(HandlerLookupError::InvalidModule))?;
    let mut regions = Regions::default();
    let mut method = false;
    let mut variables = false;
    let mut initialization = None;
    for (index, token) in tokens.iter().enumerate() {
        let text = &input[token.range.clone()];
        if token.kind == bsl::Kind::Other && text.starts_with('#') {
            regions.directive(input, token, method, template)?;
            continue;
        }
        let lower = text.to_lowercase();
        let member = index > 0 && tokens[index - 1].kind == bsl::Kind::Punctuation(b'.');
        if token.kind == bsl::Kind::Word && !member {
            match lower.as_str() {
                "procedure" | "процедура" | "function" | "функция" => {
                    method = true;
                    continue;
                }
                "endprocedure" | "конецпроцедуры" | "endfunction" | "конецфункции" =>
                {
                    method = false;
                    continue;
                }
                "var" | "перем" if !method => {
                    variables = true;
                    continue;
                }
                _ => {}
            }
        }
        if variables && token.kind == bsl::Kind::Punctuation(b';') {
            variables = false;
            continue;
        }
        if !(method || variables || token.kind == bsl::Kind::Other && text.starts_with('&')) {
            initialization.get_or_insert_with(|| {
                if regions.conditional_depth == 0 {
                    line_start(input, token.range.start)
                } else {
                    regions.conditional_start
                }
            });
        }
    }
    if !regions.stack.is_empty() || regions.conditional_depth != 0 || variables {
        return Err(HandlerEditError::InvalidRegions);
    }
    if let Some(at) = regions.found {
        if initialization.is_some_and(|start| at > start) {
            return Err(HandlerEditError::InvalidRegions);
        }
        return Ok((at, true));
    }
    let at = regions
        .private_region
        .into_iter()
        .chain(initialization)
        .min()
        .unwrap_or(input.len());
    Ok((
        at.max(if input.starts_with('\u{feff}') { 3 } else { 0 }),
        false,
    ))
}

#[derive(Default)]
struct Regions {
    stack: Vec<(String, bool)>,
    found: Option<usize>,
    private_region: Option<usize>,
    conditional_depth: usize,
    conditional_start: usize,
}

impl Regions {
    fn directive(
        &mut self,
        input: &str,
        token: &bsl::Token,
        method: bool,
        template: &HandlerTemplate,
    ) -> Result<(), HandlerEditError> {
        let text = &input[token.range.clone()];
        let mut words = text.split("//").next().unwrap_or("").split_whitespace();
        match words.next().unwrap_or("").to_lowercase().as_str() {
            "#region" | "#область" => {
                let Some(name) = words.next().filter(|name| super::identifier(name)) else {
                    return Err(HandlerEditError::InvalidRegions);
                };
                if words.next().is_some() {
                    return Err(HandlerEditError::InvalidRegions);
                }
                let eligible = !method && self.conditional_depth == 0;
                if eligible
                    && self.stack.is_empty()
                    && ["private", "служебныепроцедурыифункции"]
                        .contains(&name.to_lowercase().as_str())
                {
                    self.private_region
                        .get_or_insert_with(|| line_start(input, token.range.start));
                }
                self.stack.push((name.to_owned(), eligible));
            }
            "#endregion" | "#конецобласти" => {
                let Some((name, eligible)) = self.stack.pop() else {
                    return Err(HandlerEditError::InvalidRegions);
                };
                let names = [&template.region, &template.alternate_region];
                let same = names
                    .iter()
                    .any(|expected| name.to_lowercase() == expected.to_lowercase())
                    || names
                        .iter()
                        .any(|name| name.as_str() == "ОбработчикиСобытийЭлементовШапкиФормы")
                        && name == "ОбработчикиСобытийЭлементовФормы";
                if same {
                    if !eligible || method || self.conditional_depth > 0 || self.found.is_some() {
                        return Err(HandlerEditError::InvalidRegions);
                    }
                    self.found = Some(line_start(input, token.range.start));
                }
            }
            "#if" | "#если" => {
                if self.conditional_depth == 0 {
                    self.conditional_start = line_start(input, token.range.start);
                }
                self.conditional_depth += 1;
            }
            "#endif" | "#конецесли" => {
                self.conditional_depth = self
                    .conditional_depth
                    .checked_sub(1)
                    .ok_or(HandlerEditError::InvalidRegions)?;
            }
            "#else" | "#иначе" | "#elseif" | "#иначеесли" if self.conditional_depth == 0 =>
            {
                return Err(HandlerEditError::InvalidRegions);
            }
            _ => {}
        }
        Ok(())
    }
}

fn line_start(input: &str, offset: usize) -> usize {
    input[..offset].rfind(['\r', '\n']).map_or_else(
        || if input.starts_with('\u{feff}') { 3 } else { 0 },
        |index| index + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn template() -> HandlerTemplate {
        HandlerTemplate {
            name: "ПриОткрытии".into(),
            parameters: "Отказ",
            directive: "&НаКлиенте",
            region: "ОбработчикиСобытийФормы".into(),
            alternate_region: "FormEventHandlers".into(),
            english: false,
        }
    }
    fn apply(input: &str, plan: HandlerEditPlan) -> String {
        let change = plan.change.unwrap();
        format!(
            "{}{}{}",
            &input[..change.range.start],
            change.text,
            &input[change.range.end..]
        )
    }
    #[test]
    fn reuses_bilingual_region_ignoring_comments_and_strings() {
        let input = "// #Область ОбработчикиСобытийФормы\n#Region FormEventHandlers\n&AtClient\nProcedure Other()\nText = \"#EndRegion\n|#Region FormEventHandlers\";\nEndProcedure\n#EndRegion\n";
        let plan = insert(input, &template()).unwrap();
        let range = plan.range.clone();
        let output = apply(input, plan);
        assert_eq!(&output[range], "ПриОткрытии");
        assert_eq!(output.matches("#Region FormEventHandlers").count(), 2);
        assert!(output.ends_with("КонецПроцедуры\n\n#EndRegion\n"));
    }
    #[test]
    fn inserts_region_before_private_or_initialization_preserving_bytes() {
        for input in [
            "\u{feff}// user\r\n#Область СлужебныеПроцедурыИФункции\nПроцедура Другая()\r\nКонецПроцедуры\n#КонецОбласти\r\n",
            "Перем МояПеременная;\r\nМояПеременная = 1;\r\n",
        ] {
            let plan = insert(input, &template()).unwrap();
            let change = plan.change.as_ref().unwrap();
            assert!(change.text.contains("Процедура ПриОткрытии(Отказ)\r\n"));
            let at = change.range.start;
            let output = apply(input, plan);
            assert!(output.starts_with(&input[..at]));
            assert!(output.ends_with(&input[at..]));
        }
    }
    #[test]
    fn binding_does_not_claim_a_same_named_function_or_wrong_arity_procedure() {
        for (source, compatible) in [
            ("Procedure Open(Cancel) EndProcedure", true),
            ("Procedure Open() EndProcedure", false),
            ("Function Open(Cancel) EndFunction", false),
            ("Procedure Open(Arg = F(1, 2), Second) EndProcedure", false),
        ] {
            let range = handler_range(source, "Open").unwrap().unwrap();
            assert_eq!(compatible_handler(source, &range, "Cancel"), compatible);
        }
    }
    #[test]
    fn new_region_precedes_conditional_initialization_and_retains_bom() {
        let input = "\u{feff}#If Client Then\nReady = True;\n#EndIf\n";
        let output = apply(input, insert(input, &template()).unwrap());
        assert!(output.starts_with("\u{feff}#Область ОбработчикиСобытийФормы\n"));
        assert!(output.ends_with("#If Client Then\nReady = True;\n#EndIf\n"));
    }
    #[test]
    fn rejects_unbalanced_duplicate_or_conditional_regions() {
        for input in [
            "#Region FormEventHandlers\n",
            "#EndRegion\n",
            "#Region FormEventHandlers\n#EndRegion\n#Область ОбработчикиСобытийФормы\n#КонецОбласти",
            "#If Client Then\n#Region FormEventHandlers\n#EndRegion\n#EndIf",
        ] {
            assert_eq!(
                insert(input, &template()).unwrap_err(),
                HandlerEditError::InvalidRegions,
                "{input}"
            );
        }
    }
}
