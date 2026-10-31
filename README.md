<p align="center">
  <img src="./icon.svg" alt="ORMIFY logo" width="128" height="128">
</p>

<h1 align="center">ORMIFY_SQLITE</h1>

<p align="center">
  <a href="https://crates.io/crates/ormify_sqlite"><img src="https://img.shields.io/crates/v/ormify_sqlite?style=flat&logo=rust" alt="crates.io"></a>
  <a href="https://docs.rs/ormify_sqlite"><img src="https://img.shields.io/docsrs/ormify_sqlite?style=flat&logo=docs.rs" alt="docs.rs"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat&logo=github" alt="license"></a>
  <a href="https://github.com/Fasterino/ormify_sqlite">
    <img src="https://img.shields.io/github/stars/Fasterino/ormify_sqlite?style=social" alt="Star on GitHub">
  </a>
</p>

SQLite dialect and drivers for [`ormify`](https://github.com/Fasterino/ormify), a backend-agnostic Rust ORM.

## Features

- SQLite SQL generation with quoted identifiers and `?n` parameter placeholders.
- Naming entities using `group`\_`table_name` are selected through `#[db(group = "...")]`.
- Entity initialization creates schemas, tables, constraints, and indexes.
- Foreign keys default to `ON DELETE CASCADE ON UPDATE CASCADE`.
- Tokio and async-std runtimes.
- Engine-owned recursive joins and transactional writes are provided by `ormify`.

## Installation

Choose one driver/runtime combination:

```toml
[dependencies]
ormify = { version = "1.0.2", features = ["full"] }
ormify_sqlite = { version = "1.0.0", features = ["tokio_file_engine"] }
# Or use bundled version of SQLite:
# ormify_sqlite = { version = "1.0.0", features = ["tokio_file_engine", "tokio_bundled"] }
```

Or select async-std runtime if you don't like Tokio:

```toml
ormify = { version = "1.0.2", features = ["full"] }
ormify_sqlite = { version = "1.0.0", features = ["async_std_file_engine"] }
# Or use bundled version of SQLite:
# ormify_sqlite = { version = "1.0.0", features = ["async_std_file_engine", "async_std_bundled"] }
```

### Feature flags

| Feature                 | Meaning                                                                                                                                                          |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tokio_file_engine`     | Enables `FileEngine` using Tokio-Rusqlite runtime.                                                                                                               |
| `async_std_file_engine` | Enables `FileEngine` using (should be async-std, but in the end sync) Rusqlite runtime.                                                                          |
| `tokio_bundled`         | (`tokio_file_engine` compatible) Using a bundled version of SQLite. This is a good option for cases where linking to SQLite is complicated, such as Windows.     |
| `async_std_bundled`     | (`async_std_file_engine` compatible) Using a bundled version of SQLite. This is a good option for cases where linking to SQLite is complicated, such as Windows. |

The crate has no default driver feature; enable the one you need. Avoid enabling multiple SQLx runtime features in the same build.

## Quick start

```rust
use ormify::{Entity, Observer, Ormify, Transform as _, UnixTimeStamp, Uuid, VarChar};
use ormify_sqlite::{SQLite, FileEngine};

#[derive(Entity, Debug)]
#[db(group = "app")]
struct User {
    #[db(pk)]
    id: Uuid,
    #[db(unique)]
    nickname: VarChar<32>,
    created_at: UnixTimeStamp,
}

#[tokio::main]
async fn main() -> ormify::Result<()> {
    let mut db = Ormify::<SQLite, FileEngine>::connect("./path/to/database")
    .await?;

    db.execute(vec![User::init()]).await?;

    let user = User::builder().id(Uuid::new()).nickname("ada").build();
    db.execute(vec![user.insert()]).await?;

    let res = db.query(User::select()).await?;
    let users = Vec::<Observer<User>>::transform(res)?;
    println!("{users:?}");
    Ok(())
}
```

`FileEngine` creates a single `rusqlite` client connection. It does not create or own a runtime.

## Schema initialization and foreign keys

A scheme group is just a prefix. Initialization creates the qualified table, followed by separate index statements. Foreign-key constraints reference the target entity's qualified schema/table and currently use cascading update/delete behavior.

Submit related `Entity::init()` executions together; `Ormify` resolves the included foreign-key dependencies and initializes referenced entities first. For cycles, use a migration workflow that creates tables before adding foreign keys.

Initialization is DDL execution, not a versioned migration system. It does not alter existing tables to match changed entity definitions.

## SQLite mappings and limits

Core mappings include booleans, signed integers, floats, UUID, timestamps, strings, and byte arrays. SQLite types not recognized by the selected driver return a database error. `Option<T>` maps to nullable database values; `NULL` is bound without assuming a concrete SQLite type.

The SQL language uses bound parameters for condition and write values. `Condition::Raw` is an escape hatch and should only contain trusted SQL.

## License

[MIT.](./LICENSE)
