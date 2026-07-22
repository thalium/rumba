import pyrumba


MASK64 = 2**64


def test_build_and_evaluate():
    x = pyrumba.Expr.var(0)
    e = x + 3
    assert e.eval([10], 64) == 13


def test_operators_build_expected_mba():
    x = pyrumba.Expr.var(0)
    y = pyrumba.Expr.var(1)
    # (x ^ y) + 2*(x & y) is an MBA encoding of x + y.
    mba = (x ^ y) + 2 * (x & y)
    assert mba.eval([7, 9], 64) == (7 + 9) % MASK64


def test_parse_simplify_and_evaluate():
    mba = pyrumba.Expr.parse("(v0^v1)+2*(v0&v1)")
    simplified = mba.solve(64)
    assert simplified.eval([123, 456], 64) == (123 + 456) % MASK64
