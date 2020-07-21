use std::convert::{TryFrom, TryInto};
use std::sync::Arc;

use async_trait::async_trait;
use futures::future;
use futures::{Stream, StreamExt};

use crate::error;
use crate::transaction::{Txn, TxnId};
use crate::value::{TCResult, TCStream, Value, ValueId};

mod index;
mod schema;
mod view;

pub type Bounds = schema::Bounds;
pub type Column = schema::Column;
pub type Schema = schema::Schema;
pub type TableBase = index::TableBase;

#[async_trait]
pub trait Selection: Clone + Into<Table> + Sized + Send + Sync + 'static {
    type Stream: Stream<Item = Vec<Value>> + Send + Sync + Unpin;

    async fn count(&self, txn_id: TxnId) -> TCResult<u64> {
        let count = self
            .clone()
            .stream(txn_id)
            .await?
            .fold(0, |count, _| future::ready(count + 1))
            .await;
        Ok(count)
    }

    async fn delete(self, _txn_id: TxnId) -> TCResult<()> {
        Err(error::unsupported(
            "This table view does not support deletion (try deleting a slice of the source table)",
        ))
    }

    async fn delete_row(&self, _txn_id: &TxnId, _row: schema::Row) -> TCResult<()> {
        Err(error::unsupported("This table view does not support row deletion (try deleting from the source table directly)"))
    }

    async fn index(
        &self,
        txn: Arc<Txn>,
        columns: Option<Vec<ValueId>>,
    ) -> TCResult<index::ReadOnly> {
        index::ReadOnly::copy_from(self.clone().into(), txn, columns).await
    }

    fn limit(&self, limit: u64) -> TCResult<Arc<view::Limited>> {
        let limited = view::Limited::try_from((self.clone().into(), limit))?;
        Ok(Arc::new(limited))
    }

    async fn order_by(
        &self,
        txn: Arc<Txn>,
        columns: Vec<ValueId>,
        reverse: bool,
    ) -> TCResult<Table> {
        if self.schema().starts_with(&columns) {
            if reverse {
                self.reversed()
            } else {
                Ok(self.clone().into())
            }
        } else {
            let index = self.index(txn, Some(columns)).await?;
            if reverse {
                index.reversed()
            } else {
                Ok(index.into())
            }
        }
    }

    fn reversed(&self) -> TCResult<Table>;

    fn select(&self, columns: Vec<ValueId>) -> TCResult<view::ColumnSelection> {
        let selection = (self.clone().into(), columns).try_into()?;
        Ok(selection)
    }

    fn schema(&'_ self) -> &'_ schema::Schema;

    async fn slice(&self, _txn_id: &TxnId, _bounds: schema::Bounds) -> TCResult<Table> {
        Err(error::unsupported(
            "This table view does not support slicing (consider slicing the source table directly)",
        ))
    }

    async fn stream(&self, txn_id: TxnId) -> TCResult<Self::Stream>;

    async fn validate(&self, txn_id: &TxnId, bounds: &schema::Bounds) -> TCResult<()>;

    async fn update(self, _txn: Arc<Txn>, _value: schema::Row) -> TCResult<()> {
        Err(error::unsupported(
            "This table view does not support updates (consider updating a slice of the source table)",
        ))
    }

    async fn update_row(
        &self,
        _txn_id: TxnId,
        _row: schema::Row,
        _value: schema::Row,
    ) -> TCResult<()> {
        Err(error::unsupported("This table view does not support updates (consider updating a row in the source table directly)"))
    }
}

#[derive(Clone)]
pub enum Table {
    Columns(view::ColumnSelection),
    Limit(view::Limited),
    Table(index::TableBase),
    Index(index::Index),
    IndexSlice(view::IndexSlice),
    ROIndex(index::ReadOnly),
    TableSlice(view::TableSlice),
}

#[async_trait]
impl Selection for Table {
    type Stream = TCStream<Vec<Value>>;

    async fn count(&self, txn_id: TxnId) -> TCResult<u64> {
        match self {
            Self::Columns(columns) => columns.count(txn_id).await,
            Self::Limit(limited) => limited.count(txn_id).await,
            Self::Table(table) => table.count(txn_id).await,
            Self::Index(index) => index.count(txn_id).await,
            Self::IndexSlice(index_slice) => index_slice.count(txn_id).await,
            Self::ROIndex(ro_index) => ro_index.count(txn_id).await,
            Self::TableSlice(table_slice) => table_slice.count(txn_id).await,
        }
    }

    async fn delete(self, txn_id: TxnId) -> TCResult<()> {
        match self {
            Self::Columns(columns) => columns.clone().delete(txn_id).await,
            Self::Limit(limited) => limited.clone().delete(txn_id).await,
            Self::Table(table) => table.clone().delete(txn_id).await,
            Self::Index(index) => index.clone().delete(txn_id).await,
            Self::IndexSlice(index_slice) => index_slice.clone().delete(txn_id).await,
            Self::ROIndex(ro_index) => ro_index.clone().delete(txn_id).await,
            Self::TableSlice(table_slice) => table_slice.clone().delete(txn_id).await,
        }
    }

    async fn delete_row(&self, txn_id: &TxnId, row: schema::Row) -> TCResult<()> {
        match self {
            Self::Columns(columns) => columns.delete_row(txn_id, row).await,
            Self::Limit(limited) => limited.delete_row(txn_id, row).await,
            Self::Table(table) => table.delete_row(txn_id, row).await,
            Self::Index(index) => index.delete_row(txn_id, row).await,
            Self::IndexSlice(index_slice) => index_slice.delete_row(txn_id, row).await,
            Self::ROIndex(ro_index) => ro_index.delete_row(txn_id, row).await,
            Self::TableSlice(table_slice) => table_slice.delete_row(txn_id, row).await,
        }
    }

    fn reversed(&self) -> TCResult<Table> {
        match self {
            Self::Columns(columns) => columns.reversed(),
            Self::Limit(limited) => limited.reversed(),
            Self::Table(table) => table.reversed(),
            Self::Index(index) => index.reversed(),
            Self::IndexSlice(index_slice) => index_slice.reversed(),
            Self::ROIndex(ro_index) => ro_index.reversed(),
            Self::TableSlice(table_slice) => table_slice.reversed(),
        }
    }

    fn schema(&'_ self) -> &'_ schema::Schema {
        match self {
            Self::Columns(columns) => columns.schema(),
            Self::Limit(limited) => limited.schema(),
            Self::Table(table) => table.schema(),
            Self::Index(index) => index.schema(),
            Self::IndexSlice(index_slice) => index_slice.schema(),
            Self::ROIndex(ro_index) => ro_index.schema(),
            Self::TableSlice(table_slice) => table_slice.schema(),
        }
    }

    async fn slice(&self, txn_id: &TxnId, bounds: schema::Bounds) -> TCResult<Table> {
        match self {
            Self::Columns(columns) => columns.slice(txn_id, bounds).await,
            Self::Limit(limited) => limited.slice(txn_id, bounds).await,
            Self::Table(table) => table.slice(txn_id, bounds).await,
            Self::Index(index) => index.slice(txn_id, bounds).await,
            Self::IndexSlice(index_slice) => index_slice.slice(txn_id, bounds).await,
            Self::ROIndex(ro_index) => ro_index.slice(txn_id, bounds).await,
            Self::TableSlice(table_slice) => table_slice.slice(txn_id, bounds).await,
        }
    }

    async fn stream(&self, txn_id: TxnId) -> TCResult<Self::Stream> {
        match self {
            Self::Columns(columns) => columns.clone().stream(txn_id).await,
            Self::Limit(limited) => limited.clone().stream(txn_id).await,
            Self::Table(table) => table.clone().stream(txn_id).await,
            Self::Index(index) => index.clone().stream(txn_id).await,
            Self::IndexSlice(index_slice) => index_slice.clone().stream(txn_id).await,
            Self::ROIndex(ro_index) => ro_index.clone().stream(txn_id).await,
            Self::TableSlice(table_slice) => table_slice.clone().stream(txn_id).await,
        }
    }

    async fn update(self, txn: Arc<Txn>, value: schema::Row) -> TCResult<()> {
        match self {
            Self::Columns(columns) => columns.clone().update(txn, value).await,
            Self::Limit(limited) => limited.clone().update(txn, value).await,
            Self::Table(table) => table.clone().update(txn, value).await,
            Self::Index(index) => index.clone().update(txn, value).await,
            Self::IndexSlice(index_slice) => index_slice.clone().update(txn, value).await,
            Self::ROIndex(ro_index) => ro_index.update(txn, value).await,
            Self::TableSlice(table_slice) => table_slice.update(txn, value).await,
        }
    }

    async fn update_row(
        &self,
        txn_id: TxnId,
        row: schema::Row,
        value: schema::Row,
    ) -> TCResult<()> {
        match self {
            Self::Columns(columns) => columns.update_row(txn_id, row, value).await,
            Self::Limit(limited) => limited.update_row(txn_id, row, value).await,
            Self::Table(table) => table.update_row(txn_id, row, value).await,
            Self::Index(index) => index.update_row(txn_id, row, value).await,
            Self::IndexSlice(index_slice) => index_slice.update_row(txn_id, row, value).await,
            Self::ROIndex(ro_index) => ro_index.update_row(txn_id, row, value).await,
            Self::TableSlice(table_slice) => table_slice.update_row(txn_id, row, value).await,
        }
    }

    async fn validate(&self, txn_id: &TxnId, bounds: &schema::Bounds) -> TCResult<()> {
        match self {
            Self::Columns(columns) => columns.validate(txn_id, bounds).await,
            Self::Limit(limited) => limited.validate(txn_id, bounds).await,
            Self::Table(table) => table.validate(txn_id, bounds).await,
            Self::Index(index) => index.validate(txn_id, bounds).await,
            Self::IndexSlice(index_slice) => index_slice.validate(txn_id, bounds).await,
            Self::ROIndex(ro_index) => ro_index.validate(txn_id, bounds).await,
            Self::TableSlice(table_slice) => table_slice.validate(txn_id, bounds).await,
        }
    }
}

impl From<view::ColumnSelection> for Table {
    fn from(columns: view::ColumnSelection) -> Table {
        Table::Columns(columns)
    }
}

impl From<view::Limited> for Table {
    fn from(limited: view::Limited) -> Table {
        Table::Limit(limited)
    }
}

impl From<index::Index> for Table {
    fn from(index: index::Index) -> Table {
        Table::Index(index)
    }
}

impl From<view::IndexSlice> for Table {
    fn from(index_slice: view::IndexSlice) -> Table {
        Table::IndexSlice(index_slice)
    }
}

impl From<index::TableBase> for Table {
    fn from(table: index::TableBase) -> Table {
        Table::Table(table)
    }
}

impl From<index::ReadOnly> for Table {
    fn from(ro_index: index::ReadOnly) -> Table {
        Table::ROIndex(ro_index)
    }
}

impl From<view::TableSlice> for Table {
    fn from(table_slice: view::TableSlice) -> Table {
        Table::TableSlice(table_slice)
    }
}
