//! Transactional filesystem traits and data structures. Unstable.

use std::collections::HashSet;
use std::convert::TryFrom;
use std::io;
use std::ops::{Deref, DerefMut};

use async_trait::async_trait;
use bytes::Bytes;
use destream::{de, en};
use futures::{future, TryFutureExt, TryStreamExt};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWrite};
use tokio_util::io::StreamReader;

use tc_error::*;
use tc_value::Value;
use tcgeneric::{Id, PathSegment, TCBoxTryFuture};

use super::{Transaction, TxnId};

/// An alias for [`Id`] used for code clarity.
pub type BlockId = PathSegment;

/// The contents of a [`Block`].
#[async_trait]
pub trait BlockData: de::FromStream<Context = ()> + Clone + Send + Sync + 'static {
    fn ext() -> &'static str;

    fn max_size() -> u64;

    async fn hash<'en>(&'en self) -> TCResult<Bytes>
    where
        Self: en::ToStream<'en>,
    {
        let mut data = destream_json::en::encode(self).map_err(TCError::internal)?;
        let mut hasher = Sha256::default();
        while let Some(chunk) = data.try_next().map_err(TCError::internal).await? {
            hasher.update(&chunk);
        }

        let digest = hasher.finalize();
        Ok(Bytes::from(digest.to_vec()))
    }

    async fn load<S: AsyncReadExt + Send + Unpin>(source: S) -> TCResult<Self> {
        destream_json::de::read_from((), source)
            .map_err(|e| TCError::internal(format!("unable to parse saved block: {}", e)))
            .await
    }

    async fn persist<'en, W: AsyncWrite + Send + Unpin>(&'en self, sink: &mut W) -> TCResult<u64>
    where
        Self: en::ToStream<'en>,
    {
        let encoded = destream_json::en::encode(self)
            .map_err(|e| TCError::internal(format!("unable to serialize Value: {}", e)))?;

        let mut reader = StreamReader::new(
            encoded
                .map_ok(Bytes::from)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e)),
        );

        let size = tokio::io::copy(&mut reader, sink)
            .map_err(|e| TCError::bad_gateway(e))
            .await?;

        if size > Self::max_size() {
            log::warn!(
                "{} block exceeds maximum size of {}",
                Self::ext(),
                Self::max_size()
            )
        }

        Ok(size)
    }

    async fn into_size<'en>(self) -> TCResult<u64>
    where
        Self: Clone + en::IntoStream<'en> + 'en,
    {
        let encoded = destream_json::en::encode(self)
            .map_err(|e| TCError::bad_request("serialization error", e))?;

        encoded
            .map_err(|e| TCError::bad_request("serialization error", e))
            .try_fold(0, |size, chunk| {
                future::ready(Ok(size + chunk.len() as u64))
            })
            .await
    }

    async fn size<'en>(&'en self) -> TCResult<u64>
    where
        Self: en::ToStream<'en>,
    {
        let encoded = destream_json::en::encode(self)
            .map_err(|e| TCError::bad_request("serialization error", e))?;

        encoded
            .map_err(|e| TCError::bad_request("serialization error", e))
            .try_fold(0, |size, chunk| {
                future::ready(Ok(size + chunk.len() as u64))
            })
            .await
    }
}

#[async_trait]
impl BlockData for Value {
    fn ext() -> &'static str {
        "value"
    }

    fn max_size() -> u64 {
        4096
    }
}

pub trait BlockRead<B: BlockData, F: File<B>>: Deref<Target = B> + Send {
    fn upgrade(self, file: &F)
        -> TCBoxTryFuture<<<F as File<B>>::Block as Block<B, F>>::WriteLock>;
}

pub trait BlockWrite<B: BlockData, F: File<B>>: DerefMut<Target = B> + Send {
    fn downgrade(
        self,
        file: &F,
    ) -> TCBoxTryFuture<<<F as File<B>>::Block as Block<B, F>>::ReadLock>;
}

/// A transactional filesystem block.
#[async_trait]
pub trait Block<B: BlockData, F: File<B>>: Send + Sync {
    type ReadLock: BlockRead<B, F>;
    type WriteLock: BlockWrite<B, F>;

    /// Get a read lock on this block.
    async fn read(self) -> Self::ReadLock;

    /// Get a write lock on this block.
    async fn write(self) -> Self::WriteLock;
}

/// A transactional persistent data store.
#[async_trait]
pub trait Store: Clone + Send + Sync {
    /// Return `true` if this store contains no data as of the given [`TxnId`].
    async fn is_empty(&self, txn_id: &TxnId) -> TCResult<bool>;
}

/// A transactional file.
#[async_trait]
pub trait File<B: BlockData>: Store + Sized + 'static {
    /// The type of block which this file is divided into.
    type Block: Block<B, Self>;

    /// Return the IDs of all this `File``'s blocks.
    async fn block_ids(&self, txn_id: &TxnId) -> TCResult<HashSet<BlockId>>;

    /// Return a new [`BlockId`] which is not used within this `File`.
    async fn unique_id(&self, txn_id: &TxnId) -> TCResult<BlockId>;

    /// Return true if this `File` contains the given [`BlockId`] as of the given [`TxnId`].
    async fn contains_block(&self, txn_id: &TxnId, name: &BlockId) -> TCResult<bool>;

    /// Copy all blocks from the source `File` into this `File`.
    async fn copy_from(&self, other: &Self, txn_id: TxnId) -> TCResult<()>;

    /// Create a new [`Self::Block`].
    async fn create_block(
        &self,
        txn_id: TxnId,
        name: BlockId,
        initial_value: B,
    ) -> TCResult<Self::Block>;

    /// Delete the block with the given ID.
    async fn delete_block(&self, txn_id: TxnId, name: BlockId) -> TCResult<()>;

    /// Return a lockable owned reference to the block at `name`.
    async fn get_block(&self, txn_id: TxnId, name: BlockId) -> TCResult<Self::Block>;

    /// Get a read lock on the block at `name`.
    async fn read_block(
        &self,
        txn_id: TxnId,
        name: BlockId,
    ) -> TCResult<<Self::Block as Block<B, Self>>::ReadLock>;

    /// Get a read lock on the block at `name`, without borrowing.
    async fn read_block_owned(
        self,
        txn_id: TxnId,
        name: BlockId,
    ) -> TCResult<<Self::Block as Block<B, Self>>::ReadLock>;

    /// Get a read lock on the block at `name` as of [`TxnId`].
    async fn write_block(
        &self,
        txn_id: TxnId,
        name: BlockId,
    ) -> TCResult<<Self::Block as Block<B, Self>>::WriteLock>;

    /// Delete all of this `File`'s blocks.
    async fn truncate(&self, txn_id: TxnId) -> TCResult<()>;
}

/// A transactional directory
#[async_trait]
pub trait Dir: Store + Sized + 'static {
    /// The type of a file entry in this `Dir`
    type File: Send;

    /// The `Class` of a file stored in this `Dir`
    type FileClass;

    /// Return `true` if this directory has an entry at the given [`PathSegment`].
    async fn contains(&self, txn_id: &TxnId, name: &PathSegment) -> TCResult<bool>;

    /// Create a new `Dir`.
    async fn create_dir(&self, txn_id: TxnId, name: PathSegment) -> TCResult<Self>;

    /// Create a new `Dir` with a new unique ID.
    async fn create_dir_tmp(&self, txn_id: TxnId) -> TCResult<Self>;

    /// Create a new [`Self::File`].
    async fn create_file<F: TryFrom<Self::File, Error = TCError>, C: Send>(
        &self,
        txn_id: TxnId,
        name: Id,
        class: C,
    ) -> TCResult<F>
    where
        Self::FileClass: From<C>;

    /// Create a new [`Self::File`] with a new unique ID.
    async fn create_file_tmp<F: TryFrom<Self::File, Error = TCError>, C: Send>(
        &self,
        txn_id: TxnId,
        class: C,
    ) -> TCResult<F>
    where
        Self::FileClass: From<C>;

    /// Look up a subdirectory of this `Dir`.
    async fn get_dir(&self, txn_id: &TxnId, name: &PathSegment) -> TCResult<Option<Self>>;

    /// Get a [`Self::File`] in this `Dir`.
    async fn get_file(&self, txn_id: &TxnId, name: &Id) -> TCResult<Option<Self::File>>;
}

/// Defines how to load a persistent data structure from the filesystem.
#[async_trait]
pub trait Persist<D: Dir, T: Transaction<D>>: Sized {
    type Schema;
    type Store: Store;

    /// Return the schema of this persistent state.
    fn schema(&self) -> &Self::Schema;

    /// Load a saved state from persistent storage.
    async fn load(txn: &T, schema: Self::Schema, store: Self::Store) -> TCResult<Self>;

    /// Save this state to the given `Store` (e.g. to make a copy).
    async fn save(&self, txn_id: TxnId, store: Self::Store) -> TCResult<Self::Schema>;
}

/// Defines how to restore persistent state from backup.
#[async_trait]
pub trait Restore<D: Dir, T: Transaction<D>>: Sized {
    async fn restore(&self, backup: &Self, txn_id: TxnId) -> TCResult<()>;
}
