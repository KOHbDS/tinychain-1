use safecast::{CastFrom, TryCastInto};

use tc_error::*;
use tc_value::{Float, Number, NumberClass, NumberInstance, Trigonometry, Value};
use tcgeneric::{label, PathSegment};

use crate::route::{GetHandler, Handler, PostHandler, Route};
use crate::state::State;

struct Dual<F> {
    op: F,
}

impl<F> Dual<F> {
    fn new(op: F) -> Self {
        Self { op }
    }
}

impl<'a, F> Handler<'a> for Dual<F>
where
    F: Fn(Number) -> TCResult<Number> + Send + 'a,
{
    fn get<'b>(self: Box<Self>) -> Option<GetHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|_txn, value| {
            Box::pin(async move {
                let value = value.try_cast_into(|v| TCError::bad_request("not a Number", v))?;
                (self.op)(value).map(Value::Number).map(State::from)
            })
        }))
    }

    fn post<'b>(self: Box<Self>) -> Option<PostHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|_txn, mut params| {
            Box::pin(async move {
                let value: Number = params.require(&label("r").into())?;
                params.expect_empty()?;

                (self.op)(value).map(Value::Number).map(State::from)
            })
        }))
    }
}

// TODO: should this be more general, like `DualWithDefaultArg`?
struct Log {
    n: Number,
}

impl Log {
    fn new(n: Number) -> Self {
        Self { n }
    }
}

impl<'a> Handler<'a> for Log {
    fn get<'b>(self: Box<Self>) -> Option<GetHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|_txn, value| {
            Box::pin(async move {
                if self.n == Number::from(0) {
                    return Err(TCError::unsupported("the logarithm of zero is undefined"));
                }

                let log = if value.is_none() {
                    Ok(self.n.ln())
                } else {
                    let base: Number =
                        value.try_cast_into(|v| TCError::bad_request("not a Number", v))?;
                    if base.class().is_complex() {
                        Err(TCError::bad_request("invalid base for log", base))
                    } else {
                        let base = Float::cast_from(base);
                        Ok(self.n.log(base))
                    }
                }?;

                Ok(Value::Number(log).into())
            })
        }))
    }

    fn post<'b>(self: Box<Self>) -> Option<PostHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|_txn, mut params| {
            Box::pin(async move {
                let base: Value = params.or_default(&label("r").into())?;
                params.expect_empty()?;

                let log = if base.is_none() {
                    self.n.ln()
                } else {
                    let base: Number =
                        base.try_cast_into(|v| TCError::bad_request("invalid base for log", v))?;
                    if base.class().is_complex() {
                        return Err(TCError::bad_request(
                            "log does not support a complex base",
                            base,
                        ));
                    }

                    let base = Float::cast_from(base);
                    self.n.log(base)
                };

                Ok(Value::Number(log).into())
            })
        }))
    }
}

struct Unary<F> {
    name: &'static str,
    op: F,
}

impl<F> Unary<F> {
    fn new(name: &'static str, op: F) -> Self {
        Self { name, op }
    }
}

impl<'a, F> Handler<'a> for Unary<F>
where
    F: Fn() -> Number + Send + 'a,
{
    fn get<'b>(self: Box<Self>) -> Option<GetHandler<'a, 'b>>
    where
        'b: 'a,
    {
        Some(Box::new(|_txn, value| {
            Box::pin(async move {
                if value.is_some() {
                    return Err(TCError::unsupported(format!(
                        "{} does not have any parameters (found {})",
                        self.name, value
                    )));
                }

                Ok(State::from(Value::from((self.op)())))
            })
        }))
    }
}

impl Route for Number {
    fn route<'a>(&'a self, path: &'a [PathSegment]) -> Option<Box<dyn Handler<'a> + 'a>> {
        if path.len() != 1 {
            return None;
        }

        let handler: Box<dyn Handler<'a> + 'a> = match path[0].as_str() {
            // basic math
            "abs" => Box::new(Unary::new("abs", move || self.abs())),
            "add" => Box::new(Dual::new(move |other| Ok(*self + other))),
            "and" => Box::new(Dual::new(move |other| Ok(self.and(other)))),
            "div" => Box::new(Dual::new(move |other: Number| {
                if other == other.class().zero() {
                    Err(TCError::unsupported("cannot divide by zero"))
                } else {
                    Ok(*self / other)
                }
            })),
            "ln" => Box::new(Unary::new("ln", move || self.ln())),
            "log" => Box::new(Log::new(*self)),
            "mod" => Box::new(Dual::new(move |other| Ok(*self % other))),
            "mul" => Box::new(Dual::new(move |other| Ok(*self * other))),
            "round" => Box::new(Unary::new("round", move || self.round())),
            "sub" => Box::new(Dual::new(move |other| Ok(*self - other))),
            "pow" => Box::new(Dual::new(move |other| Ok(self.pow(other)))),

            // comparison
            "gt" => Box::new(Dual::new(move |other| Ok((*self > other).into()))),
            "gte" => Box::new(Dual::new(move |other| Ok((*self >= other).into()))),
            "lt" => Box::new(Dual::new(move |other| Ok((*self < other).into()))),
            "lte" => Box::new(Dual::new(move |other| Ok((*self <= other).into()))),
            "not" => Box::new(Unary::new("not", move || self.not())),
            "or" => Box::new(Dual::new(move |other| Ok(self.or(other)))),
            "xor" => Box::new(Dual::new(move |other| Ok(self.xor(other)))),

            // trigonometry
            "asin" => Box::new(Unary::new("abs", move || self.asin())),
            "sin" => Box::new(Unary::new("sin", move || self.sin())),
            "asinh" => Box::new(Unary::new("asinh", move || self.asinh())),
            "sinh" => Box::new(Unary::new("sinh", move || self.sinh())),

            "acos" => Box::new(Unary::new("acos", move || self.acos())),
            "cos" => Box::new(Unary::new("cos", move || self.cos())),
            "acosh" => Box::new(Unary::new("acosh", move || self.acosh())),
            "cosh" => Box::new(Unary::new("cosh", move || self.cosh())),

            "atan" => Box::new(Unary::new("atan", move || self.atan())),
            "tan" => Box::new(Unary::new("tan", move || self.tan())),
            "atanh" => Box::new(Unary::new("atanh", move || self.atanh())),
            "tanh" => Box::new(Unary::new("tanh", move || self.tanh())),

            _ => return None,
        };

        Some(handler)
    }
}
