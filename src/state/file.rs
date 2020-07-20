use std::collections::{HashMap, HashSet};
use std::convert::{TryFrom, TryInto};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use uuid::Uuid;

use crate::error;
use crate::internal::hostfs;
use crate::internal::lock::RwLock;
use crate::transaction::lock::{Mutable, Mutate, TxnLock, TxnLockReadGuard, TxnLockWriteGuard};
use crate::transaction::{Transact, TxnId};
use crate::value::link::PathSegment;
use crate::value::TCResult;

const ERR_CORRUPT: &str = "Data corruption error detected! Please file a bug report.";
const TXN_CACHE: &str = ".pending";

pub type BlockId = PathSegment;

pub struct Block<'a, T: BlockData> {
    file: &'a File<T>,
    block_id: BlockId,
    lock: TxnLockReadGuard<T>,
}

impl<'a, T: BlockData> Block<'a, T> {
    pub async fn upgrade(self) -> TCResult<BlockMut<'a, T>> {
        self.file
            .mutate(self.lock.txn_id().clone(), self.block_id.clone())
            .await?;
        Ok(BlockMut {
            file: self.file,
            block_id: self.block_id,
            lock: self.lock.upgrade().await?,
        })
    }
}

impl<'a, T: BlockData> Deref for Block<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.lock.deref()
    }
}

pub struct BlockMut<'a, T: BlockData> {
    file: &'a File<T>,
    block_id: BlockId,
    lock: TxnLockWriteGuard<T>,
}

impl<'a, T: BlockData> BlockMut<'a, T> {
    pub async fn downgrade(self, txn_id: &'a TxnId) -> TCResult<Block<'a, T>> {
        Ok(Block {
            file: self.file,
            block_id: self.block_id,
            lock: self.lock.downgrade(txn_id).await?,
        })
    }
}

impl<'a, T: BlockData> Deref for BlockMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.lock.deref()
    }
}

impl<'a, T: BlockData> DerefMut for BlockMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.lock.deref_mut()
    }
}

pub struct BlockOwned<T: BlockData> {
    lock: TxnLockReadGuard<T>,
}

impl<T: BlockData> Deref for BlockOwned<T> {
    type Target = T;

    fn deref(&'_ self) -> &'_ T {
        self.lock.deref()
    }
}

pub trait BlockData:
    Clone + Send + Sync + TryFrom<Bytes, Error = error::TCError> + Into<Bytes>
{
}

#[async_trait]
impl<T: BlockData> Mutate for T {
    type Pending = Self;

    fn diverge(&self, _txn_id: &TxnId) -> Self {
        self.clone()
    }

    async fn converge(&mut self, other: Self) {
        *self = other;
    }
}

pub struct File<T: BlockData> {
    dir: RwLock<hostfs::Dir>,
    pending: RwLock<hostfs::Dir>,
    listing: TxnLock<Mutable<HashSet<BlockId>>>,
    cache: RwLock<HashMap<BlockId, TxnLock<T>>>,
    mutated: TxnLock<Mutable<HashSet<BlockId>>>,
}

impl<T: BlockData> File<T> {
    pub async fn create(txn_id: TxnId, dir: RwLock<hostfs::Dir>) -> TCResult<Arc<File<T>>> {
        let mut lock = dir.write().await;
        if !lock.is_empty() {
            return Err(error::bad_request(
                "Tried to create a new File but there is already data in the cache!",
                "(filesystem cache)",
            ));
        }

        Ok(Arc::new(File {
            dir,
            pending: lock.create_dir(TXN_CACHE.parse()?)?,
            listing: TxnLock::new(txn_id.clone(), HashSet::new().into()),
            cache: RwLock::new(HashMap::new()),
            mutated: TxnLock::new(txn_id, HashSet::new().into()),
        }))
    }

    pub async fn unique_id(&self, txn_id: &TxnId) -> TCResult<BlockId> {
        let existing_ids = self.block_ids(txn_id).await?;
        loop {
            let id: PathSegment = Uuid::new_v4().into();
            if !existing_ids.contains(&id) {
                return Ok(id);
            }
        }
    }

    async fn block_ids(&'_ self, txn_id: &'_ TxnId) -> TCResult<HashSet<BlockId>> {
        self.listing
            .read(txn_id)
            .await
            .map(|block_ids| block_ids.clone())
    }

    async fn mutate(&self, txn_id: TxnId, block_id: BlockId) -> TCResult<()> {
        self.mutated.write(txn_id).await?.insert(block_id);
        Ok(())
    }

    pub async fn create_block(
        &self,
        txn_id: TxnId,
        block_id: BlockId,
        data: T,
    ) -> TCResult<Block<'_, T>> {
        if block_id.to_string() == TXN_CACHE {
            return Err(error::bad_request("This name is reserved", block_id));
        }

        let mut listing = self.listing.write(txn_id.clone()).await?;
        if listing.contains(&block_id) {
            return Err(error::bad_request(
                "There is already a block called",
                block_id,
            ));
        }
        listing.insert(block_id.clone());
        let txn_lock = TxnLock::new(txn_id.clone(), data);
        self.cache
            .write()
            .await
            .insert(block_id.clone(), txn_lock.clone());
        let lock = txn_lock.read(&txn_id).await?;
        Ok(Block {
            file: self,
            block_id,
            lock,
        })
    }

    pub async fn get_block<'a>(
        &'a self,
        txn_id: &'a TxnId,
        block_id: BlockId,
    ) -> TCResult<Block<'a, T>> {
        let lock = self.lock_block(txn_id, &block_id).await?;
        Ok(Block {
            file: self,
            block_id,
            lock,
        })
    }

    pub async fn get_block_owned(
        self: Arc<Self>,
        txn_id: TxnId,
        block_id: BlockId,
    ) -> TCResult<BlockOwned<T>> {
        let lock = self.lock_block(&txn_id, &block_id).await?;
        Ok(BlockOwned { lock })
    }

    async fn lock_block(
        &self,
        txn_id: &TxnId,
        block_id: &BlockId,
    ) -> TCResult<TxnLockReadGuard<T>> {
        if let Some(block) = self.cache.read().await.get(block_id) {
            block.read(txn_id).await
        } else if self.listing.read(txn_id).await?.contains(block_id) {
            let block =
                if let Some(txn_dir) = self.pending.read().await.get_dir(&txn_id.clone().into())? {
                    if let Some(block) = txn_dir.read().await.get_block(block_id)? {
                        block
                    } else {
                        self.dir
                            .read()
                            .await
                            .get_block(&block_id)?
                            .ok_or_else(|| error::internal(ERR_CORRUPT))?
                    }
                } else {
                    self.dir
                        .read()
                        .await
                        .get_block(&block_id)?
                        .ok_or_else(|| error::internal(ERR_CORRUPT))?
                };

            let block = block.read().await;
            let txn_lock = TxnLock::new(txn_id.clone(), (*block).clone().try_into()?);
            let block = txn_lock.read(txn_id).await?;
            self.cache.write().await.insert(block_id.clone(), txn_lock);
            Ok(block)
        } else {
            Err(error::not_found(block_id))
        }
    }

    pub async fn is_empty(&self, txn_id: &TxnId) -> TCResult<bool> {
        Ok(self.listing.read(txn_id).await?.is_empty())
    }
}

#[async_trait]
impl<T: BlockData> Transact for File<T> {
    async fn commit(&self, txn_id: &TxnId) {
        let new_listing = self.listing.read(txn_id).await.unwrap();
        let old_listing = self.listing.canonical().value();

        let mut dir = self.dir.write().await;
        for block_id in old_listing.difference(&new_listing) {
            dir.delete_block(block_id).unwrap();
        }

        self.listing.commit(txn_id).await;

        let mut mutated: Vec<BlockId> = self
            .mutated
            .write(txn_id.clone())
            .await
            .unwrap()
            .drain()
            .collect();
        self.mutated.commit(txn_id).await;

        let mut pending = self.pending.write().await;
        let txn_dir_id: PathSegment = txn_id.clone().into();
        if mutated.is_empty() {
            pending.delete_dir(&txn_dir_id).unwrap();
            return;
        }

        let cache = self.cache.read().await;
        let mut txn_dir = pending
            .create_or_get_dir(&txn_dir_id)
            .unwrap()
            .write()
            .await;

        // TODO: run these copy ops in parallel
        for block_id in mutated.drain(..) {
            if let Some(lock) = cache.get(&block_id) {
                txn_dir
                    .create_or_get_block(
                        &block_id,
                        lock.read(txn_id).await.unwrap().deref().clone().into(),
                    )
                    .await
                    .unwrap();
                lock.commit(txn_id).await;
            }
        }

        dir.move_all(txn_dir.deref_mut()).unwrap();
        pending.delete_dir(&txn_dir_id).unwrap();
    }

    async fn rollback(&self, txn_id: &TxnId) {
        self.pending
            .write()
            .await
            .delete_dir(&txn_id.clone().into())
            .unwrap();
        self.listing.rollback(txn_id).await;
    }
}
