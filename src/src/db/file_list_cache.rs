use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    TransactionTrait,
};

use crate::db::entities::file_list_cache;
use crate::protocol::types::FileInfo;

/// Replace the cached file list for a job with the given entries.
///
/// Deletes any existing rows for `job_id`, then inserts one row per file.
/// Both steps run inside a single transaction so the cache is either fully
/// replaced or left unchanged (atomic replacement); a mid-write failure rolls
/// back the whole replacement instead of leaving a partial or empty cache.
pub async fn replace_file_list(db: &DatabaseConnection, job_id: i64, files: &[FileInfo]) {
    let files = files.to_vec();
    if let Err(e) = db
        .transaction::<_, _, sea_orm::DbErr>(|txn| {
            Box::pin(async move {
                file_list_cache::Entity::delete_many()
                    .filter(file_list_cache::Column::JobId.eq(job_id))
                    .exec(txn)
                    .await?;
                for file in files {
                    (file_list_cache::ActiveModel {
                        job_id: Set(job_id),
                        path: Set(file.file_name.clone()),
                        is_dir: Set(file.is_directory),
                        file_size: Set(file.file_size.cast_signed()),
                        permissions: Set(file.permissions.cast_signed()),
                        ..Default::default()
                    })
                    .insert(txn)
                    .await?;
                }
                Ok(())
            })
        })
        .await
    {
        tracing::error!(
            "replace_file_list: transaction failed for job {}: {}",
            job_id,
            e
        );
    }
}
