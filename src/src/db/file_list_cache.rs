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
                    // The `path` field maps to a DB `TEXT` column (65535-byte
                    // cap); bound it to that capacity so an over-long wire file
                    // name is truncated rather than failing the whole cache
                    // replacement.
                    const MAX_PATH_BYTES: usize = 65535;
                    let path = if file.file_name.len() > MAX_PATH_BYTES {
                        tracing::warn!(
                            "replace_file_list: Truncating over-long file name for job {} ({} > {} bytes)",
                            job_id,
                            file.file_name.len(),
                            MAX_PATH_BYTES
                        );
                        let mut end = MAX_PATH_BYTES;
                        while !file.file_name.is_char_boundary(end) {
                            end -= 1;
                        }
                        file.file_name[..end].to_string()
                    } else {
                        file.file_name.clone()
                    };
                    (file_list_cache::ActiveModel {
                        job_id: Set(job_id),
                        path: Set(path),
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
