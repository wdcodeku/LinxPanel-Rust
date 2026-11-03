//! Editing the data of registered SQLite files: table structure, paged and
//! sorted rows, row updates / inserts / deletes, and an SQL console.
//!
//! Values cross the API as JSON with SQLite's own storage classes: null,
//! integers (as strings beyond what JavaScript numbers hold exactly), reals,
//! text, and blobs (shown, never edited). Rows are addressed by rowid, or by
//! the primary key for WITHOUT ROWID tables; views are read-only.

use std::path::Path;
use std::time::{Duration, Instant};

use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::sqlite::{SqliteArguments, SqliteRow};
use sqlx::{Arguments, Column, Connection, Either, Row, SqliteConnection, SqlitePool, TypeInfo, ValueRef};

use super::sqlitedb::{open_read_only, options, quote_ident, registered_path};

/// Cells longer than this are cut in listings (and cannot be edited there).
const CELL_LIMIT: usize = 64 * 1024;
/// Integers beyond this go to the browser as strings.
const JS_SAFE: i64 = 9_007_199_254_740_991;
const MAX_PAGE: u32 = 500;
const QUERY_ROW_LIMIT: usize = 1000;
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub decl_type: String,
    pub notnull: bool,
    pub default: Option<String>,
    /// Position in the primary key (0 = not part of it).
    pub pk: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexInfo {
    pub name: String,
    pub unique: bool,
    pub sql: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableSchema {
    pub name: String,
    /// table | view
    pub kind: String,
    pub sql: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
    pub without_rowid: bool,
    pub editable: bool,
}

async fn connect(path: &Path, read_only: bool) -> Result<SqliteConnection, String> {
    if read_only {
        open_read_only(path).await
    } else {
        sqlx::ConnectOptions::connect(&options(path, false))
            .await
            .map_err(|e| format!("Cannot open {}: {}", path.display(), e))
    }
}

async fn load_schema(conn: &mut SqliteConnection, table: &str) -> Result<TableSchema, String> {
    let (kind, sql): (String, Option<String>) =
        sqlx::query_as("SELECT type, sql FROM sqlite_master WHERE name = ? AND type IN ('table', 'view')")
            .bind(table)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("No table named {}", table))?;
    let sql = sql.unwrap_or_default();
    let columns: Vec<ColumnInfo> = sqlx::query(&format!("PRAGMA table_info({})", quote_ident(table)))
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|row| ColumnInfo {
            name: row.get("name"),
            decl_type: row.get::<Option<String>, _>("type").unwrap_or_default(),
            notnull: row.get::<i64, _>("notnull") != 0,
            default: row.get("dflt_value"),
            pk: row.get("pk"),
        })
        .collect();
    let mut indexes = Vec::new();
    if kind == "table" {
        let rows = sqlx::query(&format!("PRAGMA index_list({})", quote_ident(table)))
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        for row in rows {
            let name: String = row.get("name");
            let index_sql: Option<String> = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
                .bind(&name)
                .fetch_optional(&mut *conn)
                .await
                .map_err(|e| e.to_string())?
                .flatten();
            indexes.push(IndexInfo {
                name,
                unique: row.get::<i64, _>("unique") != 0,
                sql: index_sql,
            });
        }
    }
    let without_rowid = sql.to_ascii_uppercase().contains("WITHOUT ROWID");
    let editable = kind == "table" && (!without_rowid || columns.iter().any(|c| c.pk > 0));
    Ok(TableSchema {
        name: table.to_string(),
        kind,
        sql,
        columns,
        indexes,
        without_rowid,
        editable,
    })
}

pub async fn schema(pool: &SqlitePool, data_dir: &Path, file: &str, table: &str) -> Result<TableSchema, String> {
    let path = registered_path(pool, file, data_dir).await?;
    let mut conn = connect(&path, true).await?;
    let result = load_schema(&mut conn, table).await;
    let _ = conn.close().await;
    result
}

// ===== Values =====

fn cell(row: &SqliteRow, index: usize) -> Value {
    let Ok(raw) = row.try_get_raw(index) else {
        return Value::Null;
    };
    if raw.is_null() {
        return Value::Null;
    }
    // The storage class of this value (SQLite types values, not columns).
    let class = raw.type_info().name().to_string();
    match class.as_str() {
        "INTEGER" => match row.try_get::<i64, _>(index) {
            Ok(v) if (-JS_SAFE..=JS_SAFE).contains(&v) => json!(v),
            Ok(v) => json!({ "$int": v.to_string() }),
            Err(_) => Value::Null,
        },
        "REAL" => row
            .try_get::<f64, _>(index)
            .ok()
            .and_then(|v| serde_json::Number::from_f64(v).map(Value::Number))
            .unwrap_or(Value::Null),
        "BLOB" => {
            let bytes: Vec<u8> = row.try_get(index).unwrap_or_default();
            let preview: String = bytes.iter().take(32).map(|b| format!("{:02x}", b)).collect();
            json!({ "$blob": bytes.len(), "hex": preview })
        }
        _ => {
            let text: String = row.try_get(index).unwrap_or_default();
            if text.len() > CELL_LIMIT {
                let mut cut = CELL_LIMIT;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                json!({ "$text": &text[..cut], "size": text.len() })
            } else {
                Value::String(text)
            }
        }
    }
}

/// Binds a JSON value the way SQLite would store what was typed.
fn bind(args: &mut SqliteArguments<'_>, value: &Value) -> Result<(), String> {
    let result = match value {
        Value::Null => args.add(None::<String>),
        Value::Bool(b) => args.add(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => args.add(i),
            None => args.add(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => args.add(s.clone()),
        Value::Object(map) => match map.get("$int").and_then(Value::as_str) {
            Some(text) => args.add(text.parse::<i64>().map_err(|_| "Invalid integer".to_string())?),
            None => return Err("Blobs and cut-off values cannot be edited here; use SQL".into()),
        },
        Value::Array(_) => return Err("Unsupported value".into()),
    };
    result.map_err(|e| e.to_string())
}

// ===== Rows =====

#[derive(Debug, Deserialize)]
pub struct RowsQuery {
    pub path: String,
    pub table: String,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Column to sort by.
    #[serde(default)]
    pub order: String,
    #[serde(default)]
    pub desc: bool,
    /// An SQL condition (`age > 30 AND name LIKE 'a%'`).
    #[serde(default)]
    pub filter: String,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Serialize)]
pub struct RowsPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// Per row: what identifies it for updates and deletes.
    pub keys: Vec<Value>,
    pub total: u64,
    pub editable: bool,
}

/// `WHERE ...` for one row: rowid, or every primary key column.
fn key_clause(schema: &TableSchema, key: &Value, args: &mut SqliteArguments<'_>) -> Result<String, String> {
    if !schema.without_rowid {
        let rowid = key
            .get("rowid")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .ok_or("Missing row id")?;
        args.add(rowid).map_err(|e| e.to_string())?;
        return Ok("rowid = ?".into());
    }
    let pk = key.get("pk").and_then(Value::as_object).ok_or("Missing primary key")?;
    let mut parts = Vec::new();
    for column in schema.columns.iter().filter(|c| c.pk > 0) {
        let value = pk.get(&column.name).ok_or("Incomplete primary key")?;
        bind(args, value)?;
        parts.push(format!("{} IS ?", quote_ident(&column.name)));
    }
    Ok(parts.join(" AND "))
}

pub async fn rows(pool: &SqlitePool, data_dir: &Path, q: &RowsQuery) -> Result<RowsPage, String> {
    let path = registered_path(pool, &q.path, data_dir).await?;
    // Reads only: a filter cannot change anything.
    let mut conn = connect(&path, true).await?;
    let result = async {
        let schema = load_schema(&mut conn, &q.table).await?;
        let table = quote_ident(&q.table);
        let filter = q.filter.trim();
        let where_sql = if filter.is_empty() { String::new() } else { format!(" WHERE ({})", filter) };
        let order_sql = if q.order.is_empty() {
            String::new()
        } else if schema.columns.iter().any(|c| c.name == q.order) {
            format!(" ORDER BY {} {}", quote_ident(&q.order), if q.desc { "DESC" } else { "ASC" })
        } else {
            return Err(format!("No column named {}", q.order));
        };
        let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}{}", table, where_sql))
            .fetch_one(&mut conn)
            .await
            .map_err(|e| format!("Filter: {}", e))?;
        let use_rowid = schema.kind == "table" && !schema.without_rowid;
        let select = if use_rowid {
            format!("SELECT rowid AS \"__lp_rowid\", * FROM {}", table)
        } else {
            format!("SELECT * FROM {}", table)
        };
        let sql = format!(
            "{}{}{} LIMIT {} OFFSET {}",
            select,
            where_sql,
            order_sql,
            q.limit.clamp(1, MAX_PAGE),
            q.offset
        );
        let fetched = sqlx::query(&sql).fetch_all(&mut conn).await.map_err(|e| e.to_string())?;
        let skip = usize::from(use_rowid);
        let columns: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
        let mut rows = Vec::with_capacity(fetched.len());
        let mut keys = Vec::with_capacity(fetched.len());
        for row in &fetched {
            let values: Vec<Value> = (skip..row.columns().len()).map(|i| cell(row, i)).collect();
            let key = if use_rowid {
                json!({ "rowid": cell(row, 0) })
            } else if schema.editable {
                let mut pk = Map::new();
                for (i, column) in schema.columns.iter().enumerate() {
                    if column.pk > 0 {
                        pk.insert(column.name.clone(), values.get(i).cloned().unwrap_or(Value::Null));
                    }
                }
                json!({ "pk": pk })
            } else {
                Value::Null
            };
            rows.push(values);
            keys.push(key);
        }
        Ok(RowsPage {
            columns,
            rows,
            keys,
            total: total.max(0) as u64,
            editable: schema.editable,
        })
    }
    .await;
    let _ = conn.close().await;
    result
}

#[derive(Debug, Deserialize)]
pub struct RowChange {
    pub path: String,
    pub table: String,
    /// Update / delete: the row (from `RowsPage::keys`).
    #[serde(default)]
    pub key: Value,
    #[serde(default)]
    pub keys: Vec<Value>,
    /// Update / insert: column -> new value.
    #[serde(default)]
    pub values: Map<String, Value>,
}

async fn editable_schema(conn: &mut SqliteConnection, table: &str) -> Result<TableSchema, String> {
    let schema = load_schema(conn, table).await?;
    if !schema.editable {
        return Err("This table cannot be edited here (a view, or no row id); use SQL".into());
    }
    Ok(schema)
}

fn checked_columns<'a>(schema: &TableSchema, values: &'a Map<String, Value>) -> Result<Vec<(&'a String, &'a Value)>, String> {
    values
        .iter()
        .map(|(name, value)| {
            if schema.columns.iter().any(|c| &c.name == name) {
                Ok((name, value))
            } else {
                Err(format!("No column named {}", name))
            }
        })
        .collect()
}

pub async fn update_row(pool: &SqlitePool, data_dir: &Path, change: &RowChange) -> Result<u64, String> {
    let path = registered_path(pool, &change.path, data_dir).await?;
    let mut conn = connect(&path, false).await?;
    let result = async {
        let schema = editable_schema(&mut conn, &change.table).await?;
        let columns = checked_columns(&schema, &change.values)?;
        if columns.is_empty() {
            return Ok(0);
        }
        let mut args = SqliteArguments::default();
        let mut sets = Vec::new();
        for (name, value) in &columns {
            bind(&mut args, value)?;
            sets.push(format!("{} = ?", quote_ident(name)));
        }
        let key = key_clause(&schema, &change.key, &mut args)?;
        let sql = format!("UPDATE {} SET {} WHERE {}", quote_ident(&schema.name), sets.join(", "), key);
        let done = sqlx::query_with(&sql, args).execute(&mut conn).await.map_err(|e| e.to_string())?;
        if done.rows_affected() == 0 {
            return Err("The row no longer exists (changed by the application?); refresh".into());
        }
        Ok(done.rows_affected())
    }
    .await;
    let _ = conn.close().await;
    result
}

pub async fn insert_row(pool: &SqlitePool, data_dir: &Path, change: &RowChange) -> Result<i64, String> {
    let path = registered_path(pool, &change.path, data_dir).await?;
    let mut conn = connect(&path, false).await?;
    let result = async {
        let schema = editable_schema(&mut conn, &change.table).await?;
        let columns = checked_columns(&schema, &change.values)?;
        let table = quote_ident(&schema.name);
        let done = if columns.is_empty() {
            sqlx::query(&format!("INSERT INTO {} DEFAULT VALUES", table))
                .execute(&mut conn)
                .await
        } else {
            let mut args = SqliteArguments::default();
            for (_, value) in &columns {
                bind(&mut args, value)?;
            }
            let names: Vec<String> = columns.iter().map(|(n, _)| quote_ident(n)).collect();
            let marks = vec!["?"; columns.len()].join(", ");
            sqlx::query_with(&format!("INSERT INTO {} ({}) VALUES ({})", table, names.join(", "), marks), args)
                .execute(&mut conn)
                .await
        }
        .map_err(|e| e.to_string())?;
        Ok(done.last_insert_rowid())
    }
    .await;
    let _ = conn.close().await;
    result
}

/// Deletes the rows in one transaction: all or none.
pub async fn delete_rows(pool: &SqlitePool, data_dir: &Path, change: &RowChange) -> Result<u64, String> {
    if change.keys.is_empty() {
        return Err("No rows selected".into());
    }
    let path = registered_path(pool, &change.path, data_dir).await?;
    let mut conn = connect(&path, false).await?;
    let result = async {
        let schema = editable_schema(&mut conn, &change.table).await?;
        let mut tx = conn.begin().await.map_err(|e| e.to_string())?;
        let mut deleted = 0;
        for key in &change.keys {
            let mut args = SqliteArguments::default();
            let clause = key_clause(&schema, key, &mut args)?;
            let sql = format!("DELETE FROM {} WHERE {}", quote_ident(&schema.name), clause);
            deleted += sqlx::query_with(&sql, args)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .rows_affected();
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(deleted)
    }
    .await;
    let _ = conn.close().await;
    result
}

// ===== SQL console =====

#[derive(Debug, Serialize)]
pub struct StatementResult {
    /// The statement (shortened), when the text could be split reliably.
    pub statement: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    /// More rows than were returned.
    pub truncated: bool,
    pub changes: u64,
    /// Whether it returned rows (a SELECT, PRAGMA ...).
    pub returned_rows: bool,
}

#[derive(Debug, Serialize)]
pub struct QueryOutput {
    pub results: Vec<StatementResult>,
    pub millis: u64,
    /// Set when a statement failed; the earlier ones already ran.
    pub error: Option<String>,
}

/// Runs one or more statements as written (BEGIN / COMMIT included).
pub async fn run_sql(pool: &SqlitePool, data_dir: &Path, file: &str, sql: &str) -> Result<QueryOutput, String> {
    if sql.trim().is_empty() {
        return Err("Enter an SQL statement".into());
    }
    let path = registered_path(pool, file, data_dir).await?;
    let mut conn = connect(&path, false).await?;
    let started = Instant::now();
    let mut results: Vec<StatementResult> = Vec::new();
    let mut current = StatementResult {
        statement: String::new(),
        columns: Vec::new(),
        rows: Vec::new(),
        truncated: false,
        changes: 0,
        returned_rows: false,
    };
    let mut error = None;
    {
        let mut stream = sqlx::raw_sql(sql).fetch_many(&mut conn);
        let run = async {
            while let Some(item) = stream.try_next().await? {
                match item {
                    Either::Left(done) => {
                        current.changes = done.rows_affected();
                        results.push(std::mem::replace(
                            &mut current,
                            StatementResult {
                                statement: String::new(),
                                columns: Vec::new(),
                                rows: Vec::new(),
                                truncated: false,
                                changes: 0,
                                returned_rows: false,
                            },
                        ));
                    }
                    Either::Right(row) => {
                        if current.columns.is_empty() {
                            current.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                        }
                        current.returned_rows = true;
                        if current.rows.len() < QUERY_ROW_LIMIT {
                            current.rows.push((0..row.columns().len()).map(|i| cell(&row, i)).collect());
                        } else {
                            current.truncated = true;
                        }
                    }
                }
            }
            Ok::<(), sqlx::Error>(())
        };
        match tokio::time::timeout(QUERY_TIMEOUT, run).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => error = Some(err.to_string()),
            Err(_) => error = Some(format!("Stopped after {} seconds", QUERY_TIMEOUT.as_secs())),
        }
    }
    let _ = conn.close().await;
    // SQLite reports the last write's count for every statement; keep it
    // only where the statement is a write.
    let statements = split_statements(sql);
    let labelled = statements.len() >= results.len();
    for (index, result) in results.iter_mut().enumerate() {
        let text = statements.get(index).map(String::as_str).unwrap_or("");
        let keyword = text.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
        let writes = matches!(keyword.as_str(), "INSERT" | "UPDATE" | "DELETE" | "REPLACE");
        if result.returned_rows || (labelled && !writes) {
            result.changes = 0;
        }
        if labelled {
            let mut shown: String = text.chars().take(200).collect();
            if text.chars().count() > 200 {
                shown.push('…');
            }
            result.statement = shown;
        }
    }
    Ok(QueryOutput {
        results,
        millis: started.elapsed().as_millis() as u64,
        error,
    })
}

/// Splits SQL into statements at top-level semicolons: quotes, comments and
/// trigger bodies (BEGIN ... END) are respected. Best effort, for labels.
fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    let mut depth = 0i32; // inside CREATE TRIGGER ... BEGIN ... END
    let mut word = String::new();
    let flush_word = |word: &mut String, depth: &mut i32, current: &str| {
        let upper = word.to_ascii_uppercase();
        if upper == "BEGIN" && current.to_ascii_uppercase().contains("TRIGGER") {
            *depth += 1;
        } else if upper == "END" && *depth > 0 {
            *depth -= 1;
        }
        word.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' | '`' | '[' => {
                let close = if c == '[' { ']' } else { c };
                flush_word(&mut word, &mut depth, &current);
                current.push(c);
                i += 1;
                while i < chars.len() {
                    current.push(chars[i]);
                    if chars[i] == close {
                        // Doubled quote = escaped quote.
                        if close != ']' && i + 1 < chars.len() && chars[i + 1] == close {
                            current.push(chars[i + 1]);
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                flush_word(&mut word, &mut depth, &current);
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                current.push(' ');
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                flush_word(&mut word, &mut depth, &current);
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 1;
                current.push(' ');
            }
            ';' => {
                flush_word(&mut word, &mut depth, &current);
                if depth > 0 {
                    current.push(c);
                } else {
                    let statement = current.trim().to_string();
                    if !statement.is_empty() {
                        out.push(statement);
                    }
                    current.clear();
                }
            }
            _ => {
                if c.is_alphanumeric() || c == '_' {
                    word.push(c);
                } else {
                    flush_word(&mut word, &mut depth, &current);
                }
                current.push(c);
            }
        }
        i += 1;
    }
    flush_word(&mut word, &mut depth, &current);
    let rest = current.trim().to_string();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_split_at_top_level() {
        let sql = "SELECT 'a;b'; -- c;omment\nINSERT INTO t VALUES (\"x;y\");\n/* x; */ CREATE TRIGGER tr AFTER INSERT ON t BEGIN UPDATE t SET a = 1; DELETE FROM u; END; SELECT 1";
        let parts = split_statements(sql);
        assert_eq!(parts.len(), 4, "{:?}", parts);
        assert!(parts[0].starts_with("SELECT 'a;b'"));
        assert!(parts[1].starts_with("INSERT"));
        assert!(parts[2].starts_with("CREATE TRIGGER") && parts[2].ends_with("END"));
        assert_eq!(parts[3], "SELECT 1");
        assert_eq!(split_statements("  ;; "), Vec::<String>::new());
    }

    #[test]
    fn values_bind_by_kind() {
        let mut args = SqliteArguments::default();
        for value in [json!(null), json!(true), json!(42), json!(1.5), json!("x"), json!({"$int": "9007199254740993"})] {
            bind(&mut args, &value).unwrap();
        }
        assert!(bind(&mut args, &json!({"$blob": 3})).is_err());
        assert!(bind(&mut args, &json!({"$int": "abc"})).is_err());
    }
}
