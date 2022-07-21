use std::collections::HashMap;

use futures_core::future::BoxFuture;

use crate::connection::Connection;
use crate::database::Database;
use crate::error::Error;
use crate::pool::Pool;

mod fixtures;

pub use fixtures::FixtureSnapshot;

pub trait TestSupport: Database {
    /// Get a handle to the single shared `Pool` instance for the currently running binary.
    ///
    /// This `Pool` instance behaves somewhat specially:
    /// * all handles share a single global semaphore to avoid exceeding the max connections
    ///   for the database flavor.
    /// * each unique value of `test_path` results in a different temporary database
    fn shared_test_pool(
        master_opts: &<Self::Connection as Connection>::Options,
        test_path: &str,
    ) -> BoxFuture<'_, Result<Pool<Self>, Error>>;

    /// Take a snapshot of the current state of the database (data only).
    ///
    /// This snapshot can then be diffed with another to generate a fixture.
    fn snapshot(conn: &mut Self::Connection) -> BoxFuture<'_, Result<FixtureSnapshot, Error>>;
}
