use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::db::migration::m20250514_000001_create_job::Migration),
            Box::new(crate::db::migration::m20250514_000002_create_job_history::Migration),
            Box::new(crate::db::migration::m20250514_000003_create_file_download::Migration),
            Box::new(crate::db::migration::m20250514_000004_create_file_list_cache::Migration),
            Box::new(crate::db::migration::m20250514_000005_create_cluster_job::Migration),
            Box::new(crate::db::migration::m20250514_000006_create_cluster_job_status::Migration),
            Box::new(crate::db::migration::m20250514_000007_create_bundle_job::Migration),
            Box::new(crate::db::migration::m20250514_000008_create_cluster_uuid::Migration),
        ]
    }
}
