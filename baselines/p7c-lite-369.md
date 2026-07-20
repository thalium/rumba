# P7c-lite — complémentarité des atomes directs de QSynth 369

## Hypothèse testée

P7b-lite a montré que `v7` et `v9` ne sont pas individuellement des fonctions
bitwise uniformes de leurs parents immédiats. P7c-lite teste une relation
mutuelle, sans imposer cette propriété aux deux atomes :

```text
def(v7) + def(v9) + 1 == 0 mod 2^64
```

Le diagnostic utilise `HiddenAtomTrace::simplified`, c'est-à-dire les
définitions word-level complètes dans le même scope. Il ne se sert pas des
`dependency_definition`, qui peuvent encore contenir des parents abstraits.

Si, et seulement si, le simplificateur exact ramène cette relation à zéro, le
diagnostic substitue `v9 = ~v7` dans le résultat pré-restauration et le
simplifie une fois.

## Résultat

Commande :

```text
CARGO_HOME=/tmp/rumba-cargo-home just hidden-atom-diagnostics
```

Résultat du scope final de la ligne 369 :

| Mesure | Valeur |
| --- | ---: |
| Atomes directs | `v7`, `v9` |
| Relation testée | `def(v7) + def(v9) + 1` |
| Preuve exacte | obtenue |
| Substitution | `v9 -> ~v7` |
| Taille après substitution/simplification | 1 nœud |
| Résidu nul | oui |

Sortie correspondante :

```text
p7c_lite line=369 scope=1 left=v7 right=v9 proof=proved result_nodes=Some(1) residual_zero=true
```

Les définitions complètes observées sont :

```text
v7 = -1 - v2*v3 - v3*v3 - v3*(v2 & (-1 - v2 - v3))
v9 = 2*v2*v3 - v3*(v2 & (v2 + v3)) + v3*v3
```

Leur somme plus un est certifiée nulle modulo `2^64`. On obtient donc
`v7 = -v9 - 1 = ~v9`, ce qui suffit à annuler le résidu contenant les deux
conjonctions complémentaires.

## Gate

Le gate P7c-lite est positif : la ligne 369 est résolue par une petite relation
guidée par la structure du résidu. Cela justifie une phase séparée recherchant
quelques relations structurelles entre les atomes directement co-présents dans
les quatre autres NG.

Ce commit n'effectue pas cette extension et n'ajoute ni solveur relationnel
général, ni domaine LowBit.
