// Copyright 2025 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Pool implementation.

use std::{ops::Deref, path::PathBuf, sync::Arc};

use tokio::sync::Mutex;

pub type SqliteConnection = rusqlite::Connection;
pub type LockedSqliteConnection = Arc<Mutex<SqliteConnection>>;
pub type Connection =
    flaque::Object<LockedSqliteConnection, flaque::Async<SqliteConnectionBuilder>>;
pub type Pool = flaque::Pool<LockedSqliteConnection, flaque::Async<SqliteConnectionBuilder>>;

/// Type representing a connection to SQLite from the [`Pool`].
// pub type Connection = flaque::Object<SqliteConnection, flaque::Async<SqliteConnectionBuilder>>;

#[derive(Clone, Copy, Debug)]
pub struct PoolConfig {
    pub desired_size: usize,
}

#[derive(Debug)]
pub struct SqliteConnectionBuilder {
    pub(crate) database_path: PathBuf,
    runtime_config: crate::RuntimeConfig,
}

impl SqliteConnectionBuilder {
    /// Creates a new [`ObjectBuilder`] for a database.
    #[must_use]
    pub(crate) fn new(database_path: PathBuf, runtime_config: crate::RuntimeConfig) -> Self {
        Self { database_path, runtime_config }
    }
}

impl flaque::AsyncBuilder<LockedSqliteConnection> for SqliteConnectionBuilder {
    type Error = rusqlite::Error;

    fn build_object(&self) -> impl Future<Output = Result<LockedSqliteConnection, Self::Error>> {
        let path = self.database_path.clone();
        let pragmas = self.runtime_config.connection_pragmas();

        async move {
            let conn = rusqlite::Connection::open(path)?;
            conn.execute_batch(&pragmas)?;

            Ok(Arc::new(Mutex::new(conn)))
        }
    }
}

/// Gracefully close the store-owned write connection.
///
/// Callers are expected to remove the enclosing [`SqliteConnectionsInner`] from
/// the store before calling this helper so no new write acquisitions can happen
/// through the store API.
///
/// 1. Waits for any in-flight write to complete by acquiring the lock.
/// 2. Runs a WAL checkpoint (TRUNCATE) to flush pending data to the main
///    database file and release WAL locks.
/// 3. Drops the store-owned `Arc` on a blocking thread.
///
/// This is still best effort: if another cloned `Arc` or an
/// `OwnedMutexGuard<Connection>` is still alive elsewhere, the underlying
/// SQLite connection may remain alive until that handle is dropped.
pub async fn close_connection(write_connection: Connection) {
    /*
    // Acquire the lock to wait for any in-flight write to complete.
    let guard = write_connection.lock().await;

    // Flush WAL and release locks while we still own the connection.
    let _ = guard.execute_batch("PRAGMA locking_mode = NORMAL; PRAGMA wal_checkpoint(TRUNCATE);");

    drop(guard);
    */

    // Drop the store-owned Arc on a blocking thread.
    // let _ = tokio::task::spawn_blocking(move ||
    // drop(write_connection)).await;
}

/// Live database connections held by each SQLite store.
pub(crate) struct Connections {
    /// The pool of read connections.
    pub pool: Pool,

    /// The dedicated write connection.
    pub write_connection: Connection,
}

/// The wrapper holding the [`ConnectionsInner`] of a store.
///
/// It derefs to `Mutex<Option<ConnectionsInner>>`, where `Some` means the
/// store is active and `None` means it is closed.
pub(crate) struct MaybeConnections {
    inner: Mutex<Option<Connections>>,
}

impl MaybeConnections {
    /// Create a new wrapper for an active store.
    #[allow(clippy::unused_async)]
    pub async fn new(connections: Connections) -> Self {
        Self { inner: Mutex::new(Some(connections)) }
    }
}

impl Deref for MaybeConnections {
    type Target = Mutex<Option<Connections>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Close a store by taking its connections out.
///
/// After this returns, any new call to `read()` or `write()` through the store
/// will fail with [`crate::error::Error::StoreClosed`] until
/// [`reopen_connections`] is called.
///
/// Idempotent: if the store is already closed this is a no-op.
pub(crate) async fn close_connections(connections: &MaybeConnections, label: &str) {
    /*
    let mut guard = connections.lock().await;
    let Some(conns) = guard.take() else {
        // Already closed — idempotent.
        return;
    };

    let SqliteConnectionsInner { pool, write_connection } = conns;

    // Close the pool. Idle read connections are dropped immediately; in-flight
    // reads complete and their connections are discarded (not recycled) on
    // release. New pool.get() calls return PoolError::Closed.
    pool.close();

    let status = pool.status();
    info!(
        size = status.size,
        max_size = status.max_size,
        available = status.available,
        "{label} pause: pool closed"
    );

    // Close the write connection: wait for any in-flight write to finish, run a
    // WAL checkpoint, then drop on a blocking thread.
    close_connection(write_connection).await;

    let status = pool.status();
    info!(
        size = status.size,
        max_size = status.max_size,
        available = status.available,
        "{label} pause: write connection released"
    );

    // Wait for any in-flight read connections to drain. The write connection
    // has already been released above, so pool.status().size == 0 now correctly
    // means every connection is gone and no SQLite file locks are held.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while pool.status().size > 0 {
        if tokio::time::Instant::now() >= deadline {
            let status = pool.status();
            warn!(
                size = status.size,
                max_size = status.max_size,
                available = status.available,
                "Timed out waiting for SQLite pool connections to drain"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    */
}

/// Resume a store by rebuilding its connections.
///
/// Idempotent: if the store is already active this is a no-op.
pub(crate) async fn reopen_connections(
    connections: &MaybeConnections,
    db_path: PathBuf,
    pool_config: PoolConfig,
    runtime_config: crate::RuntimeConfig,
) -> crate::error::Result<()> {
    /*
    use crate::utils::SqliteAsyncConnExt as _;

    let mut guard = connections.lock().await;
    if guard.is_some() {
        // Not closed — idempotent.
        return Ok(());
    }

    // Rebuild the pool (connections are created lazily on first get()).
    let pool = Pool::builder(Manager::new(db_path, runtime_config))
        .config(pool_config)
        .runtime(RUNTIME)
        .build()
        .map_err(|e| crate::error::Error::InvalidData {
            details: format!("Failed to rebuild connection pool: {e}"),
        })?;

    let write_conn = pool.get().await?;
    write_conn.apply_runtime_config(runtime_config).await?;

    *guard =
        Some(SqliteConnectionsInner { pool, write_connection: Arc::new(Mutex::new(write_conn)) });

    Ok(())
    */
    Ok(())
}

#[cfg(all(test, feature = "state-store"))]
mod tests {
    use std::thread;

    use matrix_sdk_base::StateStore as _;
    use tempfile::tempdir;

    use crate::SqliteStateStore;

    /// Dropping a store from a thread that isn't running in a Tokio runtime
    /// must not panic.
    #[test]
    fn test_store_can_be_dropped_outside_of_a_runtime() {
        let tmpdir = tempdir().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread().build().unwrap();

        let store = runtime.block_on(async {
            let store = SqliteStateStore::open(tmpdir.path(), None).await.unwrap();

            // Use a read connection, so the pool holds an idle one too.
            store.get_kv_data(matrix_sdk_base::store::StateStoreDataKey::SyncToken).await.unwrap();

            store
        });

        // The runtime is still alive, but this thread isn't running within it.
        thread::spawn(move || drop(store)).join().expect("dropping the store must not panic");
    }
}
