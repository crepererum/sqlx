use indexmap::map::IndexMap;
use indexmap::set::IndexSet;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub type Result<T, E = FixtureError> = std::result::Result<T, E>;

pub struct FixtureSnapshot {
    tables: BTreeMap<TableName, Table>,
}

#[derive(Debug, thiserror::Error)]
#[error("could not create {0}")]
pub struct FixtureError(String);

pub struct Fixture {
    ops: Vec<FixtureOp>,
}

enum FixtureOp {
    Truncate(TableName),
    Insert {
        table: TableName,
        columns: Vec<ColumnName>,
        rows: Vec<Value>,
    },
    Update {
        table: TableName,
        set: IndexMap<ColumnName, Value>,
        cond: IndexMap<ColumnName, Value>,
    },
    Delete {
        table: TableName,
        cond: IndexMap<ColumnName, Value>,
    },
}

type TableName = Arc<str>;
type ColumnName = Arc<str>;
type Value = String;

struct Table {
    columns: IndexSet<ColumnName>,
    rows: Vec<Value>,
    primary_key: Option<ColumnName>,
    foreign_keys: HashMap<ColumnName, (TableName, ColumnName)>,
}

macro_rules! fixture_assert (
    ($cond:expr, $msg:literal $($arg:tt)*) => {
        if !($cond) {
            return Err(FixtureError(format!($msg $($arg)*)))
        }
    }
);

impl Table {
    fn assert_structure(&self, other: &Table) -> Result<()> {
        fixture_assert!(
            self.columns == other.columns,
            "mismatch in columns: {:?} vs {:?}",
            self.columns,
            other.columns
        );
        fixture_assert!(
            self.primary_key == other.primary_key,
            "mismatch in primary keys: {:?} vs {:?}",
            self.primary_key,
            other.primary_key
        );
        fixture_assert!(
            self.foreign_keys == other.foreign_keys,
            "mismatch in foreign keys: {:?} vs {:?}",
            self.foreign_keys,
            other.foreign_keys
        );
    }
}

impl FixtureSnapshot {
    pub fn fixture(&self, previous: &FixtureSnapshot) -> Result<Fixture> {
        fixture_assert!(
            self.tables.keys().eq(previous.tables.keys()),
            "mismatch in tables: {:?} vs {:?}",
            self.tables.keys(),
            previous.tables.keys()
        );
    }
}
