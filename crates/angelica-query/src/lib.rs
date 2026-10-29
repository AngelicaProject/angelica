//! Lens: a query language over the game map's object model, the plans
//! that check a query against the model and a game build, and the schema
//! of that build. The host runs plans on the game's memory for plugin API
//! 1's Lens; a plugin runs them through handles with the Lens library.

pub mod error;
pub mod mutations;
pub mod plan;
pub mod query;
pub mod schema;
