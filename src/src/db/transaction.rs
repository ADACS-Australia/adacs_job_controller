//! Database transaction utilities.
//!
//! This module provides helper functions for database transactions to ensure
//! atomicity of multi-step operations.

use sea_orm::{DatabaseConnection, DbErr, TransactionTrait};

/// Execute a database transaction with the provided closure.
///
/// This is a wrapper around `SeaORM`'s transaction API that provides
/// consistent error handling and logging.
///
/// # Errors
///
/// Returns a database error if:
/// - Transaction cannot be created
/// - The closure execution fails
/// - Transaction commit or rollback fails
#[allow(dead_code)]
pub async fn with_transaction<F, T>(db: &DatabaseConnection, f: F) -> Result<T, DbErr>
where
    F: for<'c> FnOnce(
        &'c sea_orm::DatabaseTransaction,
    ) -> futures_util::future::LocalBoxFuture<'c, Result<T, DbErr>>,
{
    tracing::trace!("DB: Starting transaction");
    let txn = db.begin().await?;
    tracing::trace!("DB: Transaction begun");

    match f(&txn).await {
        Ok(result) => {
            tracing::trace!("DB: Committing transaction");
            txn.commit().await?;
            tracing::debug!("DB: Transaction committed successfully");
            Ok(result)
        }
        Err(e) => {
            tracing::warn!("DB: Transaction failed: {}, rolling back", e);
            txn.rollback().await?;
            tracing::debug!("DB: Transaction rolled back");
            Err(e)
        }
    }
}

// Tests for transaction module would require full schema setup
// Integration tests cover transaction behavior indirectly
