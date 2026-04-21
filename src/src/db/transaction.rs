//! Database transaction utilities.
//!
//! This module provides helper functions for database transactions to ensure
//! atomicity of multi-step operations.

use sea_orm::{DatabaseConnection, DbErr, TransactionTrait};

/// Execute a database transaction with the provided closure.
///
/// This is a wrapper around SeaORM's transaction API that provides
/// consistent error handling and logging.
pub async fn with_transaction<F, T>(db: &DatabaseConnection, f: F) -> Result<T, DbErr>
where
    F: for<'c> FnOnce(
        &'c sea_orm::DatabaseTransaction,
    ) -> futures_util::future::LocalBoxFuture<'c, Result<T, DbErr>>,
{
    let txn = db.begin().await?;
    match f(&txn).await {
        Ok(result) => {
            txn.commit().await?;
            Ok(result)
        }
        Err(e) => {
            txn.rollback().await?;
            Err(e)
        }
    }
}

// Tests for transaction module would require full schema setup
// Integration tests cover transaction behavior indirectly
