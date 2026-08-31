# Restricted factorized section (FSC)

`rumba-core` implements a deliberately restricted FSC-2 one-base
specialization, not general FSC or FSC-K. It recognizes exact two-variable
behavior that can be represented by a single stationary base and never emits a
multi-base representation.

FSC is used at two points in the production pipeline:

```text
raw source
  -> descriptor-native FSC-2 frontend
       static Boolean recipe on an exact match
       or refusal -> existing v1 simplifier
  -> fixed point and prettify on the v1 path
  -> exact final Möbius surface projection
       smaller certified candidate
       or refusal -> preserve the v1 result
```

Both entry points are conservative. Unsupported input, an incomplete compile,
budget exhaustion or a failed exact comparison is a normal refusal, never an
approximate simplification.

## Descriptor-native frontend

The frontend analyzes the raw expression before the existing simplifier. A
bounded bottom-up compiler first attempts to derive an exact FSC-2 one-base
descriptor directly. When that deliberately incomplete path returns unknown,
the same descriptor may be recovered through the bounded graph compiler.

A descriptor is rendered only when its four Boolean responses form one of the
16 two-variable truth tables. The corresponding static recipe is emitted from
`0`, `1`, `x`, `y`, complement, `&`, `|` and `^`. The recipes are untrusted
candidate producers: tests replay every truth table and compile every recipe
back to its declared descriptor. A descriptor miss falls through to v1
unchanged.

## Final candidate surface

After the v1 fixed point and `prettify`, the final projection evaluates a
source with exactly two variables, `x` and `y`, at the four Boolean
assignments, in this order:

```text
(x,y) = (0,0), (1,0), (0,1), (1,1)
```

From these values it computes the four Möbius coefficients:

```text
c0 = f(0,0)
c1 = f(1,0) - f(0,0)
c2 = f(0,1) - f(0,0)
c3 = f(1,1) - f(1,0) - f(0,1) + f(0,0)
```

At the production width, candidate values are derived from 64-bit words. FSC
stores the exact signed coefficients and graph carries in checked `i128`
arithmetic. The nominated expression is the corresponding surface form:

```text
c0 + c1*x + c2*y + c3*(x & y)
```

Zero terms and unit coefficients are reduced before the candidate is emitted.
The candidate must also be strictly smaller than the source according to the
expression-node cost. It is only a nomination; it is accepted after the exact
source comparison described below.

## Exact section semantics

The shared certifier compiles the source into a finite two-adic transition
graph. A graph has a root, an exact root response `r0`, and bases. Each base
contains:

```text
epsilon[4]  -- an exact offset for each pair of input bits
tau[4]      -- the next base for each pair of input bits
```

During the bit-by-bit evaluation, a cursor contains a base and a carry. For an
input symbol `s` in `{0, 1, 2, 3}`, the transition is:

```text
total = carry + epsilon[s]
bit   = total mod 2
carry = (total - bit) / 2
next  = tau[s]
```

The recurrence is exact over two-adic arithmetic. It accounts for carries
between word bits instead of relying on a finite sample of machine-word values.
The bounded additive fallback described below may use rational proof
coefficients internally, but it emits no such coefficient.

### Graph construction

The compiler handles the supported expression constructors compositionally:

- a locally stationary Boolean expression becomes a one-base graph from its
  width-one truth table;
- addition forms the product of child bases and adds their responses;
- bitwise operators form a product of child cursors and combine their output
  bits;
- complement uses `~F = -1 - F`;
- integer scaling transforms the exact responses and offsets;
- multiplication nodes are outside this pass and are refused.

After composition, states with identical exact behavior are merged. The
minimization key is the pair consisting of a state's offset profile and the
already-refined classes of its four destinations. Only reachable canonical
classes are retained.

## Final direct match

For the nominated candidate, the five exact scalars are:

```text
r0        = c0
epsilon   = [0, c1, c2, c1 + c2 + c3]
```

The source graph is required to reduce to one canonical base rooted at `0`,
with zero offset for symbol `0` and integer `r0`/offsets. Those five scalars
are then compared directly with the candidate's five scalars. This is an exact
equality check, not a random or bounded semantic test.

If every scalar matches, FSC emits the candidate and runs the normal prettifier
on it. Otherwise the original prettified source is preserved.

## Bounded additive fallback

For an `Add` source whose product graph exceeds the graph-state budget, FSC
keeps the already compiled child graphs and checks a bounded exact linear
relation against the candidate section. The relation is propagated through
every input symbol and every reachable child base. Rational coefficients are
used only for this internal elimination; a candidate is accepted only when all
four symbol observations cancel exactly.

The fallback is limited to:

- dimension at most 512;
- at most 512 basis vectors;
- at most 50,000,000 charged exact operations.

It is attempted only for additive sources after the ordinary product-graph
construction reports a state-budget refusal. Any mismatch, overflow, or budget
exhaustion preserves the source unchanged.

## Scope and failure behavior

The current production implementation intentionally has a small domain:

- machine width 64;
- exactly two source variables;
- no multiplication node;
- at most 4096 source nodes;
- nesting depth below 256 levels;
- variable identifiers below the 4096-slot evaluation bound;
- at most 256 intermediate graph states and 64 minimized classes on the direct
  graph path;
- a single-base exact result on the direct graph path, or a bounded additive
  relation on the fallback path.

The graph compiler may manipulate multiple bases internally to prove a source,
but production never emits a multi-base FSC object. The early frontend emits
only the 16 static Boolean recipes; more general one-base Möbius coefficients
are available only through the final projection.

Any unsupported node, invalid variable or width, arithmetic overflow, graph or
linear budget overflow, multiple semantic bases, or scalar mismatch is a normal
refusal. FSC never turns such a refusal into a
simplification result and never changes the behavior of the existing
simplifier for expressions it cannot certify. Exact response values are
represented with bounded signed integers, so an unrepresentable carry is also
a conservative refusal.
