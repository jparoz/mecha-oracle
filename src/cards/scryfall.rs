use crate::parser::{parse_instant_or_sorcery, parse_permanent};
use crate::types::card::{CardDefinition, CardType, Supertype, TypeLine};
use crate::types::mana::{ManaColor, ManaCost, ManaPip};
use serde_json::Value;

pub enum ParsedEntry {
    Card(CardDefinition),
    Token(CardDefinition),
    UnCard,  // Silver border or acorn stamp (CR 100.7); out of scope
    ArtCard, // Non-playable; out of scope
}

pub fn parse_entry(v: &Value) -> Result<ParsedEntry, String> {
    // CR 100.7: Un-cards (silver border, acorn stamp, or playtest cards) are out of scope
    if v["border_color"].as_str() == Some("silver")
        || v["security_stamp"].as_str() == Some("acorn")
        || {
            if let Some(promo_types) = v["promo_types"].as_array() {
                promo_types.contains(&Value::String("playtest".to_string()))
            } else {
                false
            }
        }
    {
        return Ok(ParsedEntry::UnCard);
    }

    // Art series cards are unplayable, out of scope
    if v["layout"].as_str() == Some("art_series") || v["set_type"].as_str() == Some("memorabilia") {
        return Ok(ParsedEntry::ArtCard);
    }

    let name = v["name"].as_str().ok_or("missing name")?.to_string();

    let _span = tracing::debug_span!("parsing", card = name).entered();

    let mana_cost = match v["mana_cost"].as_str() {
        Some(s) if !s.is_empty() => Some(parse_mana_cost(s)),
        _ => None,
    };

    let type_line = v["type_line"]
        .as_str()
        .ok_or("missing type_line")
        .map(parse_type_line)?;

    let oracle_text = v["oracle_text"].as_str().unwrap_or("").to_string();

    let abilities = if type_line
        .card_types
        .iter()
        .any(|t| matches!(t, CardType::Instant | CardType::Sorcery))
    {
        parse_instant_or_sorcery(&oracle_text, &name)
    } else {
        parse_permanent(&oracle_text, &name)
    };

    let power = v["power"].as_str().and_then(|s| s.parse::<i32>().ok());
    let toughness = v["toughness"].as_str().and_then(|s| s.parse::<i32>().ok());

    let def = CardDefinition {
        name,
        mana_cost,
        type_line,
        oracle_text,
        abilities,
        text_annotations: vec![],
        power,
        toughness,
    };

    match v["layout"].as_str() {
        Some("token") | Some("double_faced_token") | Some("emblem") => Ok(ParsedEntry::Token(def)),
        _ => Ok(ParsedEntry::Card(def)),
    }
}

fn color_from_str(s: &str) -> Option<ManaColor> {
    match s {
        "W" => Some(ManaColor::White),
        "U" => Some(ManaColor::Blue),
        "B" => Some(ManaColor::Black),
        "R" => Some(ManaColor::Red),
        "G" => Some(ManaColor::Green),
        "C" => Some(ManaColor::Colorless),
        _ => None,
    }
}

/// CR 107.4: parse every mana symbol; unknown symbols are silently skipped.
fn parse_mana_cost(s: &str) -> ManaCost {
    let mut pips = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            continue;
        }
        let mut token = String::new();
        for inner in chars.by_ref() {
            if inner == '}' {
                break;
            }
            token.push(inner);
        }
        let parts: Vec<&str> = token.split('/').collect();
        let pip = match parts.as_slice() {
            ["W"] => Some(ManaPip::White),
            ["U"] => Some(ManaPip::Blue),
            ["B"] => Some(ManaPip::Black),
            ["R"] => Some(ManaPip::Red),
            ["G"] => Some(ManaPip::Green),
            ["C"] => Some(ManaPip::Colorless),
            ["X"] => Some(ManaPip::X),
            ["S"] => Some(ManaPip::Snow),
            [n] => n.parse::<u32>().ok().map(ManaPip::Generic),
            [a, "P"] => color_from_str(a).map(ManaPip::Phyrexian),
            [a, b] => {
                let ca = color_from_str(a);
                let cb = color_from_str(b);
                match (ca, cb) {
                    (Some(_), Some(c2)) if *a == "C" => Some(ManaPip::ColorlessHybrid(c2)),
                    (Some(c1), Some(c2)) => Some(ManaPip::Hybrid(c1, c2)),
                    (None, Some(c2)) => {
                        a.parse::<u32>().ok().map(|n| ManaPip::GenericHybrid(n, c2))
                    }
                    _ => None,
                }
            }
            [a, b, "P"] => match (color_from_str(a), color_from_str(b)) {
                (Some(c1), Some(c2)) => Some(ManaPip::HybridPhyrexian(c1, c2)),
                _ => None,
            },
            _ => None,
        };
        match pip {
            Some(p) => pips.push(p),
            None => tracing::debug!(symbol = token, "skipping unknown mana symbol"),
        }
    }
    ManaCost { pips }
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
    use crate::types::mana::{ManaColor, ManaPip};
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
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        assert_eq!(card.name, "Grizzly Bears");
        assert_eq!(card.power, Some(2));
        assert_eq!(card.toughness, Some(2));
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.mana_value(), 2);
        assert!(cost.pips.contains(&ManaPip::Generic(1)));
        assert!(cost.pips.contains(&ManaPip::Green));
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
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
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
    fn parse_dryad_arbor_is_land_and_creature() {
        let v = json!({
            "name": "Dryad Arbor",
            "mana_cost": "",
            "type_line": "Land Creature \u{2014} Forest Dryad",
            "oracle_text": "",
            "power": "1",
            "toughness": "1"
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        assert!(card.mana_cost.is_none());
        assert!(card.type_line.is_land());
        assert!(card.type_line.is_creature());
        assert!(card.type_line.subtypes.contains(&"Forest".to_string()));
        assert_eq!(card.power, Some(1));
        assert_eq!(card.toughness, Some(1));
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
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.mana_value(), 4);
        assert!(cost.pips.contains(&ManaPip::Generic(3)));
        assert!(cost.pips.contains(&ManaPip::Red));
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
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        assert_eq!(card.power, None);
        assert_eq!(card.toughness, None);
    }

    #[test]
    fn parse_x_cost() {
        let v = json!({
            "name": "Fireball",
            "mana_cost": "{X}{R}",
            "type_line": "Sorcery",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert!(cost.pips.contains(&ManaPip::X));
        assert!(cost.pips.contains(&ManaPip::Red));
        assert_eq!(cost.mana_value(), 1); // X counts 0
    }

    #[test]
    fn parse_hybrid_cost() {
        let v = json!({
            "name": "Boggart Ram-Gang",
            "mana_cost": "{R/G}{R/G}{R/G}",
            "type_line": "Creature \u{2014} Goblin Warrior",
            "oracle_text": "Haste"
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.pips.len(), 3);
        assert!(
            cost.pips
                .iter()
                .all(|p| matches!(p, ManaPip::Hybrid(ManaColor::Red, ManaColor::Green)))
        );
        assert_eq!(cost.mana_value(), 3);
    }

    #[test]
    fn parse_phyrexian_cost() {
        let v = json!({
            "name": "Gitaxian Probe",
            "mana_cost": "{U/P}",
            "type_line": "Instant",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.pips, vec![ManaPip::Phyrexian(ManaColor::Blue)]);
        assert_eq!(cost.mana_value(), 1);
    }

    #[test]
    fn parse_hybrid_phyrexian_cost() {
        let v = json!({
            "name": "Test Card",
            "mana_cost": "{W/U/P}",
            "type_line": "Instant",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert_eq!(
            cost.pips,
            vec![ManaPip::HybridPhyrexian(ManaColor::White, ManaColor::Blue)]
        );
    }

    #[test]
    fn parse_generic_hybrid_cost() {
        let v = json!({
            "name": "Spectral Procession",
            "mana_cost": "{2/W}{2/W}{2/W}",
            "type_line": "Sorcery",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert_eq!(cost.pips.len(), 3);
        assert!(
            cost.pips
                .iter()
                .all(|p| matches!(p, ManaPip::GenericHybrid(2, ManaColor::White)))
        );
        assert_eq!(cost.mana_value(), 6);
    }

    #[test]
    fn parse_colorless_symbol_in_cost() {
        let v = json!({
            "name": "Spatial Contortion",
            "mana_cost": "{1}{C}",
            "type_line": "Instant",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert!(cost.pips.contains(&ManaPip::Colorless));
    }

    #[test]
    fn parse_snow_cost() {
        let v = json!({
            "name": "Skred",
            "mana_cost": "{S}{R}",
            "type_line": "Instant",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        assert!(cost.pips.contains(&ManaPip::Snow));
        assert!(cost.pips.contains(&ManaPip::Red));
    }

    #[test]
    fn unknown_symbol_is_skipped_not_errored() {
        // {E} (energy) should not cause parse_entry to fail
        let v = json!({
            "name": "Test",
            "mana_cost": "{E}{G}",
            "type_line": "Creature \u{2014} Test",
            "oracle_text": ""
        });
        let ParsedEntry::Card(card) = parse_entry(&v).unwrap() else {
            panic!("expected Card")
        };
        let cost = card.mana_cost.unwrap();
        // {E} is skipped; only {G} is kept
        assert!(cost.pips.contains(&ManaPip::Green));
    }

    #[test]
    fn parse_entry_routes_token() {
        let v = json!({
            "layout": "token",
            "name": "Llanowar Elves",
            "mana_cost": "",
            "type_line": "Token Creature \u{2014} Elf Druid",
            "oracle_text": ""
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Token(_))));
    }

    #[test]
    fn parse_entry_routes_normal_card() {
        let v = json!({
            "layout": "normal",
            "name": "Grizzly Bears",
            "mana_cost": "{1}{G}",
            "type_line": "Creature \u{2014} Bear",
            "oracle_text": "",
            "power": "2",
            "toughness": "2"
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Card(_))));
    }

    #[test]
    fn parse_entry_missing_layout_defaults_to_card() {
        let v = json!({
            "name": "Hill Giant",
            "mana_cost": "{3}{R}",
            "type_line": "Creature \u{2014} Giant",
            "oracle_text": "",
            "power": "3",
            "toughness": "3"
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Card(_))));
    }

    #[test]
    fn parse_entry_routes_double_faced_token() {
        let v = json!({
            "layout": "double_faced_token",
            "name": "Treasure",
            "mana_cost": "",
            "type_line": "Token Artifact — Treasure",
            "oracle_text": "{T}, Sacrifice this artifact: Add one mana of any color."
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Token(_))));
    }

    #[test]
    fn parse_entry_routes_emblem() {
        let v = json!({
            "layout": "emblem",
            "name": "Emblem Garruk",
            "mana_cost": "",
            "type_line": "Emblem — Garruk",
            "oracle_text": "At the beginning of your end step, you may search your library for a creature card, put it onto the battlefield, then shuffle."
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::Token(_))));
    }

    #[test]
    fn parse_entry_skips_art_series() {
        let v = json!({
            "layout": "art_series",
            "name": "Island Art Series",
            "mana_cost": "",
            "type_line": "Card",
            "oracle_text": ""
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::ArtCard)));
    }

    #[test]
    fn parse_entry_skips_silver_border() {
        let v = json!({
            "border_color": "silver",
            "name": "Look at Me, I'm the DCI",
            "mana_cost": "{3}{W}{W}",
            "type_line": "Sorcery",
            "oracle_text": ""
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::UnCard)));
    }

    #[test]
    fn parse_entry_skips_acorn_stamp() {
        let v = json!({
            "security_stamp": "acorn",
            "name": "Everythingamajig",
            "mana_cost": "{5}",
            "type_line": "Artifact",
            "oracle_text": ""
        });
        assert!(matches!(parse_entry(&v), Ok(ParsedEntry::UnCard)));
    }
}
