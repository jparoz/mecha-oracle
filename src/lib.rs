//! Mecha-Oracle — MTG comprehensive-rules enforcement engine.
//!
//! The crate is structured in four top-level modules:
//!
//! * [`types`] — data structures (cards, permanents, game state, mana, stack, …)
//! * [`engine`] — rules enforcement (casting, stack resolution, combat, SBAs, …)
//! * [`parser`] — oracle-text → `RulesText` conversion driven by `oracle.rs`
//! * [`cards`] — `CardDatabase` backed by Scryfall bulk data

pub mod cards;
pub mod engine;
pub mod parser;
pub mod types;
