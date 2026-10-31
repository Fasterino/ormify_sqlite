//! # ormify_sqlite
//!
//! SQLite language and driver implementations for [`ormify`](https://docs.rs/ormify).
//!
//! [`SQLite`] compiles ORM schemes, queries, and executions into
//! SQLite SQL with quoted identifiers and `$n` parameters. A scheme's
//! `group` is used as the SQLite schema name.
//!
//! Enable one driver feature for database access:
//!
//! - `tokio_file_engine` exposes [`FileEngine`] with Tokio-Rusqlite runtime.
//! - `async_std_file_engine` exposes [`FileEngine`] with (should be async-std,
//! but in the end sync) Rusqlite runtime.
//!
//! Enable bundled driver feature for cases where linking to SQLite is complicated, such as Windows.
//!
//! - `tokio_bundled` - compatible with `tokio_file_engine`.
//! - `async_std_bundled` - compatible with `async_std_file_engine`.
//! Initialization emits schema/table/constraint/index commands. Foreign keys
//! use cascading update and delete behavior. This crate does not provide a
//! versioned migration system; changed schemas should be managed by an
//! application migration workflow.

#[cfg(feature = "_file_engine")]
mod file_engine;
mod lang;

#[cfg(feature = "_file_engine")]
pub use file_engine::*;
pub use lang::*;

#[cfg(test)]
mod tests;
