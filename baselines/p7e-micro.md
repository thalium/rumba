# P7e-micro — dépendances bitwise binaires prédites

## Périmètre

P7e-micro ne synthétise aucune fonction. Il teste uniquement les trois
dépendances prédites par l'inspection des résidus :

| Ligne | Cible | Candidat compact |
| ---: | --- | --- |
| 53 | `v7` | `~(v2 | v6)` |
| 249 | `v4` | `v1 ^ v3` |
| 260 | `v10` | `~(v0 | v8)` |

Pour la preuve, chaque parent caché du candidat est remplacé par sa définition
word-level complète dans le même scope. Le candidat compact est conservé pour
la substitution locale dans le résultat pré-restauration.

L'acceptation exige que le moteur exact existant réduise :

```text
definition(cible) - candidat_développé
```

à zéro. Aucun échantillonnage et aucune énumération des seize fonctions
binaires ne sont utilisés.

## Cas particulier de 260

La preuve est effectuée après l'alias P7d déjà certifié `v9 -> v8`. La
`dependency_definition` de `v10` est d'abord réécrite avec cet alias, puis
entièrement développée. Sans cet ordre, le résidu de preuve contient seulement
la différence entre `(v0 & def(v9))` et `(v0 & def(v8))` et le moteur ne retrouve
pas l'alias à travers le contexte bitwise.

Après les substitutions compactes :

```text
v9  -> v8
v10 -> ~(v0 | v8)
```

le résidu pré-restauration devient :

```text
2*(v0*v4) - v6
```

La restauration word-level normale de `v6 = 2*(v0*v4)` le réduit à zéro. Ce
n'est pas une relation supplémentaire inventée par P7e-micro.

## Résultats

Commande :

```text
CARGO_HOME=/tmp/rumba-cargo-home just hidden-atom-diagnostics
```

| Ligne | Relation | Preuve exacte | Résidu final |
| ---: | --- | :---: | :---: |
| 53 | NOR binaire | oui | zéro |
| 249 | XOR binaire | oui | zéro |
| 260 | égalité P7d + NOR binaire | oui | zéro |

Sorties correspondantes :

```text
p7e_micro line=53 scope=1 target=v7 candidate=~ (v2 | v6) proof=proved initial_aliases=0 result_nodes=Some(1) residual_zero=true
p7e_micro line=249 scope=1 target=v4 candidate=v1 ^ v3 proof=proved initial_aliases=0 result_nodes=Some(1) residual_zero=true
p7e_micro line=260 scope=63 target=v10 candidate=~ (v0 | v8) proof=proved initial_aliases=1 result_nodes=Some(1) residual_zero=true
```

## Gate

Le gate P7e-micro est positif : `3/3` dépendances sont certifiées et expliquent
entièrement leurs résidus. Avec P7c et P7d, les cinq cas ciblés possèdent
maintenant une explication diagnostique :

| Ligne | Dépendance |
| ---: | --- |
| 53 | NOR binaire |
| 249 | XOR binaire |
| 260 | identité puis NOR binaire |
| 369 | complément unaire |
| 481 | identité unaire |

Ce résultat justifie une étape P7e-lite générique séparée, suivie d'une mesure
corpus. Ce commit n'énumère pas les fonctions unaires/binaires, ne modifie pas
`hide_in_var` et n'active aucun fallback dans le chemin normal.
