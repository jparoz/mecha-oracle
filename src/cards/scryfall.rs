use crate::parser::parse_oracle_text;
use crate::types::card::{CardDefinition, CardType, Supertype, TypeLine};
use crate::types::mana::ManaCost;
use serde_json::Value;

pub fn parse_card(v: &Value) -> Result<CardDefinition, String> {
    let name = v["name"].as_str().ok_or("missing name")?.to_string();

    let mana_cost = match v["mana_cost"].as_str() {
        Some(s) if !s.is_empty() => Some(parse_mana_cost(s)?),
        _ => None,
    };

    let type_line = v["type_line"]
        .as_str()
        .ok_or("missing type_line")
        .map(parse_type_line)?;

    let oracle_text = v["oracle_text"].as_str().unwrap_or("").to_string();

    let abilities = parse_oracle_text(&oracle_text);

    let power = v["power"].as_str().and_then(|s| s.parse::<i32>().ok());
    let toughness = v["toughness"].as_str().and_then(|s| s.parse::<i32>().ok());

    Ok(CardDefinition {
        name,
        mana_cost,
        type_line,
        oracle_text,
        abilities,
        power,
        toughness,
    })
}

fn parse_mana_cost(s: &str) -> Result<ManaCost, String> {
    let mut cost = ManaCost::default();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut token = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                token.push(inner);
            }
            match token.as_str() {
                "W" => cost.white += 1,
                "U" => cost.blue += 1,
                "B" => cost.black += 1,
                "R" => cost.red += 1,
                "G" => cost.green += 1,
                "C" => cost.colorless += 1,
                n => {
                    let n: u32 = n
                        .parse()
                        .map_err(|_| format!("unknown mana symbol {{{n}}}"))?;
                    cost.generic += n;
                }
            }
        }
    }
    Ok(cost)
}

fn parse_type_line(s: &str) -> TypeLine {
    let em_dash = '\u{2014}';
    let (left, right) = if let Some(idx) = s.find(em_dash) {
        let l = s[..idx].trim();
        let r = s[idx + em_dash.len_utf8()..].trim();
        (l, r)
    } else {
        (s.trim(), "")
    };

    let mut supertypes = Vec::new();
    let mut card_types = Vec::new();

    for token in left.split_whitespace() {
        match token {
            "Basic" => supertypes.push(Supertype::Basic),
            "Legendary" => supertypes.push(Supertype::Legendary),
            "Snow" => supertypes.push(Supertype::Snow),
            "World" => supertypes.push(Supertype::World),
            "Creature" => card_types.push(CardType::Creature),
            "Land" => card_types.push(CardType::Land),
            "Instant" => card_types.push(CardType::Instant),
            "Sorcery" => card_types.push(CardType::Sorcery),
            "Artifact" => card_types.push(CardType::Artifact),
            "Enchantment" => card_types.push(CardType::Enchantment),
            "Planeswalker" => card_types.push(CardType::Planeswalker),
            _ => {}
        }
    }

    let subtypes = if right.is_empty() {
        vec![]
    } else {
        right.split_whitespace().map(|s| s.to_string()).collect()
    };

    TypeLine {
        supertypes,
        card_types,
        subtypes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_grizzly_bears() {
        let v = json!({
            "name": "Grizzly Bears",
            "mana_cost": "{1}{G}",
            "type_line": "Creature \u{2014} Bear",
            "oracle_text": "",
            "power": "2",
            "toughness": "2"
        });
        let card = parse_card(&v).unwrap();
        assert_eq!(card.name, "Grizzly Bears");
        assert_eq!(card.power, Some(2));
        assert_eq!(card.toughness, Some(2));
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.generic, 1);
        assert_eq!(cost.green, 1);
        assert_eq!(cost.converted_mana_cost(), 2);
        assert!(card.type_line.is_creature());
        assert_eq!(card.type_line.subtypes, vec!["Bear"]);
    }

    #[test]
    fn parse_forest() {
        let v = json!({
            "name": "Forest",
            "mana_cost": "",
            "type_line": "Basic Land \u{2014} Forest",
            "oracle_text": "({T}: Add {G}.)"
        });
        let card = parse_card(&v).unwrap();
        assert!(card.mana_cost.is_none());
        assert!(card.type_line.is_land());
        assert!(!card.type_line.is_creature());
        assert!(
            card.type_line
                .supertypes
                .contains(&crate::types::card::Supertype::Basic)
        );
    }

    #[test]
    fn parse_hill_giant() {
        let v = json!({
            "name": "Hill Giant",
            "mana_cost": "{3}{R}",
            "type_line": "Creature \u{2014} Giant",
            "oracle_text": "",
            "power": "3",
            "toughness": "3"
        });
        let card = parse_card(&v).unwrap();
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.generic, 3);
        assert_eq!(cost.red, 1);
        assert_eq!(cost.converted_mana_cost(), 4);
    }

    #[test]
    fn non_numeric_power_is_none() {
        let v = json!({
            "name": "Tarmogoyf",
            "mana_cost": "{1}{G}",
            "type_line": "Creature \u{2014} Lhurgoyf",
            "oracle_text": "",
            "power": "*",
            "toughness": "*+1"
        });
        let card = parse_card(&v).unwrap();
        assert_eq!(card.power, None);
        assert_eq!(card.toughness, None);
    }
}
