use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};

use crate::db::entities::file_list_cache;
use crate::protocol::types::FileInfo;

/// Replace the cached file list for a job with the given entries.
///
/// Deletes any existing rows for `job_id`, then inserts one row per file.
pub async fn replace_file_list(db: &DatabaseConnection, job_id: i64, files: &[FileInfo]) {
    let _ = file_list_cache::Entity::delete_many()
        .filter(file_list_cache::Column::JobId.eq(job_id))
        .exec(db)
        .await;
    for file in files {
        let _ = file_list_cache::ActiveModel {
            job_id: Set(job_id),
            path: Set(file.file_name.clone()),
            is_dir: Set(file.is_directory),
            file_size: Set(file.file_size.cast_signed()),
            permissions: Set(file.permissions.cast_signed()),
            ..Default::default()
        }
        .insert(db)
        .await;
    }
}
