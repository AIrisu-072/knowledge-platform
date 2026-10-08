//! Audit Infrastructure v1 acceptance (unit C, design §14.4).
//!
//! This crate has no production code. The acceptance tests live in
//! `tests/acceptance` and drive the real Document producers, the relay and
//! the Audit Store against disposable PostgreSQL 18.6 containers with
//! synthetic data only (see `README.md`).
#![forbid(unsafe_code)]
