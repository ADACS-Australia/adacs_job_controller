use sea_orm::entity::prelude::*;

/// `SeaORM` entity for the `JobserverClusteruuid` table.
///
/// Stores short-lived WebSocket authentication tokens issued when the controller
/// reconnects an offline cluster via SSH/Kerberos. A remote cluster presents the
/// token on its next WebSocket handshake; expired rows are pruned before lookup.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobserver_clusteruuid")]
#[allow(dead_code)]
pub struct Model {
    /// Auto-increment primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Cluster name this token authorizes (matches `ClusterConfig::name`).
    pub cluster: String,
    /// One-time UUID token passed to the remote cluster for WebSocket auth.
    pub uuid: String,
    /// UTC timestamp when the token was issued; used for expiry pruning.
    pub timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
#[allow(dead_code)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
