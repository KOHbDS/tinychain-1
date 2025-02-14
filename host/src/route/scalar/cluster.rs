use tcgeneric::{PathSegment, TCPathBuf};

use crate::route::{DeleteHandler, GetHandler, Handler, PostHandler, PutHandler, Route};
use crate::scalar::ClusterRef;

struct ClusterHandler {
    path: TCPathBuf,
}

impl ClusterHandler {
    fn new(cluster: &ClusterRef, path: &[PathSegment]) -> Self {
        let mut cluster_path = cluster.path().clone();
        cluster_path.extend_from_slice(path);
        Self { path: cluster_path }
    }
}

impl<'a> Handler<'a> for ClusterHandler {
    fn get<'b>(self: Box<Self>) -> Option<GetHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|txn, key| {
            Box::pin(txn.get(self.path.into(), key))
        }))
    }

    fn put<'b>(self: Box<Self>) -> Option<PutHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|txn, key, value| {
            Box::pin(txn.put(self.path.into(), key, value))
        }))
    }

    fn post<'b>(self: Box<Self>) -> Option<PostHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|txn, params| {
            Box::pin(txn.post(self.path.into(), params.into()))
        }))
    }

    fn delete<'b>(self: Box<Self>) -> Option<DeleteHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|txn, key| {
            Box::pin(txn.delete(self.path.into(), key))
        }))
    }
}

impl Route for ClusterRef {
    fn route<'a>(&'a self, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>> {
        Some(Box::new(ClusterHandler::new(self, path)))
    }
}
