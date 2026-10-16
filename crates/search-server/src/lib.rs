#![forbid(unsafe_code)]
//! Search API server composition with the PoC identity, never a business API.
//! It serves the Search, Discover, Resource and Sources routes of one Document
//! Source as its own process, next to the Document server.
pub mod composition;
pub mod config;
pub mod identity;
