use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize)]
#[sea_orm(table_name = "watch_progress")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub page_id: i32,
    pub position_ms: i64,
    pub completed: bool,
    pub revision: i64,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
