# P7f-micro — seconde relation ciblée pour QSynth 260

## Point de départ

P7d-lite certifie `v9 == v8`. Après la substitution `v9 -> v8` et un appel au
simplificateur ordinaire, le résidu exact est :

```text
2*(v0*v4) - (v0&v6) - (v6&v8) - (v6&v10) + (v0&v6&v8)
```

Il contient encore trois atomes cachés directs :

| Atome | Occurrences dans le résidu |
| --- | ---: |
| `v6` | 4 |
| `v8` | 2 |
| `v10` | 1 |

Cet ordre structurel fixe `(a,b,c) = (v6,v8,v10)`. P7f-micro ne teste ni
permutation exhaustive, ni autre famille de coefficients.

## Relations testées

Les définitions word-level complètes des trois atomes sont mises en cache, puis
les trois seules relations autorisées sont envoyées au moteur exact existant :

```text
v6 - v8 - v10       == 0
v6 + v8 - v10       == 0
v6 + v8 + v10 + 1   == 0
```

Une relation prouvée aurait produit une substitution simultanée avant un seul
nouvel appel au simplificateur. Aucune substitution n'est appliquée sans preuve.

## Résultat

Commande :

```text
CARGO_HOME=/tmp/rumba-cargo-home just hidden-atom-diagnostics
```

```text
p7f_micro line=260 scope=63 remaining_atoms=[VarId(6), VarId(8), VarId(10)] occurrence_counts=[(VarId(6), 4), (VarId(8), 2), (VarId(10), 1)] attempts=3 proved=0 proof_errors=0 substitutions=0 result_nodes=21 residual_zero=false
```

| Mesure | Valeur |
| --- | ---: |
| Relations tentées | 3 |
| Relations prouvées | 0 |
| Erreurs ou dépassements de budget | 0 |
| Substitutions ternaires | 0 |
| Taille finale | 21 nœuds |
| Résidu nul | non |

## Gate

P7f-micro n'apporte pas la seconde relation recherchée. La branche ternaire
ciblée s'arrête donc ici pour 260 :

- ne pas élargir aux permutations ou coefficients généraux ;
- ne pas ajouter de solveur relationnel ;
- ne pas modifier `hide_in_var` ;
- conserver l'alias binaire `v9 == v8` comme diagnostic utile mais insuffisant.

Le résidu restant dépend vraisemblablement d'une relation contextuelle avec les
opérandes originaux ou d'une famille qui sort volontairement du budget P7f.
