use std::path::Path;

mod auto_vacuum;
mod connect;
mod journal_mode;
mod locking_mode;
mod parse;
mod synchronous;

use crate::connection::LogSettings;
pub use auto_vacuum::SqliteAutoVacuum;
pub use journal_mode::SqliteJournalMode;
pub use locking_mode::SqliteLockingMode;
use std::cmp::Ordering;
use std::sync::Arc;
use std::{borrow::Cow, time::Duration};
pub use synchronous::SqliteSynchronous;

use crate::common::DebugFn;
use crate::sqlite::connection::collation::Collation;
use indexmap::IndexMap;

/// Options and flags which can be used to configure a SQLite connection.
///
/// A value of `SqliteConnectOptions` can be parsed from a connection URL,
/// as described by [SQLite](https://www.sqlite.org/uri.html).
///
/// | URL | Description |
/// | -- | -- |
/// `sqlite::memory:` | Open an in-memory database. |
/// `sqlite:data.db` | Open the file `data.db` in the current directory. |
/// `sqlite://data.db` | Open the file `data.db` in the current directory. |
/// `sqlite:///data.db` | Open the file `data.db` from the root (`/`) directory. |
/// `sqlite://data.db?mode=ro` | Open the file `data.db` for read-only access. |
///
/// # Example
///
/// ```rust,no_run
/// # use sqlx_core::connection::ConnectOptions;
/// # use sqlx_core::error::Error;
/// use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
/// use std::str::FromStr;
///
/// # fn main() {
/// # #[cfg(feature = "_rt-async-std")]
/// # sqlx_rt::async_std::task::block_on::<_, Result<(), Error>>(async move {
/// let conn = SqliteConnectOptions::from_str("sqlite://data.db")?
///     .journal_mode(SqliteJournalMode::Wal)
///     .read_only(true)
///     .connect().await?;
/// # Ok(())
/// # }).unwrap();
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct SqliteConnectOptions {
    pub(crate) filename: Cow<'static, Path>,
    pub(crate) in_memory: bool,
    pub(crate) read_only: bool,
    pub(crate) create_if_missing: bool,
    pub(crate) shared_cache: bool,
    pub(crate) statement_cache_capacity: usize,
    pub(crate) busy_timeout: Duration,
    pub(crate) log_settings: LogSettings,
    pub(crate) immutable: bool,

    pub(crate) pragmas: IndexMap<Cow<'static, str>, Option<Cow<'static, str>>>,

    pub(crate) command_channel_size: usize,
    pub(crate) row_channel_size: usize,

    pub(crate) collations: Vec<Collation>,

    pub(crate) serialized: bool,
    pub(crate) thread_name: Arc<DebugFn<dyn Fn(u64) -> String + Send + Sync + 'static>>,
}

impl Default for SqliteConnectOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteConnectOptions {
    /// Construct `Self` with default options.
    ///
    /// See the source of this method for the current defaults.
    pub fn new() -> Self {
        let mut pragmas: IndexMap<Cow<'static, str>, Option<Cow<'static, str>>> = IndexMap::new();

        // Standard pragmas
        //
        // Most of these don't actually need to be sent because they would be set to their
        // default values anyway. See the SQLite documentation for default values of these PRAGMAs:
        // https://www.sqlite.org/pragma.html
        //
        // However, by inserting into the map here, we can ensure that they're set in the proper
        // order, even if they're overwritten later by their respective setters or
        // directly by `pragma()`

        // SQLCipher special case: if the `key` pragma is set, it must be executed first.
        pragmas.insert("key".into(), None);

        // Normally, page_size must be set before any other action on the database.
        // Defaults to 4096 for new databases.
        pragmas.insert("page_size".into(), None);

        // locking_mode should be set before journal_mode:
        // https://www.sqlite.org/wal.html#use_of_wal_without_shared_memory
        pragmas.insert("locking_mode".into(), None);

        // Don't set `journal_mode` unless the user requested it.
        // WAL mode is a permanent setting for created databases and changing into or out of it
        // requires an exclusive lock that can't be waited on with `sqlite3_busy_timeout()`.
        // https://github.com/launchbadge/sqlx/pull/1930#issuecomment-1168165414
        pragmas.insert("journal_mode".into(), None);

        // We choose to enable foreign key enforcement by default, though SQLite normally
        // leaves it off for backward compatibility: https://www.sqlite.org/foreignkeys.html#fk_enable
        pragmas.insert("foreign_keys".into(), Some("ON".into()));

        // The `synchronous` pragma defaults to FULL
        // https://www.sqlite.org/compile.html#default_synchronous.
        pragmas.insert("synchronous".into(), None);

        pragmas.insert("auto_vacuum".into(), None);

        Self {
            filename: Cow::Borrowed(Path::new(":memory:")),
            in_memory: false,
            read_only: false,
            create_if_missing: false,
            shared_cache: false,
            statement_cache_capacity: 100,
            busy_timeout: Duration::from_secs(5),
            log_settings: Default::default(),
            immutable: false,
            pragmas,
            collations: Default::default(),
            serialized: false,
            thread_name: Arc::new(DebugFn(|id| format!("sqlx-sqlite-worker-{}", id))),
            command_channel_size: 50,
            row_channel_size: 50,
        }
    }

    /// Sets the name of the database file.
    pub fn filename(mut self, filename: impl AsRef<Path>) -> Self {
        self.filename = Cow::Owned(filename.as_ref().to_owned());
        self
    }

    /// Set the enforcement of [foreign key constraints](https://www.sqlite.org/pragma.html#pragma_foreign_keys).
    ///
    /// SQLx chooses to enable this by default so that foreign keys function as expected,
    /// compared to other database flavors.
    pub fn foreign_keys(mut self, on: bool) -> Self {
        self.pragma("foreign_keys", if on { "ON" } else { "OFF" })
    }

    /// Set the [`SQLITE_OPEN_SHAREDCACHE` flag](https://sqlite.org/sharedcache.html).
    ///
    /// By default, this is disabled.
    pub fn shared_cache(mut self, on: bool) -> Self {
        self.shared_cache = on;
        self
    }

    /// Sets the [journal mode](https://www.sqlite.org/pragma.html#pragma_journal_mode) for the database connection.
    ///
    /// Journal modes are ephemeral per connection, with the exception of the
    /// [Write-Ahead Log (WAL) mode](https://www.sqlite.org/wal.html).
    ///
    /// A database created in WAL mode retains the setting and will apply it to all connections
    /// opened against it that don't set a `journal_mode`.
    ///
    /// Opening a connection to a database created in WAL mode with a different `journal_mode` will
    /// erase the setting on the database, requiring an exclusive lock to do so.
    /// You may get a `database is locked` (corresponding to `SQLITE_BUSY`) error if another
    /// connection is accessing the database file at the same time.
    ///
    /// SQLx does not set a journal mode by default, to avoid unintentionally changing a database
    /// into or out of WAL mode.
    ///
    /// The default journal mode for non-WAL databases is `DELETE`, or `MEMORY` for in-memory
    /// databases.
    ///
    /// For consistency, any commands in `sqlx-cli` which create a SQLite database will create it
    /// in WAL mode.
    pub fn journal_mode(mut self, mode: SqliteJournalMode) -> Self {
        self.pragma("journal_mode", mode.as_str())
    }

    /// Sets the [locking mode](https://www.sqlite.org/pragma.html#pragma_locking_mode) for the database connection.
    ///
    /// The default locking mode is NORMAL.
    pub fn locking_mode(mut self, mode: SqliteLockingMode) -> Self {
        self.pragma("locking_mode", mode.as_str())
    }

    /// Sets the [access mode](https://www.sqlite.org/c3ref/open.html) to open the database
    /// for read-only access.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets the [access mode](https://www.sqlite.org/c3ref/open.html) to create the database file
    /// if the file does not exist.
    ///
    /// By default, a new file **will not be created** if one is not found.
    pub fn create_if_missing(mut self, create: bool) -> Self {
        self.create_if_missing = create;
        self
    }

    /// Sets the capacity of the connection's statement cache in a number of stored
    /// distinct statements. Caching is handled using LRU, meaning when the
    /// amount of queries hits the defined limit, the oldest statement will get
    /// dropped.
    ///
    /// The default cache capacity is 100 statements.
    pub fn statement_cache_capacity(mut self, capacity: usize) -> Self {
        self.statement_cache_capacity = capacity;
        self
    }

    /// Sets a timeout value to wait when the database is locked, before
    /// returning a busy timeout error.
    ///
    /// The default busy timeout is 5 seconds.
    pub fn busy_timeout(mut self, timeout: Duration) -> Self {
        self.busy_timeout = timeout;
        self
    }

    /// Sets the [synchronous](https://www.sqlite.org/pragma.html#pragma_synchronous) setting for the database connection.
    ///
    /// The default synchronous settings is FULL. However, if durability is not a concern,
    /// then NORMAL is normally all one needs in WAL mode.
    pub fn synchronous(mut self, synchronous: SqliteSynchronous) -> Self {
        self.pragma("synchronous", synchronous.as_str())
    }

    /// Sets the [auto_vacuum](https://www.sqlite.org/pragma.html#pragma_auto_vacuum) setting for the database connection.
    ///
    /// The default auto_vacuum setting is NONE.
    ///
    /// For existing databases, a change to this value does not take effect unless a
    /// [`VACUUM` command](https://www.sqlite.org/lang_vacuum.html) is executed.
    pub fn auto_vacuum(mut self, auto_vacuum: SqliteAutoVacuum) -> Self {
        self.pragma("auto_vacuum", auto_vacuum.as_str())
    }

    /// Sets the [page_size](https://www.sqlite.org/pragma.html#pragma_page_size) setting for the database connection.
    ///
    /// The default page_size setting is 4096.
    ///
    /// For existing databases, a change to this value does not take effect unless a
    /// [`VACUUM` command](https://www.sqlite.org/lang_vacuum.html) is executed.
    /// However, it cannot be changed in WAL mode.
    pub fn page_size(mut self, page_size: u32) -> Self {
        self.pragma("page_size", page_size.to_string())
    }

    /// Sets custom initial pragma for the database connection.
    pub fn pragma<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<Cow<'static, str>>,
        V: Into<Cow<'static, str>>,
    {
        self.pragmas.insert(key.into(), Some(value.into()));
        self
    }

    /// Add a custom collation for comparing strings in SQL.
    ///
    /// If a collation with the same name already exists, it will be replaced.
    ///
    /// See [`sqlite3_create_collation()`](https://www.sqlite.org/c3ref/create_collation.html) for details.
    ///
    /// Note this excerpt:
    /// > The collating function must obey the following properties for all strings A, B, and C:
    /// >
    /// > If A==B then B==A.  
    /// > If A==B and B==C then A==C.  
    /// > If A\<B then B>A.  
    /// > If A<B and B<C then A<C.
    /// >
    /// > If a collating function fails any of the above constraints and that collating function is
    /// > registered and used, then the behavior of SQLite is undefined.
    pub fn collation<N, F>(mut self, name: N, collate: F) -> Self
    where
        N: Into<Arc<str>>,
        F: Fn(&str, &str) -> Ordering + Send + Sync + 'static,
    {
        self.collations.push(Collation::new(name, collate));
        self
    }

    /// Set to `true` to signal to SQLite that the database file is on read-only media.
    ///
    /// If enabled, SQLite assumes the database file _cannot_ be modified, even by higher
    /// privileged processes, and so disables locking and change detection. This is intended
    /// to improve performance but can produce incorrect query results or errors if the file
    /// _does_ change.
    ///
    /// Note that this is different from the `SQLITE_OPEN_READONLY` flag set by
    /// [`.read_only()`][Self::read_only], though the documentation suggests that this
    /// does _imply_ `SQLITE_OPEN_READONLY`.
    ///
    /// See [`sqlite3_open`](https://www.sqlite.org/capi3ref.html#sqlite3_open) (subheading
    /// "URI Filenames") for details.
    pub fn immutable(mut self, immutable: bool) -> Self {
        self.immutable = immutable;
        self
    }

    /// Sets the [threading mode](https://www.sqlite.org/threadsafe.html) for the database connection.
    ///
    /// The default setting is `false` corresponding to using `OPEN_NOMUTEX`.
    /// If set to `true` then `OPEN_FULLMUTEX`.
    ///
    /// See [open](https://www.sqlite.org/c3ref/open.html) for more details.
    ///
    /// ### Note
    /// Setting this to `true` may help if you are getting access violation errors or segmentation
    /// faults, but will also incur a significant performance penalty. You should leave this
    /// set to `false` if at all possible.    
    ///
    /// If you do end up needing to set this to `true` for some reason, please
    /// [open an issue](https://github.com/launchbadge/sqlx/issues/new/choose) as this may indicate
    /// a concurrency bug in SQLx. Please provide clear instructions for reproducing the issue,
    /// including a sample database schema if applicable.
    pub fn serialized(mut self, serialized: bool) -> Self {
        self.serialized = serialized;
        self
    }

    /// Provide a callback to generate the name of the background worker thread.
    ///
    /// The value passed to the callback is an auto-incremented integer for use as the thread ID.
    pub fn thread_name(
        mut self,
        generator: impl Fn(u64) -> String + Send + Sync + 'static,
    ) -> Self {
        self.thread_name = Arc::new(DebugFn(generator));
        self
    }

    /// Set the maximum number of commands to buffer for the worker thread before backpressure is
    /// applied.
    ///
    /// Given that most commands sent to the worker thread involve waiting for a result,
    /// the command channel is unlikely to fill up unless a lot queries are executed in a short
    /// period but cancelled before their full resultsets are returned.
    pub fn command_buffer_size(mut self, size: usize) -> Self {
        self.command_channel_size = size;
        self
    }

    /// Set the maximum number of rows to buffer back to the calling task when a query is executed.
    ///
    /// If the calling task cannot keep up, backpressure will be applied to the worker thread
    /// in order to limit CPU and memory usage.
    pub fn row_buffer_size(mut self, size: usize) -> Self {
        self.row_channel_size = size;
        self
    }
}
