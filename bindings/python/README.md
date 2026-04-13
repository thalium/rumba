# Pyrumba

An MBA simplification library.

## Example

```py
from pyrumba import Expr

v0 = Expr.var(0)
v1 = Expr.var(1)

e = (v0 ^ v1) + 2 * (v0 & v1)

print(e)
# (v0 ^ v1) + (v0 & v1) * 0x2

print(e.solve(64))
# (v0) + (v1)
```