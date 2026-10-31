use std::path::PathBuf;

use ormify::{
    DefaultLanguageEnum, Driver, Entry, Error, ExecutionResponse, FieldType, LanguageQuery,
    Supported, UnixTimeStamp, Uuid, Value, VarChar,
};
use rusqlite::{
    Row, ToSql,
    types::{FromSql, Null},
};

use crate::SQLite;

/// File SQLite driver.
///
/// Select `tokio_file_engine` or `async_std_file_engine` to choose the
/// runtime integration. The driver uses a SQLite file driver and executes ORM
/// writes in a transaction.
pub struct FileEngine {
    #[cfg(not(feature = "tokio_file_engine"))]
    conn: rusqlite::Connection,
    #[cfg(feature = "tokio_file_engine")]
    conn: tokio_rusqlite::Connection,
}

impl Supported<SQLite> for FileEngine {
    fn get_language() -> Self::LanguageEnum {
        Default::default()
    }
}

fn bind(value: Value) -> Box<dyn ToSql> {
    match value {
        Value::Null => Box::new(Null),
        Value::Bool(value) => Box::new(value),
        Value::I16(value) => Box::new(value),
        Value::I32(value) => Box::new(value),
        Value::I64(value) => Box::new(value),
        Value::F32(value) => Box::new(value),
        Value::F64(value) => Box::new(value),
        Value::Uuid(value) => Box::new(*value),
        Value::UnixTimeStamp(value) => Box::new(*value),
        Value::VarChar(value) => Box::new(value),
        Value::Blob(value) => Box::new(value),
    }
}

fn params(values: Vec<Value>) -> Vec<Box<dyn ToSql>> {
    values.into_iter().map(bind).collect()
}

fn try_get<T: FromSql>(row: &Row, index: usize) -> ormify::Result<T> {
    row.get(index)
        .map_err(|e| Error::Database(format!("Rusqlite decode failed: {e}")))
}

impl Driver for FileEngine {
    type LanguageEnum = DefaultLanguageEnum;

    type ConnectArgs = PathBuf;

    fn connect(
        args: Self::ConnectArgs,
        _language: Self::LanguageEnum,
    ) -> impl std::future::Future<Output = ormify::Result<Self>> {
        async move {
            #[cfg(not(feature = "tokio_file_engine"))]
            let conn_res = rusqlite::Connection::open(args);
            #[cfg(feature = "tokio_file_engine")]
            let conn_res = tokio_rusqlite::Connection::open(args).await;

            let conn = conn_res
                .map_err(|e| Error::Database(format!("Rusqlite connection failed: {e}")))?;
            Ok(Self { conn })
        }
    }

    fn query(
        &mut self,
        query: LanguageQuery,
    ) -> impl std::future::Future<Output = ormify::Result<Vec<Entry>>> {
        self.call(move |conn| {
            let mut statement = conn
                .prepare(&query.query)
                .map_err(|e| Error::Database(format!("Rusqlite prepare failed: {e}")))?;

            let values = params(query.args);
            let references: Vec<&dyn ToSql> = values
                .iter()
                .map(|value| value.as_ref() as &dyn ToSql)
                .collect();

            let mut rows = statement
                .query(&references as &[&dyn ToSql])
                .map_err(|e| Error::Database(format!("Rusqlite query failed: {e}")))?;

            let mut entries = Vec::new();

            while let Some(row) = rows
                .next()
                .map_err(|e| Error::Database(format!("Rusqlite query failed: {e}")))?
            {
                let mut entry = Entry::new();
                for (index, column) in row.as_ref().columns().iter().enumerate() {
                    let value = match column.decl_type() {
                        Some("BOOL") => {
                            <Option<bool> as FieldType>::to_value(try_get(&row, index)?)
                        }
                        Some("INT2") => <Option<i16> as FieldType>::to_value(try_get(&row, index)?),
                        Some("INT4") => <Option<i32> as FieldType>::to_value(try_get(&row, index)?),
                        Some("INT8") => <Option<i64> as FieldType>::to_value(try_get(&row, index)?),
                        Some("FLOAT4") => {
                            <Option<f32> as FieldType>::to_value(try_get(&row, index)?)
                        }
                        Some("FLOAT8") => {
                            <Option<f64> as FieldType>::to_value(try_get(&row, index)?)
                        }
                        Some("UUID") => {
                            let value: Option<_> = try_get(&row, index)?;
                            FieldType::to_value(value.map(Uuid))
                        }
                        Some("TIMESTAMP") => {
                            let value: Option<_> = try_get(&row, index)?;
                            FieldType::to_value(value.map(UnixTimeStamp))
                        }
                        Some("VARCHAR") => {
                            let value: Option<_> = try_get(&row, index)?;
                            FieldType::to_value(value.map(VarChar::new_unsized))
                        }
                        Some("BLOB") => {
                            <Option<Vec<u8>> as FieldType>::to_value(try_get(&row, index)?)
                        }
                        Some(type_name) => {
                            return Err(Error::Database(format!(
                                "Unsupported Rusqlite type: {}",
                                type_name
                            )));
                        }
                        None => {
                            return Err(Error::Database("Get Rusqlite decltype failed".to_owned()));
                        }
                    };
                    entry.insert(column.name().to_owned(), value);
                }
                entries.push(entry);
            }

            Ok(entries)
        })
    }

    fn execute(
        &mut self,
        commands: Vec<LanguageQuery>,
    ) -> impl std::future::Future<Output = ormify::Result<ExecutionResponse>> {
        self.call(move |conn| {
            let transaction = conn
                .transaction()
                .map_err(|e| Error::Database(format!("Rusqlite transaction begin failed: {e}")))?;
            let mut affected = 0;

            for query in commands {
                let mut statement = transaction
                    .prepare(&query.query)
                    .map_err(|e| Error::Database(format!("Rusqlite prepare failed: {e}")))?;

                let values = params(query.args);
                let references: Vec<&dyn ToSql> = values
                    .iter()
                    .map(|value| value.as_ref() as &dyn ToSql)
                    .collect();

                affected += statement
                    .execute(&references as &[&dyn ToSql])
                    .map_err(|e| Error::Database(format!("Rusqlite execute failed: {e}")))?;
            }

            transaction
                .commit()
                .map_err(|e| Error::Database(format!("Rusqlite transaction commit failed: {e}")))?;

            Ok(affected as u64)
        })
    }
}

impl FileEngine {
    async fn call<T: Send + 'static>(
        &mut self,
        f: impl FnOnce(&mut rusqlite::Connection) -> ormify::Result<T> + Send + 'static,
    ) -> ormify::Result<T> {
        #[cfg(not(feature = "tokio_file_engine"))]
        let res = f(&mut self.conn);
        #[cfg(feature = "tokio_file_engine")]
        let res = self.conn.call(|conn| f(conn)).await.map_err(|e| match e {
            tokio_rusqlite::Error::ConnectionClosed => {
                Error::Database("Rusqlite connection closed".to_owned())
            }
            tokio_rusqlite::Error::Error(e) => e,
            _ => Error::Database("Rusqlite unexpected error".to_owned()),
        });

        res
    }
}
