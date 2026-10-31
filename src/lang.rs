use ormify::{
    ConstraintType, Language, LanguageQuery, SchemedExecution, SchemedQuery, Type, Value,
};

/// SQLite dialect for the core ORM language interface.
///
/// Uses `?n` placeholders, quoted identifiers, SQLite schema-qualified
/// tables, and native SQLite column types. Indexes are emitted as separate
/// `CREATE INDEX` commands. Foreign-key DDL uses `ON DELETE CASCADE` and
/// `ON UPDATE CASCADE`.
#[derive(Default)]
pub struct SQLite;

impl Language for SQLite {
    fn parse_single_query(&self, query: SchemedQuery) -> ormify::Result<LanguageQuery> {
        let mut args = Vec::new();
        let mut sql = format!(
            "SELECT * FROM {}",
            qualified_table(query.scheme.group.as_deref(), &query.scheme.name)
        );

        if let Some(condition) = query.condition {
            let mut args_with_offset = ArgsWithOffset::new(&mut args);
            sql.push_str(" WHERE ");
            sql.push_str(&compile_condition(condition, &mut args_with_offset));
        }

        if !query.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&query.order_by.to_sql());
        }

        if query.limit.value() != 0 {
            sql.push_str(&format!(" LIMIT {}", query.limit.value()));
        }
        if query.offset.value() != 0 {
            sql.push_str(&format!(" OFFSET {}", query.offset.value()));
        }

        Ok(LanguageQuery::new(sql, args))
    }

    fn parse_execution(&self, execution: SchemedExecution) -> ormify::Result<Vec<LanguageQuery>> {
        match execution {
            SchemedExecution::Init {
                entity_id,
                scheme_registry,
            } => compile_init(entity_id, scheme_registry),
            SchemedExecution::Insert { scheme, entry } => {
                let mut fields: Vec<_> = entry.into_iter().collect();
                fields.sort_by(|(left, _), (right, _)| left.cmp(right));
                let (keys, args): (Vec<_>, Vec<_>) = fields.into_iter().unzip();
                let columns = keys.iter().map(|key| quote_ident(key)).collect::<Vec<_>>();
                let placeholders = (1..=keys.len())
                    .map(|index| format!("?{index}"))
                    .collect::<Vec<_>>();

                let sql = format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    qualified_table(scheme.group.as_deref(), &scheme.name),
                    columns.join(", "),
                    placeholders.join(", ")
                );

                Ok(vec![LanguageQuery::new(sql, args)])
            }
            SchemedExecution::Update {
                scheme,
                entry,
                condition,
            } => {
                let mut fields: Vec<_> = entry.into_iter().collect();
                if fields.is_empty() {
                    return Ok(Vec::new());
                }

                fields.sort_by(|(left, _), (right, _)| left.cmp(right));
                let (keys, mut args): (Vec<_>, Vec<_>) = fields.into_iter().unzip();
                let assignments = keys
                    .iter()
                    .enumerate()
                    .map(|(index, key)| format!("{} = ?{}", quote_ident(key), index + 1))
                    .collect::<Vec<_>>();

                let mut args_with_offset = ArgsWithOffset::new(&mut args);
                let predicate = compile_condition(condition, &mut args_with_offset);
                let sql = format!(
                    "UPDATE {} SET {} WHERE {}",
                    qualified_table(scheme.group.as_deref(), &scheme.name),
                    assignments.join(", "),
                    predicate
                );

                Ok(vec![LanguageQuery::new(sql, args)])
            }
            SchemedExecution::Delete { scheme, condition } => {
                let mut args = Vec::new();
                let mut args_with_offset = ArgsWithOffset::new(&mut args);
                let predicate = compile_condition(condition, &mut args_with_offset);
                let sql = format!(
                    "DELETE FROM {} WHERE {}",
                    qualified_table(scheme.group.as_deref(), &scheme.name),
                    predicate
                );

                Ok(vec![LanguageQuery::new(sql, args)])
            }
        }
    }
}

fn compile_init(
    entity_id: ormify::EntityId,
    scheme_registry: &ormify::SchemeRegistry,
) -> ormify::Result<Vec<LanguageQuery>> {
    let scheme = scheme_registry.get(entity_id)?;
    let fields = scheme
        .fields
        .iter()
        .map(|field| {
            let nullable = if field.nullable { "" } else { " NOT NULL" };
            format!(
                "{} {}{}",
                quote_ident(&field.name),
                sqlite_type(&field.r#type),
                nullable
            )
        })
        .collect::<Vec<_>>();
    let mut constraints = Vec::new();
    for constraint in &scheme.constraints {
        match &constraint.inner {
            ConstraintType::PrimaryKey if !constraint.keys.is_empty() => {
                constraints.push(format!(
                    "CONSTRAINT {} PRIMARY KEY ({})",
                    quote_ident(&constraint.name),
                    quote_ident_list(&constraint.keys)
                ));
            }
            ConstraintType::Unique if !constraint.keys.is_empty() => {
                constraints.push(format!(
                    "CONSTRAINT {} UNIQUE ({})",
                    quote_ident(&constraint.name),
                    quote_ident_list(&constraint.keys)
                ));
            }
            ConstraintType::ForeignKey { entity_id, keys } => {
                if constraint.keys.is_empty() || keys.is_empty() {
                    return Err(ormify::Error::make_invalid_schema_error(format!(
                        "foreign key {} must define local and referenced columns",
                        constraint.name
                    )));
                }
                if constraint.keys.len() != keys.len() {
                    return Err(ormify::Error::make_invalid_schema_error(format!(
                        "foreign key {} has mismatched local/referenced column counts",
                        constraint.name
                    )));
                }
                let referenced = scheme_registry.get(*entity_id)?;
                let local = quote_ident_list(&constraint.keys);
                let remote = quote_ident_list(keys);
                constraints.push(format!(
                    "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE CASCADE ON UPDATE CASCADE",
                    quote_ident(&constraint.name),
                    local,
                    qualified_table(referenced.group.as_deref(), &referenced.name),
                    remote
                ));
            }
            ConstraintType::Index => {}
            _ => {}
        }
    }
    let mut definitions = fields;
    definitions.extend(constraints);

    let mut queries = vec![LanguageQuery::new(
        format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            qualified_table(scheme.group.as_deref(), &scheme.name),
            definitions.join(", ")
        ),
        Vec::new(),
    )];

    for constraint in &scheme.constraints {
        if let ConstraintType::Index = &constraint.inner {
            if !constraint.keys.is_empty() {
                queries.push(LanguageQuery::new(
                    format!(
                        "CREATE INDEX IF NOT EXISTS {} ON {} ({})",
                        quote_ident(&constraint.name),
                        qualified_table(scheme.group.as_deref(), &scheme.name),
                        quote_ident_list(&constraint.keys),
                    ),
                    Vec::new(),
                ));
            }
        }
    }

    Ok(queries)
}

struct ArgsWithOffset<'a>(&'a mut Vec<Value>);

impl<'a> ArgsWithOffset<'a> {
    pub fn new(args: &'a mut Vec<Value>) -> Self {
        Self(args)
    }

    pub fn push_arg(&mut self, arg: Value) -> usize {
        self.0.push(arg);
        self.0.len()
    }
}

fn compile_condition(condition: ormify::Condition, args: &mut ArgsWithOffset) -> String {
    match condition {
        ormify::Condition::Raw(sql) => sql,
        ormify::Condition::Not(inner) => {
            format!("NOT ({})", compile_condition(*inner, args))
        }
        ormify::Condition::And(left, right) => format!(
            "({}) AND ({})",
            compile_condition(*left, args),
            compile_condition(*right, args)
        ),
        ormify::Condition::Or(left, right) => format!(
            "({}) OR ({})",
            compile_condition(*left, args),
            compile_condition(*right, args)
        ),
        ormify::Condition::Between(field, from, to) => {
            let first = args.push_arg(from);
            let second = args.push_arg(to);
            format!("{} BETWEEN ?{first} AND ?{second}", quote_ident(&field))
        }
        ormify::Condition::In(field, values) => {
            let placeholders: Vec<_> = values
                .into_iter()
                .map(|value| format!("?{}", args.push_arg(value)))
                .collect();
            if placeholders.is_empty() {
                "FALSE".to_owned()
            } else {
                format!("{} IN ({})", quote_ident(&field), placeholders.join(", "))
            }
        }
        ormify::Condition::Like(field, pattern) => {
            comparison("LIKE", &field, Value::VarChar(pattern), args)
        }
        ormify::Condition::Eq(field, value) => comparison("=", &field, value, args),
        ormify::Condition::Neq(field, value) => comparison("<>", &field, value, args),
        ormify::Condition::Lt(field, value) => comparison("<", &field, value, args),
        ormify::Condition::Lte(field, value) => comparison("<=", &field, value, args),
        ormify::Condition::Gt(field, value) => comparison(">", &field, value, args),
        ormify::Condition::Gte(field, value) => comparison(">=", &field, value, args),
    }
}

fn comparison(operator: &str, field: &str, value: Value, args: &mut ArgsWithOffset) -> String {
    if matches!(value, Value::Null) {
        return match operator {
            "=" => format!("{} IS NULL", quote_ident(field)),
            "<>" => format!("{} IS NOT NULL", quote_ident(field)),
            _ => "FALSE".to_owned(),
        };
    }
    let index = args.push_arg(value);
    format!("{} {} ?{index}", quote_ident(field), operator)
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

// Unlike postgres, there are no schemas in sqlite, so an underscore is used.
fn qualified_table(group: Option<&str>, name: &str) -> String {
    match group {
        Some(group) => quote_ident(&format!("{}_{}", group, name)),
        None => quote_ident(name),
    }
}

fn quote_ident_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| quote_ident(value))
        .collect::<Vec<_>>()
        .join(", ")
}

// These names intentionally match the declaration types consumed by
// FileEngine::query via rusqlite's `column_decltype` feature.
fn sqlite_type(value: &Type) -> String {
    match value {
        Type::Bool => "BOOL".to_owned(),
        Type::I16 => "INT2".to_owned(),
        Type::I32 => "INT4".to_owned(),
        Type::I64 => "INT8".to_owned(),
        Type::F32 => "FLOAT4".to_owned(),
        Type::F64 => "FLOAT8".to_owned(),
        Type::Uuid => "UUID".to_owned(),
        Type::UnixTimeStamp => "TIMESTAMP".to_owned(),
        Type::VarChar(_) => "VARCHAR".to_owned(),
        Type::Blob => "BLOB".to_owned(),
    }
}
