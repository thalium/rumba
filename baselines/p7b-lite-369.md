# P7b-lite — diagnostic ciblé sur QSynth 369

## Hypothèse testée

Le résultat pré-restauration du scope final référence directement deux atomes
arithmétiques. Le micro-expériment tente, pour chacun d'eux :

1. d'évaluer sa définition sur le cube `0/mask` de ses parents immédiats ;
2. de reconstruire une formule bitwise en forme normale disjonctive si toutes
   les sorties sont `0` ou `mask` ;
3. de certifier exactement `definition - candidate == 0` avec le PCT actuel ;
4. de substituer uniquement les candidats certifiés puis de simplifier une fois.

La procédure est diagnostique : elle ne modifie pas le chemin normal du
simplificateur.

## Résultat

Commande :

```text
CARGO_HOME=/tmp/rumba-cargo-home just hidden-atom-diagnostics
```

Résultat du scope final de la ligne 369 :

| Mesure | Valeur |
| --- | ---: |
| Atomes directement référencés | 2 (`v7`, `v9`) |
| Atomes tentés | 2 |
| Candidats certifiés | 0 |
| Rejets `non_boolean_cube` | 2 |
| Taille après substitution/simplification | 11 nœuds |
| Résidu nul | non |

Sortie correspondante :

```text
p7b_lite line=369 scope=1 direct_atoms=[VarId(7), VarId(9)] attempted_atoms=[VarId(7), VarId(9)] proofs=0 result_nodes=11 residual_zero=false
  rejection atom=v7 reason=non_boolean_cube
  rejection atom=v9 reason=non_boolean_cube
```

Les deux dépendances directes produisent au moins une valeur différente de
`0` et `mask` sur le cube des parents. Elles ne définissent donc pas une
fonction bitwise uniforme de ces parents, et la preuve PCT n'a aucun candidat
à certifier.

## Gate

La ligne 369 n'est pas résolue. Conformément au gate P7b-lite :

- ne pas étendre ce mécanisme aux quatre autres NG ;
- ne pas industrialiser P7a ;
- arrêter cette branche et orienter une éventuelle suite vers les relations
  arithmétiques entre dépendances.

Ce résultat n'exclut pas une relation contextuelle ou arithmétique plus riche.
Il invalide seulement l'hypothèse ciblée selon laquelle l'un des deux atomes
directs de 369 serait, à lui seul, une fonction bitwise de ses parents
immédiats.
