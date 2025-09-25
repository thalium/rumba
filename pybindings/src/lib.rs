use mbalib::anf::ANF;
use mbalib::anf::ANFExpr;
use mbalib::expr::Expr;
use pyo3::exceptions;
use pyo3::prelude::*;

#[pyclass]
#[derive(Clone)]
struct PyExpr {
    inner: Expr,
}

#[pymethods]
impl PyExpr {
    #[new]
    fn new(obj: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        if let Some(py_obj) = obj {
            if let Ok(value) = py_obj.extract::<u128>() {
                Ok(Self {
                    inner: value.into(),
                })
            } else {
                Err(pyo3::exceptions::PyTypeError::new_err(
                    "Expected an int for Expr constructor",
                ))
            }
        } else {
            Ok(Self {
                inner: Expr::Const(0),
            })
        }
    }

    #[staticmethod]
    fn var(id: usize) -> PyResult<Self> {
        Ok(Self {
            inner: Expr::Var(id),
        })
    }

    fn to_int(&self) -> PyResult<u128> {
        match self.inner {
            Expr::Const(c) => Ok(c),
            _ => Err(pyo3::exceptions::PyTypeError::new_err("Not an int")),
        }
    }

    fn simplify(&mut self) {
        self.inner = self.inner.clone().arith_reduce();
    }

    fn anf(&mut self, n: usize) -> PyANFExpr {
        PyANFExpr {
            inner: (self.inner.clone(), n).into(),
        }
    }

    fn size(&self) -> usize {
        self.inner.size()
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }
    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    // Arithmetic operators
    fn __add__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() + rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() + rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }

    fn __radd__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__add__(other)
    }

    fn __sub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() - rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() - rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }

    fn __rsub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: rhs.inner.clone() - self.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: Expr::Const(rhs_int) - self.inner.clone(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }

    fn __mul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() * rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() * rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }
    fn __rmul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__mul__(other)
    }

    fn __xor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() ^ rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() ^ rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }
    fn __rxor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__xor__(other)
    }

    fn __and__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() & rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() & rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }

    fn __rand__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__and__(other)
    }

    fn __or__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() | rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() | rhs_int.into(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be Expr or int",
            ))
        }
    }
    fn __ror__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__or__(other)
    }

    fn __eq__(&self, other: Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(self.inner == rhs.inner)
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(self.inner == rhs_int.into())
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }
    fn __req__(&self, other: Bound<'_, PyAny>) -> PyResult<bool> {
        self.__eq__(other)
    }

    fn __invert__(&self) -> Self {
        Self {
            inner: !self.inner.clone(),
        }
    }

    fn __neg__(&self) -> Self {
        Self {
            inner: -self.inner.clone(),
        }
    }

    fn __lshift__(&self, shift: u128) -> Self {
        Self {
            inner: self.inner.clone() << Expr::Const(shift),
        }
    }

    fn __rshift__(&self, shift: u128) -> Self {
        Self {
            inner: self.inner.clone() >> Expr::Const(shift),
        }
    }
}

#[pyclass]
#[derive(Clone, Default)]
struct PyANF {
    inner: ANF,
}

#[pymethods]
impl PyANF {
    #[new]
    fn new() -> Self {
        Self {
            inner: ANF::default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    // Example operator overloads (if ANF supports them)
    fn __and__(&self, other: &Self) -> Self {
        Self {
            inner: self.inner.clone() & other.inner.clone(),
        }
    }

    fn __xor__(&self, other: &Self) -> Self {
        Self {
            inner: self.inner.clone() ^ other.inner.clone(),
        }
    }

    fn size(&self) -> usize {
        self.inner.size()
    }
}

// Macro to expose ANFExpr<N> to Python

#[pyclass]
#[derive(Clone)]
struct PyANFExpr {
    inner: ANFExpr,
}

#[pymethods]
impl PyANFExpr {
    #[new]
    fn new(n: usize, obj: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        if let Some(py_obj) = obj {
            if let Ok(value) = py_obj.extract::<u128>() {
                Ok(Self {
                    inner: ANFExpr::from_value(value, n),
                })
            } else {
                Err(pyo3::exceptions::PyTypeError::new_err(
                    "Expected an int for ANFExpr constructor",
                ))
            }
        } else {
            Ok(Self {
                inner: ANFExpr::zero(n),
            })
        }
    }

    #[staticmethod]
    fn var(id: usize, n: usize) -> PyResult<Self> {
        Ok(Self {
            inner: ANFExpr::var(id, n),
        })
    }

    fn to_int(&self) -> PyResult<u128> {
        self.inner
            .to_int()
            .ok_or(pyo3::exceptions::PyTypeError::new_err("Not an int"))
    }

    fn eval(&self, vars: Vec<u128>) -> PyResult<u128> {
        Ok(self.inner.eval(&vars))
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }
    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    // Indexing
    fn __getitem__(&self, idx: usize) -> PyResult<PyANF> {
        self.inner
            .bits
            .get(idx)
            .cloned()
            .map(|inner| PyANF { inner })
            .ok_or_else(|| exceptions::PyIndexError::new_err("index out of bounds"))
    }

    fn __setitem__(&mut self, idx: usize, value: PyRef<PyANF>) -> PyResult<()> {
        if idx >= self.inner.bits.len() {
            return Err(exceptions::PyIndexError::new_err("index out of bounds"));
        }
        self.inner.bits[idx] = value.inner.clone();
        Ok(())
    }

    // Arithmetic operators
    fn __add__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() + rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() + ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }

    fn __radd__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__add__(other)
    }

    fn __sub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() - rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() - ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }

    fn __rsub__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: rhs.inner.clone() - self.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: ANFExpr::from_value(rhs_int, self.inner.bits.len()) - self.inner.clone(),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }

    fn __mul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() * rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() * ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }
    fn __rmul__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__mul__(other)
    }

    fn __xor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() ^ rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() ^ ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }
    fn __rxor__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__xor__(other)
    }

    fn __and__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() & rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() & ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }

    fn __rand__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__and__(other)
    }

    fn __or__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() | rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(Self {
                inner: self.inner.clone() | ANFExpr::from_value(rhs_int, self.inner.bits.len()),
            })
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }
    fn __ror__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        self.__or__(other)
    }

    fn __eq__(&self, other: Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(self.inner.bits == rhs.inner.bits)
        } else if let Ok(rhs_int) = other.extract::<u128>() {
            Ok(self.inner.bits == ANFExpr::from_value(rhs_int, self.inner.bits.len()).bits)
        } else {
            Err(exceptions::PyTypeError::new_err(
                "Operand must be ANFExpr or int",
            ))
        }
    }
    fn __req__(&self, other: Bound<'_, PyAny>) -> PyResult<bool> {
        self.__eq__(other)
    }

    fn __invert__(&self) -> Self {
        Self {
            inner: !self.inner.clone(),
        }
    }

    fn __neg__(&self) -> Self {
        Self {
            inner: -self.inner.clone(),
        }
    }

    fn __lshift__(&self, shift: u128) -> Self {
        Self {
            inner: self.inner.clone() << shift,
        }
    }

    fn __rshift__(&self, shift: u128) -> Self {
        Self {
            inner: self.inner.clone() >> shift,
        }
    }

    fn size(&self) -> usize {
        self.inner.size()
    }
}

#[pymodule]
mod mba {
    #[pymodule_export]
    use super::PyANF;

    #[pymodule_export]
    use super::PyANFExpr;

    #[pymodule_export]
    use super::PyExpr;
}
