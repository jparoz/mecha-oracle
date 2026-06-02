use crate::types::AbilityAST;

/// Parses Oracle text into ability AST nodes.
///
/// Phase 1 returns an empty Vec for all input — vanilla creatures have no abilities.
/// The function exists now so CardDefinition always carries a parsed AST, making
/// Phase 2 (adding keyword parsing) an extension rather than a refactor.
pub fn parse_oracle_text(text: &str) -> Vec<AbilityAST> {
    let _ = text;
    // Phase 2+: parse keywords, activated abilities, triggered abilities.
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_oracle_text_returns_no_abilities() {
        assert!(parse_oracle_text("").is_empty());
    }

    #[test]
    fn non_empty_oracle_text_returns_no_abilities_in_phase_1() {
        // Phase 1 intentionally ignores all text — vanilla-only scope.
        assert!(parse_oracle_text("Flying").is_empty());
        assert!(parse_oracle_text("Trample\nWhen ~ enters, draw a card.").is_empty());
    }
}
