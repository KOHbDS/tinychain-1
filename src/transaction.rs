use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use rand::Rng;

use crate::cache::{Map, Set, Value};
use crate::context::{TCContext, TCResult, TCState, TCValue};
use crate::error;
use crate::host::HostContext;

pub type Pending = (
    Vec<String>,
    Arc<dyn FnOnce(HashMap<String, TCState>) -> TCResult<Arc<TCState>> + Send + Sync>,
);

#[derive(Clone)]
pub struct TransactionId {
    timestamp: u128, // nanoseconds since Unix epoch
    nonce: u16,
}

impl TransactionId {
    fn new(timestamp: u128) -> TransactionId {
        let nonce: u16 = rand::thread_rng().gen();
        TransactionId { timestamp, nonce }
    }
}

pub struct Transaction {
    id: TransactionId,
    host: Arc<HostContext>,
    known: Set<String>,
    queue: RwLock<Vec<(String, Pending)>>,
    resolved: Map<String, TCState>,
    state: Value<State>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Open,
    Closed,
    Resolved,
}

impl Transaction {
    fn of(id: TransactionId, host: Arc<HostContext>) -> Arc<Transaction> {
        Arc::new(Transaction {
            id,
            host,
            known: Set::new(),
            queue: RwLock::new(vec![]),
            resolved: Map::new(),
            state: Value::of(State::Open),
        })
    }

    pub fn new(host: Arc<HostContext>) -> Arc<Transaction> {
        Self::of(TransactionId::new(host.time()), host)
    }

    pub fn include(
        self: Arc<Self>,
        name: String,
        context: String,
        args: HashMap<String, TCValue>,
    ) -> TCResult<()> {
        if self.state.get() != State::Open {
            return Err(error::internal(
                "Attempted to extend a transaction already in progress",
            ));
        }

        let txn = Self::of(self.id.clone(), self.host.clone());
        for (name, arg) in args {
            txn.clone().provide(name, arg)?;
        }

        let pending = self.host.clone().post(context)?;
        self.queue.write().unwrap().push((name.clone(), pending));
        self.known.insert(name);

        Ok(())
    }

    pub fn provide(self: Arc<Self>, name: String, value: TCValue) -> TCResult<()> {
        if self.state.get() != State::Open {
            return Err(error::internal(
                "Attempted to provide a value to a transaction already in progress",
            ));
        }

        if self.known.contains(&name) {
            Err(error::bad_request(
                "This transaction already contains a value called",
                name,
            ))
        } else {
            self.resolved.insert(name, Arc::new(TCState::Value(value)));
            Ok(())
        }
    }

    pub async fn resolve(&self, capture: Vec<&str>) -> TCResult<HashMap<String, TCValue>> {
        if self.state.get() != State::Open {
            return Err(error::internal(
                "Attempt to resolve the same transaction multiple times",
            ));
        }

        self.state.set(State::Closed);

        // TODO: resolve all child transactions

        self.state.set(State::Resolved);

        let mut results: HashMap<String, TCValue> = HashMap::new();
        for name in capture {
            let name = name.to_string();
            match self.resolved.get(&name) {
                Some(arc_ref) => match &*arc_ref {
                    TCState::Value(val) => {
                        results.insert(name, val.clone());
                    },
                    TCState::Table(_) => {
                        return Err(error::bad_request("The transaction completed successfully but some captured values could not be serialized", name))
                    }
                },
                None => {
                    return Err(error::bad_request(
                        "Attempted to read value not in transaction",
                        name,
                    ));
                }
            }
        }

        Ok(results)
    }
}
