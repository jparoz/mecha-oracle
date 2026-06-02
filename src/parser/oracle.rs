use super::ParseError;
use crate::types::{AbilityAST, ability::StaticAbility};

/// Strip all parenthetical reminder text from oracle text before tokenising.
/// CR 305.6: parenthetical text on basic lands is reminder text, not rules text.
fn strip_reminder_text(text: &str) -> String {
    let mut result = String::new();
    let mut depth: usize = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => result.push(c),
            _ => {}
        }
    }
    result
}

/// Parse Oracle text into ability AST nodes.
/// Returns Err if any token is not a recognised keyword.
/// Blank tokens (blank lines, trailing commas) are silently skipped.
pub fn parse_oracle_text(text: &str) -> Result<Vec<AbilityAST>, ParseError> {
    let stripped = strip_reminder_text(text);
    let mut abilities = vec![];
    for token in stripped
        .split(['\n', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let kw = match token.to_lowercase().as_str() {
            "flying" => StaticAbility::Flying,
            "reach" => StaticAbility::Reach,
            "trample" => StaticAbility::Trample,
            "first strike" => StaticAbility::FirstStrike,
            "double strike" => StaticAbility::DoubleStrike,
            "vigilance" => StaticAbility::Vigilance,
            "haste" => StaticAbility::Haste,
            "lifelink" => StaticAbility::Lifelink,
            "deathtouch" => StaticAbility::Deathtouch,
            "menace" => StaticAbility::Menace,
            "indestructible" => StaticAbility::Indestructible,
            other => {
                return Err(ParseError::UnknownKeyword {
                    keyword: other.to_string(),
                    card_text: text.to_string(),
                });
            }
        };
        abilities.push(AbilityAST::Static(kw));
    }
    Ok(abilities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ability::StaticAbility;

    #[test]
    fn empty_text_returns_empty_vec() {
        assert_eq!(parse_oracle_text("").unwrap(), vec![]);
    }

    #[test]
    fn reminder_text_only_returns_empty_vec() {
        // Forest's oracle text: reminder text, not a keyword
        assert_eq!(parse_oracle_text("({T}: Add {G}.)").unwrap(), vec![]);
    }

    #[test]
    fn single_keyword_newline() {
        let result = parse_oracle_text("Flying").unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::Flying)]);
    }

    #[test]
    fn comma_separated_keywords() {
        let result = parse_oracle_text("Flying, vigilance").unwrap();
        assert_eq!(
            result,
            vec![
                AbilityAST::Static(StaticAbility::Flying),
                AbilityAST::Static(StaticAbility::Vigilance),
            ]
        );
    }

    #[test]
    fn two_word_keyword_first_strike() {
        let result = parse_oracle_text("First strike").unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::FirstStrike)]);
    }

    #[test]
    fn keyword_with_reminder_text_stripped() {
        // Deathtouch reminder text is stripped before tokenising
        let result = parse_oracle_text(
            "Deathtouch (Any amount of damage this deals to a creature is enough to destroy it.)",
        )
        .unwrap();
        assert_eq!(result, vec![AbilityAST::Static(StaticAbility::Deathtouch)]);
    }

    #[test]
    fn multiline_keywords() {
        let result = parse_oracle_text("Trample\nLifelink").unwrap();
        assert_eq!(
            result,
            vec![
                AbilityAST::Static(StaticAbility::Trample),
                AbilityAST::Static(StaticAbility::Lifelink),
            ]
        );
    }

    #[test]
    fn all_eleven_keywords_parse() {
        let text = "Flying\nReach\nTrample\nFirst strike\nDouble strike\nVigilance\nHaste\nLifelink\nDeathtouch\nMenace\nIndestructible";
        let result = parse_oracle_text(text).unwrap();
        assert_eq!(result.len(), 11);
    }

    #[test]
    fn unknown_keyword_returns_error() {
        let err = parse_oracle_text("Intimidate").unwrap_err();
        match err {
            ParseError::UnknownKeyword { keyword, .. } => assert_eq!(keyword, "intimidate"),
        }
    }

    #[test]
    fn triggered_ability_text_returns_error() {
        // Triggered ability text is not a Phase 2 keyword — should fail
        assert!(parse_oracle_text("When this creature enters, draw a card.").is_err());
    }
}
