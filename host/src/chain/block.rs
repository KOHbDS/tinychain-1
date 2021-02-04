use std::convert::TryFrom;
use std::fmt;

use async_trait::async_trait;
use bytes::Bytes;
use destream::{de, en};
use futures::TryFutureExt;

use error::*;
use transact::fs::BlockData;
use transact::lock::Mutate;
use transact::TxnId;

use crate::scalar::OpRef;

#[derive(Clone)]
pub struct ChainBlock {
    hash: Bytes,
    contents: Vec<OpRef>,
}

impl ChainBlock {
    pub fn append(&mut self, op_ref: OpRef) {
        self.contents.push(op_ref);
    }
}

#[async_trait]
impl Mutate for ChainBlock {
    type Pending = Self;

    fn diverge(&self, _txn_id: &TxnId) -> Self::Pending {
        self.clone()
    }

    async fn converge(&mut self, new_value: Self::Pending) {
        *self = new_value;
    }
}

impl BlockData for ChainBlock {}

#[async_trait]
impl de::FromStream for ChainBlock {
    type Context = ();

    async fn from_stream<D: de::Decoder>(context: (), decoder: &mut D) -> Result<Self, D::Error> {
        de::FromStream::from_stream(context, decoder)
            .map_ok(|(hash, contents)| Self { hash, contents })
            .await
    }
}

impl<'en> en::IntoStream<'en> for ChainBlock {
    fn into_stream<E: en::Encoder<'en>>(self, encoder: E) -> Result<E::Ok, E::Error> {
        let hash = base64::encode(self.hash);
        en::IntoStream::into_stream((hash, self.contents), encoder)
    }
}

impl TryFrom<Bytes> for ChainBlock {
    type Error = TCError;

    fn try_from(_data: Bytes) -> TCResult<Self> {
        unimplemented!()
    }
}

impl From<ChainBlock> for Bytes {
    fn from(_block: ChainBlock) -> Bytes {
        unimplemented!()
    }
}

impl fmt::Display for ChainBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("(chain block)")
    }
}