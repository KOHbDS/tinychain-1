use std::collections::HashMap;
use std::iter;

use num::Integer;

use crate::error;
use crate::value::TCResult;

use super::bounds::{AxisBounds, Bounds, Shape};

#[derive(Clone)]
pub struct Broadcast {
    source_shape: Shape,
    shape: Shape,
    broadcast: Vec<bool>,
    offset: usize,
    inverted_axes: Vec<usize>,
}

impl Broadcast {
    pub fn new(source_shape: Shape, shape: Shape) -> TCResult<Broadcast> {
        let ndim = shape.len();
        if source_shape.len() > shape.len() {
            return Err(error::bad_request(
                &format!("Cannot broadcast into {}", shape),
                source_shape,
            ));
        }

        let offset = ndim - source_shape.len();
        let mut inverted_axes = Vec::with_capacity(shape.len());
        let mut broadcast: Vec<bool> = iter::repeat(true).take(ndim).collect();

        for axis in offset..ndim {
            if shape[axis] == source_shape[axis - offset] {
                broadcast[axis] = false;
                inverted_axes.push(axis);
            } else if shape[axis] == 1 || source_shape[axis - offset] == 1 {
                inverted_axes.push(axis - offset);
            } else {
                return Err(error::bad_request(
                    &format!("Cannot broadcast into {}", shape),
                    source_shape,
                ));
            }
        }

        Ok(Broadcast {
            source_shape,
            shape,
            broadcast,
            offset,
            inverted_axes,
        })
    }

    pub fn invert_axes(&self, axes: Vec<usize>) -> Vec<usize> {
        let mut inverted: Vec<usize> = axes.iter().map(|x| self.inverted_axes[*x]).collect();
        inverted.dedup();
        inverted
    }

    pub fn invert_bounds(&self, bounds: Bounds) -> Bounds {
        let source_ndim = self.source_shape.len();
        let mut source_bounds = Vec::with_capacity(source_ndim);
        for axis in 0..source_ndim {
            if self.broadcast[axis + self.offset] {
                source_bounds.push(AxisBounds::from(0))
            } else {
                source_bounds.push(bounds[axis + self.offset].clone())
            }
        }

        source_bounds.into()
    }

    pub fn invert_coord(&self, coord: &[u64]) -> Vec<u64> {
        let source_ndim = self.source_shape.len();
        let mut source_coord = Vec::with_capacity(source_ndim);
        for axis in 0..source_ndim {
            if self.broadcast[axis + self.offset] {
                source_coord.push(0);
            } else {
                source_coord.push(coord[axis + self.offset]);
            }
        }

        source_coord
    }

    pub fn map_bounds(&self, source_bounds: Bounds) -> Bounds {
        assert!(source_bounds.len() == self.source_shape.len());

        let mut bounds = Bounds::all(&self.shape);

        for axis in 0..self.source_shape.len() {
            if !self.broadcast[axis + self.offset] {
                bounds[axis + self.offset] = source_bounds[axis].clone();
            }
        }

        bounds
    }

    pub fn shape(&'_ self) -> &'_ Shape {
        &self.shape
    }
}

#[derive(Clone)]
pub struct Slice {
    source_shape: Shape,
    shape: Shape,
    bounds: Bounds,
    offset: HashMap<usize, u64>,
    elided: HashMap<usize, u64>,
    inverted_axes: Vec<usize>,
}

impl Slice {
    pub fn new(source_shape: Shape, bounds: Bounds) -> TCResult<Slice> {
        let mut shape: Vec<u64> = Vec::with_capacity(bounds.len());
        let mut offset = HashMap::new();
        let mut elided = HashMap::new();
        let mut inverted_axes = Vec::with_capacity(bounds.len());

        for axis in 0..bounds.len() {
            match &bounds[axis] {
                AxisBounds::At(c) => {
                    elided.insert(axis, *c);
                }
                AxisBounds::In(range, step) => {
                    let dim = (range.end - range.start).div_ceil(step);
                    shape.push(dim);
                    offset.insert(axis, range.start);
                    inverted_axes.push(axis);
                }
                AxisBounds::Of(indices) => {
                    shape.push(indices.len() as u64);
                    inverted_axes.push(axis);
                }
            }
        }

        for axis in bounds.len()..source_shape.len() {
            shape.push(source_shape[axis]);
            inverted_axes.push(axis);
        }

        let shape: Shape = shape.into();

        Ok(Slice {
            source_shape,
            shape,
            bounds,
            offset,
            elided,
            inverted_axes,
        })
    }

    pub fn bounds(&'_ self) -> &'_ Bounds {
        &self.bounds
    }

    pub fn ndim(&self) -> usize {
        self.shape.len()
    }

    pub fn shape(&'_ self) -> &'_ Shape {
        &self.shape
    }

    pub fn size(&self) -> u64 {
        self.shape.size()
    }

    pub fn invert_axes(&self, axes: Vec<usize>) -> Vec<usize> {
        assert!(axes.len() <= self.ndim());
        for x in &axes {
            assert!(*x < self.ndim());
        }

        axes.iter().map(|x| self.inverted_axes[*x]).collect()
    }

    pub fn invert_bounds(&self, mut bounds: Bounds) -> Bounds {
        bounds.normalize(&self.shape);

        let mut source_bounds = Vec::with_capacity(self.source_shape.len());
        let mut source_axis = 0;
        for axis in 0..self.shape.len() {
            if let Some(c) = self.elided.get(&axis) {
                source_bounds.push(AxisBounds::At(*c));
                continue;
            }

            use AxisBounds::*;
            match &bounds[source_axis] {
                In(range, this_step) => {
                    if let In(source_range, source_step) = &self.bounds[axis] {
                        let start = range.start + source_range.start;
                        let end = start + (source_step * (range.end - range.start));
                        let step = source_step * this_step;
                        source_bounds.push((start..end, step).into());
                    } else {
                        assert!(range.start == 0);
                        source_bounds.push(self.bounds[axis].clone());
                    }
                }
                Of(indices) => {
                    let offset = self.offset.get(&axis).unwrap_or(&0);
                    source_bounds.push(
                        indices
                            .iter()
                            .map(|i| i + offset)
                            .collect::<Vec<u64>>()
                            .into(),
                    )
                }
                At(i) => {
                    let offset = self.offset.get(&axis).unwrap_or(&0);
                    source_bounds.push((i + offset).into())
                }
            }
            source_axis += 1;
        }

        source_bounds.into()
    }

    pub fn invert_coord(&self, coord: &[u64]) -> Vec<u64> {
        let mut source_coord = Vec::with_capacity(self.source_shape.len());
        let mut source_axis = 0;
        for axis in 0..self.shape.len() {
            if let Some(elided) = self.elided.get(&axis) {
                source_coord.push(*elided);
            } else {
                let offset = self.offset.get(&axis).unwrap_or(&0);
                source_coord.push(coord[source_axis] + *offset);
                source_axis += 1;
            }
        }

        source_coord
    }

    pub fn map_bounds(&self, source_bounds: Bounds) -> Bounds {
        assert!(source_bounds.len() == self.source_shape.len());

        let mut coord: Vec<AxisBounds> = Vec::with_capacity(self.shape.len());

        for axis in 0..self.source_shape.len() {
            if self.elided.contains_key(&axis) {
                continue;
            }

            use AxisBounds::*;
            match &source_bounds[axis] {
                In(_, _) => todo!(),
                Of(indices) => {
                    let offset = self.offset.get(&axis).unwrap_or(&0);
                    coord.push(
                        indices
                            .iter()
                            .map(|i| i - offset)
                            .collect::<Vec<u64>>()
                            .into(),
                    );
                }
                At(i) => {
                    let offset = self.offset.get(&axis).unwrap_or(&0);
                    coord.push((i - offset).into())
                }
            }
        }

        coord.into()
    }

    pub fn map_coord(&self, source_coord: Vec<u64>) -> Vec<u64> {
        assert!(source_coord.len() == self.source_shape.len());
        let mut coord = Vec::with_capacity(self.shape.len());
        for (axis, c) in source_coord.iter().enumerate() {
            if self.elided.contains_key(&axis) {
                continue;
            }

            let offset = self.offset.get(&axis).unwrap_or(&0);
            coord.push(c - offset);
        }

        coord
    }
}

#[derive(Clone)]
pub struct Transpose {
    source_shape: Shape,
    shape: Shape,
    permutation: Vec<usize>,
}

impl Transpose {
    pub fn new(source_shape: Shape, permutation: Option<Vec<usize>>) -> TCResult<Transpose> {
        let ndim = source_shape.len();
        let permutation = permutation
            .or_else(|| {
                let mut axes: Vec<usize> = (0..ndim).collect();
                axes.reverse();
                Some(axes)
            })
            .unwrap();

        if permutation.len() != ndim {
            let permutation: Vec<String> = permutation.iter().map(|x| x.to_string()).collect();
            return Err(error::bad_request(
                "Invalid permutation for transpose",
                format!(
                    "Tensor with shape {} cannot transpose axes ({})",
                    source_shape,
                    permutation.join(", ")
                ),
            ));
        }

        let mut shape: Vec<u64> = Vec::with_capacity(ndim);
        for axis in &permutation {
            shape.push(source_shape[*axis]);
        }
        let shape: Shape = shape.into();
        Ok(Transpose {
            source_shape,
            shape,
            permutation,
        })
    }

    pub fn invert_axes(&self, axes: Vec<usize>) -> Vec<usize> {
        axes.iter().map(|x| self.permutation[*x]).collect()
    }

    pub fn invert_bounds(&self, bounds: Bounds) -> Bounds {
        let mut source_bounds = Bounds::all(&self.source_shape);
        for axis in 0..bounds.len() {
            source_bounds[self.permutation[axis]] = bounds[axis].clone();
        }
        source_bounds
    }

    pub fn invert_coord(&self, coord: &[u64]) -> Vec<u64> {
        let mut source_coord = Vec::with_capacity(self.source_shape.len());
        for axis in 0..coord.len() {
            source_coord[self.permutation[axis]] = coord[axis];
        }

        source_coord
    }

    pub fn map_bounds(&self, source_bounds: Bounds) -> Bounds {
        let mut bounds = Bounds::all(&self.shape);
        for axis in 0..source_bounds.len() {
            bounds[self.permutation[axis]] = source_bounds[axis].clone();
        }
        bounds
    }

    pub fn map_coord(&self, source_coord: Vec<u64>) -> Vec<u64> {
        let mut coord = Vec::with_capacity(self.source_shape.len());
        for axis in 0..source_coord.len() {
            coord[self.permutation[axis]] = source_coord[axis];
        }

        coord
    }

    pub fn shape(&'_ self) -> &'_ Shape {
        &self.shape
    }
}