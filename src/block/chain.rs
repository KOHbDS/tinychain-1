use std::convert::TryFrom;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::join;

use crate::class::TCResult;
use crate::collection::Collect;
use crate::error;
use crate::transaction::lock::{Mutable, TxnLock};
use crate::transaction::{Transact, Txn, TxnId};

use super::file::File;
use super::BlockData;

#[derive(Clone)]
pub struct ChainBlock {}

impl TryFrom<Bytes> for ChainBlock {
    type Error = error::TCError;

    fn try_from(_data: Bytes) -> TCResult<ChainBlock> {
        Err(error::not_implemented())
    }
}

impl From<ChainBlock> for Bytes {
    fn from(_block: ChainBlock) -> Bytes {
        unimplemented!()
    }
}

impl BlockData for ChainBlock {}

pub struct Chain<O: Collect> {
    file: Arc<File<ChainBlock>>,
    object: O,
    latest_block: TxnLock<Mutable<u64>>,
}

impl<O: Collect> Chain<O> {
    pub async fn create(txn: Arc<Txn>, object: O) -> TCResult<Chain<O>> {
        let file = txn.context().await?;
        let latest_block = TxnLock::new(txn.id().clone(), 0.into());
        Ok(Chain {
            file,
            object,
            latest_block,
        })
    }
}

#[async_trait]
impl<O: Collect> Transact for Chain<O> {
    async fn commit(&self, txn_id: &TxnId) {
        self.object.commit(txn_id).await;
        // don't commit the Chain until the actual changes are committed, for crash recovery
        join!(self.file.commit(txn_id), self.latest_block.commit(txn_id));
    }

    async fn rollback(&self, txn_id: &TxnId) {
        self.object.rollback(txn_id).await;
        join!(
            self.file.rollback(txn_id),
            self.latest_block.rollback(txn_id)
        );
    }
}