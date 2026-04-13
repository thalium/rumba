# pyrumba.pyi

from typing import Optional, Any, List

class Expr:
    """
    Represents a symbolic or constant expression.
    """

    def __init__(self, obj: Optional[Any] = ...) -> None:
        """
        Construct a new expression.

        If given an integer, produces a constant expression.
        If given a string, attempts to parse it.
        If given nothing, creates a zero expression.
        """

    @staticmethod
    def var(id: int) -> "Expr":
        """
        Create a variable expression with the given variable index.
        """

    @staticmethod
    def int(c: int) -> "Expr":
        """
        Create a constant expression with the given integer value.
        """

    @staticmethod
    def parse(s: str) -> "Expr":
        """
        Parse a string into an expression.
        """

    def to_int(self) -> int:
        """
        Return the integer value of this expression if it is a constant.

        Raises:
            TypeError: if the expression is not a constant.
        """

    def eval(self, vars: List[int], n: int) -> int:
        """
        Evaluate the expression using the supplied list of variable values.

        Args:
            vars: Values corresponding to variable identifiers.
            n: Bit-width mask to apply to the computation.

        Returns:
            The evaluated integer result, masked to n bits.
        """

    def reduce(self, n: int) -> "Expr":
        """
        Perform arithmetic reduction on this expression using an n-bit mask.

        Returns:
            A new reduced expression.
        """

    def solve(self, n: int) -> "Expr":
        """
        Attempt to algebraically simplify or solve the expression
        as a non-polynomial MBA expression using an n-bit domain.

        Returns:
            A simplified expression.
        """

    def size(self) -> int:
        """
        Return the number of nodes in the underlying expression tree.
        """

    def __repr__(self) -> str:
        """
        Return a programmer-friendly string representation of the expression.
        """

    def __str__(self) -> str:
        """
        Return a user-friendly string representation of the expression.
        """

    def __debug__(self) -> str:
        """
        Return a debug representation of the underlying Rust expression.
        """

    def repr(self, bits: int, hex: bool, latex: bool) -> str:
        """
        Produce a formatted string representation of the expression.

        Args:
            bits: Bit-width for masking.
            hex: Whether to format constants in hexadecimal.
            latex: Whether to output LaTeX-compatible formatting.
        """

    def __add__(self, other: Any) -> "Expr":
        """
        Add another expression or integer to this expression.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __radd__(self, other: Any) -> "Expr":
        """
        Right-hand addition. Equivalent to __add__.
        """

    def __sub__(self, other: Any) -> "Expr":
        """
        Subtract another expression or integer from this expression.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __rsub__(self, other: Any) -> "Expr":
        """
        Right-hand subtraction: compute other - self.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __mul__(self, other: Any) -> "Expr":
        """
        Multiply this expression by another expression or integer.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __rmul__(self, other: Any) -> "Expr":
        """
        Right-hand multiplication. Equivalent to __mul__.
        """

    def __xor__(self, other: Any) -> "Expr":
        """
        Bitwise XOR with another expression or integer.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __rxor__(self, other: Any) -> "Expr":
        """
        Right-hand XOR. Equivalent to __xor__.
        """

    def __and__(self, other: Any) -> "Expr":
        """
        Bitwise AND with another expression or integer.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __rand__(self, other: Any) -> "Expr":
        """
        Right-hand AND. Equivalent to __and__.
        """

    def __or__(self, other: Any) -> "Expr":
        """
        Bitwise OR with another expression or integer.

        Raises:
            TypeError: if the operand is not an Expr or int.
        """

    def __ror__(self, other: Any) -> "Expr":
        """
        Right-hand OR. Equivalent to __or__.
        """

    def __eq__(self, other: Any) -> bool:
        """
        Compare this expression with another expression or integer.

        Returns:
            True if equal, False otherwise.

        Raises:
            TypeError: if the operand is neither an Expr nor an int.
        """

    def __invert__(self) -> "Expr":
        """
        Compute the bitwise NOT of this expression.
        """

    def __neg__(self) -> "Expr":
        """
        Compute the arithmetic negation of this expression.
        """

__all__ = ["Expr"]
