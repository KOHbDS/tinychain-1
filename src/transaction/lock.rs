use std::cell::UnsafeCell;
use std::collections::{BTreeMap, VecDeque};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::future::{self, Future};
use futures::task::{Context, Poll, Waker};

use crate::error;
use crate::value::TCResult;

use super::{Transact, TxnId};

#[async_trait]
pub trait Mutable: Clone + Send + Sync {
    async fn commit(&mut self, txn_id: &TxnId, new_value: Self);
}

pub struct TxnLockReadGuard<T: Mutable> {
    txn_id: TxnId,
    lock: TxnLock<T>,
}

impl<T: Mutable> Deref for TxnLockReadGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*self
                .lock
                .inner
                .lock()
                .unwrap()
                .value_at
                .get(&self.txn_id)
                .unwrap()
                .get()
        }
    }
}

impl<T: Mutable> Drop for TxnLockReadGuard<T> {
    fn drop(&mut self) {
        let lock = &mut self.lock.inner.lock().unwrap();
        match lock.state.readers.get_mut(&self.txn_id) {
            Some(count) if *count > 1 => (*count) -= 1,
            Some(1) => {
                lock.state.readers.remove(&self.txn_id);

                while let Some(waker) = lock.state.wakers.pop_front() {
                    waker.wake()
                }

                lock.state.wakers.shrink_to_fit()
            }
            _ => panic!("TxnLockReadGuard count updated incorrectly!"),
        }
    }
}

pub struct TxnLockWriteGuard<T: Mutable> {
    txn_id: TxnId,
    lock: TxnLock<T>,
}

impl<T: Mutable> Deref for TxnLockWriteGuard<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*self
                .lock
                .inner
                .lock()
                .unwrap()
                .value_at
                .get(&self.txn_id)
                .unwrap()
                .get()
        }
    }
}

impl<T: Mutable> DerefMut for TxnLockWriteGuard<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {
            &mut *self
                .lock
                .inner
                .lock()
                .unwrap()
                .value_at
                .get_mut(&self.txn_id)
                .unwrap()
                .get()
        }
    }
}

impl<T: Mutable> Drop for TxnLockWriteGuard<T> {
    fn drop(&mut self) {
        let lock = &mut self.lock.inner.lock().unwrap();
        lock.state.writer = false;

        while let Some(waker) = lock.state.wakers.pop_front() {
            waker.wake()
        }

        lock.state.wakers.shrink_to_fit();
    }
}

struct LockState {
    last_commit: TxnId,
    readers: BTreeMap<TxnId, usize>,
    reserved: Option<TxnId>,
    writer: bool,
    wakers: VecDeque<Waker>,
}

struct Inner<T: Mutable> {
    state: LockState,
    value: UnsafeCell<T>,
    value_at: BTreeMap<TxnId, UnsafeCell<T>>,
}

#[derive(Clone)]
pub struct TxnLock<T: Mutable> {
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T: Mutable> TxnLock<T> {
    pub fn new(last_commit: TxnId, value: T) -> TxnLock<T> {
        let state = LockState {
            last_commit,
            readers: BTreeMap::new(),
            reserved: None,
            writer: false,
            wakers: VecDeque::new(),
        };

        let inner = Inner {
            state,
            value: UnsafeCell::new(value),
            value_at: BTreeMap::new(),
        };

        TxnLock {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    pub fn try_read<'a>(&self, txn_id: &'a TxnId) -> TCResult<Option<TxnLockReadGuard<T>>> {
        let lock = &mut self.inner.lock().unwrap();

        if txn_id < &lock.state.last_commit && !lock.value_at.contains_key(txn_id) {
            // If the requested time is too old, just return an error.
            // We can't keep track of every historical version here.
            Err(error::conflict())
        } else if lock.state.reserved.is_some() && txn_id >= lock.state.reserved.as_ref().unwrap() {
            // If a writer can mutate the locked value at the requested time, wait it out.
            Ok(None)
        } else {
            // Otherwise, return a ReadGuard.
            if !lock.value_at.contains_key(txn_id) {
                let value_at_txn_id = UnsafeCell::new(unsafe { (&*lock.value.get()).clone() });
                lock.value_at.insert(txn_id.clone(), value_at_txn_id);
            }

            Ok(Some(TxnLockReadGuard {
                txn_id: txn_id.clone(),
                lock: self.clone(),
            }))
        }
    }

    pub fn read<'a>(&self, txn_id: &'a TxnId) -> TxnLockReadFuture<'a, T> {
        TxnLockReadFuture {
            txn_id,
            lock: self.clone(),
        }
    }

    pub fn try_write<'a>(&self, txn_id: &'a TxnId) -> TCResult<Option<TxnLockWriteGuard<T>>> {
        let lock = &mut self.inner.lock().unwrap();
        let latest_reader = lock.state.readers.keys().max();

        if latest_reader.is_some() && latest_reader.unwrap() > txn_id {
            // If there's already a reader in the future, there's no point in waiting.
            return Err(error::conflict());
        }

        match &lock.state.reserved {
            // If there's already a writer in the future, there's no point in waiting.
            Some(current_txn) if current_txn > txn_id => Err(error::conflict()),
            // If there's a writer in the past, wait for it to complete.
            Some(current_txn) if current_txn < txn_id => Ok(None),
            // If there's already a writer for the current transaction, wait for it to complete.
            Some(_) if lock.state.writer => Ok(None),
            _ => {
                // Otherwise, copy the value to be mutated in this transaction.
                lock.state.writer = true;
                lock.state.reserved = Some(txn_id.clone());
                if !lock.value_at.contains_key(txn_id) {
                    let mutation = UnsafeCell::new(unsafe { (&*lock.value.get()).clone() });
                    lock.value_at.insert(txn_id.clone(), mutation);
                }

                Ok(Some(TxnLockWriteGuard {
                    txn_id: txn_id.clone(),
                    lock: self.clone(),
                }))
            }
        }
    }

    pub fn write<'a>(&self, txn_id: &'a TxnId) -> TxnLockWriteFuture<'a, T> {
        TxnLockWriteFuture {
            txn_id,
            lock: self.clone(),
        }
    }
}

#[async_trait]
impl<T: Mutable> Transact for TxnLock<T> {
    async fn commit(&self, txn_id: &TxnId) {
        async {
            let _ = self.write(txn_id).await; // prevent any more writes
            let lock = &mut self.inner.lock().unwrap();
            lock.state.last_commit = txn_id.clone();
            lock.state.reserved = None;

            let value = unsafe { &mut *lock.value.get() };
            if let Some(new_value) = lock.value_at.remove(txn_id) {
                value.commit(txn_id, new_value.into_inner())
            } else {
                Box::pin(future::ready(()))
            }
        }
        .await;
    }

    async fn rollback(&self, txn_id: &TxnId) {
        let _ = self.write(txn_id).await; // prevent any more writes
        let lock = &mut self.inner.lock().unwrap();
        lock.value_at.remove(txn_id);
    }
}

pub struct TxnLockReadFuture<'a, T: Mutable> {
    txn_id: &'a TxnId,
    lock: TxnLock<T>,
}

impl<'a, T: Mutable> Future for TxnLockReadFuture<'a, T> {
    type Output = TCResult<TxnLockReadGuard<T>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        match self.lock.try_read(self.txn_id) {
            Ok(Some(guard)) => Poll::Ready(Ok(guard)),
            Err(cause) => Poll::Ready(Err(cause)),
            Ok(None) => {
                self.lock
                    .inner
                    .lock()
                    .unwrap()
                    .state
                    .wakers
                    .push_back(context.waker().clone());

                Poll::Pending
            }
        }
    }
}

pub struct TxnLockWriteFuture<'a, T: Mutable> {
    txn_id: &'a TxnId,
    lock: TxnLock<T>,
}

impl<'a, T: Mutable> Future for TxnLockWriteFuture<'a, T> {
    type Output = TCResult<TxnLockWriteGuard<T>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        match self.lock.try_write(self.txn_id) {
            Ok(Some(guard)) => Poll::Ready(Ok(guard)),
            Err(cause) => Poll::Ready(Err(cause)),
            Ok(None) => {
                self.lock
                    .inner
                    .lock()
                    .unwrap()
                    .state
                    .wakers
                    .push_back(context.waker().clone());

                Poll::Pending
            }
        }
    }
}
