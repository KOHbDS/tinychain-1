use std::convert::TryFrom;

use afarray::Array;
use futures::{TryFutureExt};
use log::debug;
use safecast::{Match, TryCastFrom, TryCastInto};

use tc_error::*;
use tc_tensor::{
    AxisBounds, Bounds, Coord, DenseAccess, DenseTensor, TensorAccess, TensorDualIO, TensorIO, TensorMath,
    TensorTransform, TensorType,
};
use tc_transact::fs::Dir;
use tc_transact::Transaction;
use tcgeneric::{label, PathSegment};

use crate::collection::{Collection, Tensor};
use crate::fs;
use crate::route::{GetHandler, PostHandler, PutHandler};
use crate::scalar::{Bound, Number, NumberClass, NumberType, Range, Scalar, Value, ValueType};
use crate::state::State;
use crate::txn::Txn;

use super::{Handler, Route};

struct ConstantHandler;

impl<'a> Handler<'a> for ConstantHandler {
    fn get(self: Box<Self>) -> Option<GetHandler<'a>> {
        Some(Box::new(|txn, key| {
            Box::pin(async move {
                if key.matches::<(Vec<u64>, Number)>() {
                    let (shape, value): (Vec<u64>, Number) = key.opt_cast_into().unwrap();
                    constant(&txn, shape, value).await
                } else {
                    Err(TCError::bad_request("invalid tensor schema", key))
                }
            })
        }))
    }
}

struct CreateHandler {
    class: TensorType,
}

impl<'a> Handler<'a> for CreateHandler {
    fn get(self: Box<Self>) -> Option<GetHandler<'a>> {
        Some(Box::new(|txn, key| {
            Box::pin(async move {
                if key.matches::<(Vec<u64>, ValueType)>() {
                    let (shape, dtype): (Vec<u64>, ValueType) = key.opt_cast_into().unwrap();
                    let dtype = NumberType::try_from(dtype)?;

                    match self.class {
                        TensorType::Dense => constant(&txn, shape.into(), dtype.zero()).await,
                    }
                } else {
                    Err(TCError::bad_request(
                        "invalid schema for constant tensor",
                        key,
                    ))
                }
            })
        }))
    }
}

struct RangeHandler;

impl<'a> Handler<'a> for RangeHandler {
    fn get(self: Box<Self>) -> Option<GetHandler<'a>> {
        Some(Box::new(|txn, key| {
            Box::pin(async move {
                if key.matches::<(Vec<u64>, Number, Number)>() {
                    let (shape, start, stop): (Vec<u64>, Number, Number) =
                        key.opt_cast_into().unwrap();

                    let file = create_file(&txn).await?;

                    DenseTensor::range(file, *txn.id(), shape, start, stop)
                        .map_ok(Tensor::from)
                        .map_ok(Collection::from)
                        .map_ok(State::from)
                        .await
                } else {
                    Err(TCError::bad_request("invalid schema for range tensor", key))
                }
            })
        }))
    }
}

impl Route for TensorType {
    fn route<'a>(&'a self, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>> {
        if path.is_empty() {
            Some(Box::new(CreateHandler { class: *self }))
        } else if path.len() == 1 && self == &Self::Dense {
            match path[0].as_str() {
                "constant" => Some(Box::new(ConstantHandler)),
                "range" => Some(Box::new(RangeHandler)),
                _ => None,
            }
        } else {
            None
        }
    }
}

struct MathHandler<'a, T> {
    tensor: &'a T,
    op: fn(&'a T, &Tensor) -> TCResult<Tensor>,
}

impl<'a, T> MathHandler<'a, T> {
    fn new(tensor: &'a T, op: fn(&'a T, &Tensor) -> TCResult<Tensor>) -> Self {
        Self { tensor, op }
    }
}

impl<'a, T> Handler<'a> for MathHandler<'a, T>
where
    T: TensorMath<fs::Dir, Tensor, Combine = Tensor> + Send + Sync + 'a,
{
    fn post(self: Box<Self>) -> Option<PostHandler<'a>> {
        Some(Box::new(|_txn, mut params| {
            Box::pin(async move {
                let r = params.require::<Tensor>(&label("r").into())?;
                let r = if r.shape() == self.tensor.shape() {
                    r
                } else {
                    r.broadcast(self.tensor.shape().clone())?
                };

                (self.op)(self.tensor, &r)
                    .map(Collection::from)
                    .map(State::from)
            })
        }))
    }
}

struct TensorHandler<'a, T> {
    tensor: &'a T,
}

impl<'a, T> Handler<'a> for TensorHandler<'a, T>
where
    T: TensorIO<fs::Dir, Txn = Txn>
        + TensorDualIO<fs::Dir, Tensor>
        + TensorTransform<fs::Dir>
        + Clone
        + Send
        + Sync,
    Collection: From<T>,
    Collection: From<<T as TensorTransform<fs::Dir>>::Slice>,
{
    fn get(self: Box<Self>) -> Option<GetHandler<'a>> {
        Some(Box::new(|txn, key| {
            Box::pin(async move {
                if key.is_none() {
                    Ok(Collection::from(self.tensor.clone()).into())
                } else if key.matches::<Coord>() {
                    let coord = key.opt_cast_into().unwrap();
                    self.tensor
                        .read_value(&txn, coord)
                        .map_ok(Value::from)
                        .map_ok(State::from)
                        .await
                } else if key.matches::<Bounds>() {
                    let bounds = key.opt_cast_into().unwrap();
                    self.tensor
                        .slice(bounds)
                        .map(Collection::from)
                        .map(State::from)
                } else {
                    Err(TCError::bad_request("invalid tensor bounds", key))
                }
            })
        }))
    }

    fn put(self: Box<Self>) -> Option<PutHandler<'a>> {
        Some(Box::new(move |txn, key, value| {
            Box::pin(write(self.tensor, txn, key.into(), value))
        }))
    }

    fn post(self: Box<Self>) -> Option<PostHandler<'a>> {
        Some(Box::new(|_txn, mut params| {
            Box::pin(async move {
                let bounds: Scalar = params.or_default(&label("bounds").into())?;
                let bounds = cast_bounds(self.tensor.shape(), bounds)?;
                self.tensor
                    .slice(bounds)
                    .map(Collection::from)
                    .map(State::from)
            })
        }))
    }
}

impl<'a, T> From<&'a T> for TensorHandler<'a, T> {
    fn from(tensor: &'a T) -> Self {
        Self { tensor }
    }
}

impl<B: DenseAccess<fs::File<Array>, fs::Dir, Txn>> Route
    for DenseTensor<fs::File<Array>, fs::Dir, Txn, B>
{
    fn route<'a>(&'a self, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>> {
        route(self, path)
    }
}

impl Route for Tensor {
    fn route<'a>(&'a self, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>> {
        route(self, path)
    }
}

fn route<'a, T>(tensor: &'a T, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>>
where
    T: TensorIO<fs::Dir, Txn = Txn>
        + TensorDualIO<fs::Dir, Tensor>
        + TensorMath<fs::Dir, Tensor, Combine = Tensor>
        + TensorTransform<fs::Dir>
        + Clone
        + Send
        + Sync,
    Collection: From<T>,
    Collection: From<<T as TensorTransform<fs::Dir>>::Slice>,
{
    if path.is_empty() {
        Some(Box::new(TensorHandler::from(tensor)))
    } else if path.len() == 1 {
        match path[0].as_str() {
            "add" => Some(Box::new(MathHandler::new(tensor, TensorMath::add))),
            "div" => Some(Box::new(MathHandler::new(tensor, TensorMath::div))),
            "mul" => Some(Box::new(MathHandler::new(tensor, TensorMath::mul))),
            "sub" => Some(Box::new(MathHandler::new(tensor, TensorMath::sub))),
            _ => None,
        }
    } else {
        None
    }
}

async fn constant(txn: &Txn, shape: Vec<u64>, value: Number) -> TCResult<State> {
    let file = create_file(txn).await?;

    DenseTensor::constant(file, *txn.id(), shape, value)
        .map_ok(Tensor::from)
        .map_ok(Collection::from)
        .map_ok(State::from)
        .await
}

async fn write<T: TensorIO<fs::Dir, Txn = Txn> + TensorDualIO<fs::Dir, Tensor>>(
    tensor: &T,
    txn: Txn,
    key: Scalar,
    value: State,
) -> TCResult<()> {
    if key.matches::<Coord>() {
        let value =
            Value::try_cast_from(value, |v| TCError::bad_request("invalid tensor element", v))?;
        let value = value.try_cast_into(|v| TCError::bad_request("invalid tensor element", v))?;
        let coord = key.opt_cast_into().unwrap();
        return tensor.write_value_at(*txn.id(), coord, value).await;
    }

    let bounds = if key.is_none() {
        Bounds::all(tensor.shape())
    } else {
        let bounds =
            Value::try_cast_from(key, |k| TCError::bad_request("invalid tensor bounds", k))?;

        bounds.try_cast_into(|k| TCError::bad_request("invalid tensor bounds", k))?
    };

    match value {
        State::Collection(Collection::Tensor(value)) => tensor.write(txn, bounds, value).await,
        State::Scalar(scalar) => {
            let value =
                scalar.try_cast_into(|v| TCError::bad_request("invalid tensor element", v))?;

            tensor.write_value(*txn.id(), bounds, value).await
        }
        other => Err(TCError::bad_request(
            "cannot write this value to tensor",
            other,
        )),
    }
}

async fn create_file(txn: &Txn) -> TCResult<fs::File<afarray::Array>> {
    txn.context()
        .create_file_tmp(*txn.id(), TensorType::Dense)
        .await
}

fn cast_bound(dim: u64, bound: Value) -> TCResult<u64> {
    let bound = i64::try_cast_from(bound, |v| TCError::bad_request("invalid bound", v))?;
    if bound.abs() as u64 > dim {
        return Err(TCError::bad_request(
            format!("Index out of bounds for dimension {}", dim),
            bound,
        ));
    }

    if bound < 0 {
        Ok(dim - bound.abs() as u64)
    } else {
        Ok(bound as u64)
    }
}

pub fn cast_bounds(shape: &[u64], scalar: Scalar) -> TCResult<Bounds> {
    debug!("tensor bounds from {}", scalar);

    match scalar {
        Scalar::Tuple(bounds) => {
            let mut axes = Vec::with_capacity(shape.len());

            for (axis, bound) in bounds.into_inner().into_iter().enumerate() {
                let bound = if bound.matches::<Range>() {
                    let range = Range::opt_cast_from(bound).unwrap();
                    let start = match range.start {
                        Bound::Un => 0,
                        Bound::In(start) => cast_bound(shape[axis], start)?,
                        Bound::Ex(start) => cast_bound(shape[1], start)? + 1,
                    };

                    let end = match range.end {
                        Bound::Un => shape[axis],
                        Bound::In(end) => cast_bound(shape[axis], end)?,
                        Bound::Ex(end) => cast_bound(shape[1], end)?,
                    };

                    AxisBounds::In(start..end)
                } else if bound.matches::<Vec<u64>>() {
                    bound.opt_cast_into().map(AxisBounds::Of).unwrap()
                } else if let Scalar::Value(value) = bound {
                    cast_bound(shape[axis], value).map(AxisBounds::At)?
                } else {
                    return Err(TCError::bad_request(
                        format!("invalid bound for axis {}", axis),
                        bound,
                    ));
                };

                axes.push(bound);
            }

            Ok(Bounds { axes })
        }
        Scalar::Value(Value::Tuple(bounds)) => {
            let mut axes = Vec::with_capacity(shape.len());
            for (axis, bound) in bounds.into_inner().into_iter().enumerate() {
                let bound = match bound {
                    Value::Tuple(indices) => {
                        let indices = shape[..]
                            .iter()
                            .zip(indices.into_inner().into_iter())
                            .map(|(dim, i)| cast_bound(*dim, i.into()))
                            .collect::<TCResult<Vec<u64>>>()?;

                        AxisBounds::Of(indices)
                    }
                    value => {
                        let i = cast_bound(shape[axis], value)?;
                        AxisBounds::At(i)
                    }
                };

                axes.push(bound);
            }

            Ok(Bounds { axes })
        }
        other => Err(TCError::bad_request("invalid tensor bounds", other)),
    }
}
