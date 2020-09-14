use std::convert::{TryFrom, TryInto};
use std::fmt;

use crate::class::{Class, Instance, TCResult};
use crate::error;

use super::class::{ValueClass, ValueInstance};
use super::link::{Link, TCPath};
use super::{label, TCRef, Value, ValueId, ValueType};

#[derive(Clone, Eq, PartialEq)]
pub enum OpDefType {
    Get,
    Put,
    Post,
}

impl Class for OpDefType {
    type Instance = OpDef;

    fn from_path(path: &TCPath) -> TCResult<Self> {
        let suffix = path.from_path(&Self::prefix())?;
        if suffix.len() == 1 {
            match suffix[0].as_str() {
                "get" => Ok(OpDefType::Get),
                "put" => Ok(OpDefType::Put),
                "post" => Ok(OpDefType::Post),
                other => Err(error::not_found(other)),
            }
        } else {
            Err(error::not_found(suffix))
        }
    }

    fn prefix() -> TCPath {
        OpType::prefix().join(label("def").into())
    }
}

impl From<OpDefType> for Link {
    fn from(odt: OpDefType) -> Link {
        let prefix = OpDefType::prefix();
        match odt {
            OpDefType::Get => prefix.join(label("get").into()).into(),
            OpDefType::Put => prefix.join(label("put").into()).into(),
            OpDefType::Post => prefix.join(label("post").into()).into(),
        }
    }
}

impl fmt::Display for OpDefType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Get => write!(f, "type: GET Op definition"),
            Self::Put => write!(f, "type: PUT Op definition"),
            Self::Post => write!(f, "type: POST Op definition"),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum MethodType {
    Get,
    Put,
    Post,
}

impl Class for MethodType {
    type Instance = Method;

    fn from_path(path: &TCPath) -> TCResult<Self> {
        let suffix = path.from_path(&Self::prefix())?;
        if suffix.len() == 1 {
            match suffix[0].as_str() {
                "get" => Ok(MethodType::Get),
                "put" => Ok(MethodType::Put),
                "post" => Ok(MethodType::Post),
                other => Err(error::not_found(other)),
            }
        } else {
            Err(error::not_found(suffix))
        }
    }

    fn prefix() -> TCPath {
        OpType::prefix().join(label("method").into())
    }
}

impl From<MethodType> for Link {
    fn from(mt: MethodType) -> Link {
        let prefix = MethodType::prefix();
        match mt {
            MethodType::Get => prefix.join(label("get").into()).into(),
            MethodType::Put => prefix.join(label("put").into()).into(),
            MethodType::Post => prefix.join(label("post").into()).into(),
        }
    }
}

impl fmt::Display for MethodType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Get => write!(f, "type: GET method"),
            Self::Put => write!(f, "type: PUT method"),
            Self::Post => write!(f, "type: POST method"),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum OpRefType {
    Get,
    Put,
    Post,
}

impl Class for OpRefType {
    type Instance = OpRef;

    fn from_path(path: &TCPath) -> TCResult<Self> {
        let suffix = path.from_path(&Self::prefix())?;
        if suffix.len() == 1 {
            match suffix[0].as_str() {
                "get" => Ok(OpRefType::Get),
                "put" => Ok(OpRefType::Put),
                "post" => Ok(OpRefType::Post),
                other => Err(error::not_found(other)),
            }
        } else {
            Err(error::not_found(suffix))
        }
    }

    fn prefix() -> TCPath {
        OpType::prefix().join(label("ref").into())
    }
}

impl From<OpRefType> for Link {
    fn from(ort: OpRefType) -> Link {
        let prefix = OpRefType::prefix();
        match ort {
            OpRefType::Get => prefix.join(label("get").into()).into(),
            OpRefType::Put => prefix.join(label("put").into()).into(),
            OpRefType::Post => prefix.join(label("post").into()).into(),
        }
    }
}

impl fmt::Display for OpRefType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Get => write!(f, "type: GET Op ref"),
            Self::Put => write!(f, "type: PUT Op ref"),
            Self::Post => write!(f, "type: POST Op ref"),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum OpType {
    Def(OpDefType),
    If,
    Method(MethodType),
    Ref(OpRefType),
}

impl Class for OpType {
    type Instance = Op;

    fn from_path(path: &TCPath) -> TCResult<Self> {
        let suffix = path.from_path(&Self::prefix())?;

        if suffix.is_empty() {
            Err(error::unsupported("You must specify a type of Op"))
        } else {
            match suffix[0].as_str() {
                "def" => OpDefType::from_path(path).map(OpType::Def),
                "if" if suffix.len() == 1 => Ok(OpType::If),
                "method" => MethodType::from_path(path).map(OpType::Method),
                "ref" => OpRefType::from_path(path).map(OpType::Ref),
                other => Err(error::not_found(other)),
            }
        }
    }

    fn prefix() -> TCPath {
        ValueType::prefix().join(label("op").into())
    }
}

impl ValueClass for OpType {
    type Instance = Op;

    fn get(
        _path: &TCPath,
        _value: <Self as ValueClass>::Instance,
    ) -> TCResult<<Self as ValueClass>::Instance> {
        Err(error::unsupported("Op does not support casting"))
    }

    fn size(self) -> Option<usize> {
        None
    }
}

impl From<OpType> for Link {
    fn from(ot: OpType) -> Link {
        let prefix = OpType::prefix();
        match ot {
            OpType::Def(odt) => odt.into(),
            OpType::If => prefix.join(label("if").into()).into(),
            OpType::Method(mt) => mt.into(),
            OpType::Ref(ort) => ort.into(),
        }
    }
}

impl fmt::Display for OpType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Def(odt) => write!(f, "{}", odt),
            Self::If => write!(f, "type: Conditional Op"),
            Self::Method(mt) => write!(f, "{}", mt),
            Self::Ref(ort) => write!(f, "{}", ort),
        }
    }
}

pub type Cond = (TCRef, Value, Value);
pub type GetOp = (TCRef, Vec<(ValueId, Value)>);
pub type PutOp = (TCRef, TCRef, Vec<(ValueId, Value)>);
pub type PostOp = (Vec<TCRef>, Vec<(ValueId, Value)>);

#[derive(Clone, Eq, PartialEq)]
pub enum OpDef {
    Get(GetOp),
    Put(PutOp),
    Post(PostOp),
}

impl Instance for OpDef {
    type Class = OpDefType;

    fn class(&self) -> OpDefType {
        match self {
            Self::Get(_) => OpDefType::Get,
            Self::Put(_) => OpDefType::Put,
            Self::Post(_) => OpDefType::Post,
        }
    }
}

impl TryFrom<Value> for OpDef {
    type Error = error::TCError;

    fn try_from(value: Value) -> TCResult<OpDef> {
        if let Ok(get_op) = value.clone().try_into() {
            Ok(OpDef::Get(get_op))
        } else {
            Err(error::bad_request("Expected OpDef but found", value))
        }
    }
}

impl fmt::Display for OpDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Get(_) => write!(f, "GET Op"),
            Self::Put(_) => write!(f, "PUT Op"),
            Self::Post(_) => write!(f, "POST"),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum Method {
    Get(TCRef, TCPath, Value),
    Put(TCRef, TCPath, Value, Value),
    Post(TCRef, TCPath, Vec<Value>),
}

impl Instance for Method {
    type Class = MethodType;

    fn class(&self) -> MethodType {
        match self {
            Self::Get(_, _, _) => MethodType::Get,
            Self::Put(_, _, _, _) => MethodType::Put,
            Self::Post(_, _, _) => MethodType::Post,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Get(subject, path, _) => write!(f, "GET {}{}", subject, path),
            Self::Put(subject, path, _, _) => write!(f, "PUT {}{}", subject, path),
            Self::Post(subject, path, _) => write!(f, "PUT {}{}", subject, path),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum OpRef {
    Get(Link, Value),
    Put(Link, Value, Value),
    Post(Link, Vec<Value>),
}

impl Instance for OpRef {
    type Class = OpRefType;

    fn class(&self) -> OpRefType {
        match self {
            Self::Get(_, _) => OpRefType::Get,
            Self::Put(_, _, _) => OpRefType::Put,
            Self::Post(_, _) => OpRefType::Post,
        }
    }
}

impl fmt::Display for OpRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OpRef::Get(link, id) => write!(f, "OpRef::Get {}: {}", link, id),
            OpRef::Put(path, id, val) => write!(f, "OpRef::Put {}: {} <- {}", path, id, val),
            OpRef::Post(path, data) => {
                write!(f, "OpRef::Post {}({})", path, Value::Tuple(data.to_vec()))
            }
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum Op {
    Def(OpDef),
    If(Cond),
    Method(Method),
    Ref(OpRef),
}

impl Instance for Op {
    type Class = OpType;

    fn class(&self) -> OpType {
        match self {
            Self::Def(op_def) => OpType::Def(op_def.class()),
            Self::If(_) => OpType::If,
            Self::Method(method) => OpType::Method(method.class()),
            Self::Ref(op_ref) => OpType::Ref(op_ref.class()),
        }
    }
}

impl ValueInstance for Op {
    type Class = OpType;
}

impl Default for Op {
    fn default() -> Op {
        Op::Ref(OpRef::Get(Link::default(), Value::default()))
    }
}

impl From<Method> for Op {
    fn from(method: Method) -> Op {
        Op::Method(method)
    }
}

impl From<OpRef> for Op {
    fn from(op_ref: OpRef) -> Op {
        Op::Ref(op_ref)
    }
}

impl TryFrom<Op> for GetOp {
    type Error = error::TCError;

    fn try_from(op: Op) -> TCResult<GetOp> {
        match op {
            Op::Def(OpDef::Get(get_op)) => Ok(get_op),
            other => Err(error::bad_request("Expected GetOp but found", other)),
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Op::Def(op_def) => write!(f, "{}", op_def),
            Op::If((cond, then, or_else)) => write!(
                f,
                "Op::If({} then {{ {} }} else {{ {} }})",
                cond, then, or_else
            ),
            Op::Method(method) => write!(f, "{}", method),
            Op::Ref(op_ref) => write!(f, "{}", op_ref),
        }
    }
}
