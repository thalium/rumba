use pyo3::exceptions;
use pyo3::prelude::*;
use rumba_core::expr;
use rumba_core::simplify::simplify_mba;
use rumba_core::varint::make_mask;

#[cfg(feature = "parse")]
use rumba_core::parser::parse_expr;
/// A python wrapper for an MBA expression
#[pyclass]
#[derive(Clone)]
struct Expr {
    inner: expr::Expr,
}

#[pymethods]
impl Expr {
    /// Creates a new expression from an int or by parsing a string
    #[new]
    fn new(obj: Option<Bound<'_, PyAny>>) -> PyResult<Self> {
        if let Some(py_obj) = obj {
            if let Ok(value) = py_obj.extract::<u64>() {
                Ok(Self {
                    inner: expr::Expr::Const(value.into()),
                })
            } else if let Ok(_value) = py_obj.extract::<String>() {
                #[cfg(feature = "parse")]
                {
                    match parse_expr(&_value) {
                        Ok(inner) => Ok(Self { inner }),
                        Err(e) => Err(pyo3::exceptions::PySyntaxError::new_err(e)),
                    }
                }

                #[cfg(not(feature = "parse"))]
                {
                    panic!("Rumba was not compiled with parsing.")
                }
            } else {
                Err(pyo3::exceptions::PyTypeError::new_err(
                    "Expected an int for Expr constructor",
                ))
            }
        } else {
            Ok(Self {
                inner: expr::Expr::zero(),
            })
        }
    }

    /// Creates a new variable
    #[staticmethod]
    fn var(id: usize) -> PyResult<Self> {
        Ok(Self {
            inner: expr::Expr::Var(id.into()),
        })
    }

    /// Creates a new int
    #[staticmethod]
    fn int(c: u64) -> PyResult<Self> {
        Ok(Self {
            inner: expr::Expr::Const(c.into()),
        })
    }

    /// Parses an expression
    #[cfg(feature = "parse")]
    #[staticmethod]
    fn parse(s: &str) -> PyResult<Self> {
        match parse_expr(s) {
            Ok(inner) => Ok(Self { inner }),
            Err(e) => Err(pyo3::exceptions::PySyntaxError::new_err(e)),
        }
    }

    fn to_int(&self) -> PyResult<u64> {
        match self.inner {
            expr::Expr::Const(c) => Ok(c.get(u64::MAX)),
            _ => Err(pyo3::exceptions::PyTypeError::new_err("Not an int")),
        }
    }

    /// Evaluates an expression with the given variables
    fn eval(&self, vars: Vec<u64>, n: u8) -> u64 {
        self.inner.eval(&vars).get(make_mask(n))
    }

    /// Perform arithmetic reduction on this expression
    fn reduce(&mut self, n: u8) -> Self {
        Self {
            inner: self.inner.clone().reduce(make_mask(n)),
        }
    }

    /// Attempts to solve this expression, treating it as a non polynomial MBA
    fn solve(&mut self, n: u8) -> Self {
        Self {
            inner: simplify_mba(self.inner.clone(), n),
        }
    }

    /// The number of nodes in this expression
    fn size(&self) -> usize {
        self.inner.size()
    }

    fn __repr__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    fn __debug__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// A string representation of the expression
    fn repr(&self, bits: u8, hex: bool, latex: bool) -> String {
        self.inner.repr(bits, make_mask(bits), hex, latex)
    }

    // Arithmetic operators
    fn __add__(&self, other: Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(rhs) = other.extract::<Self>() {
            Ok(Self {
                inner: self.inner.clone() + rhs.inner.clone(),
            })
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() + expr::Expr::Const(rhs_int.into()),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() - expr::Expr::Const(rhs_int.into()),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: expr::Expr::Const(rhs_int.into()) - self.inner.clone(),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() * expr::Expr::Const(rhs_int.into()),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() ^ expr::Expr::Const(rhs_int.into()),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() & expr::Expr::Const(rhs_int.into()),
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
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(Self {
                inner: self.inner.clone() | expr::Expr::Const(rhs_int.into()),
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
            Ok(self.inner.clone() == rhs.inner.clone())
        } else if let Ok(rhs_int) = other.extract::<u64>() {
            Ok(self.inner.clone() == expr::Expr::Const(rhs_int.into()))
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
}

#[pymodule]
mod pyrumba {
    #[pymodule_export]
    use super::Expr;
}
