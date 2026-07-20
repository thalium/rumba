# P7d-lite — alias sémantiques guidés sur les quatre NG restants

## Périmètre

Le diagnostic analyse uniquement le scope dont l'entrée est le résidu final de
QSynth 53, 249, 260 et 481. Il conserve les seuls atomes cachés directement
référencés par le résultat pré-restauration.

La sélection des paires est guidée par deux formes locales :

1. deux atomes occupent le même contexte dans des termes additifs distincts,
   par exemple `2*(c&a) - 2*(c&b)` ;
2. deux atomes sont co-présents dans un même opérateur bitwise ou produit.

Les contextes identiques sont prioritaires. Les paires sont dédupliquées et
limitées à 12 par scope.

Pour chaque paire `(a, b)`, seules trois relations sont tentées sur les
définitions word-level complètes mises en cache :

```text
a - b       == 0  (égalité)
a + b + 1   == 0  (complément)
a + b       == 0  (opposé arithmétique)
```

Une substitution n'est appliquée qu'après réduction exacte de sa relation à
zéro. Le résultat pré-restauration est ensuite simplifié par un seul appel au
moteur ordinaire. Le chemin normal de RUMBA n'est pas modifié.

## Résultats

Commande :

```text
CARGO_HOME=/tmp/rumba-cargo-home just hidden-atom-diagnostics
```

| Ligne | Atomes directs | Paires | Preuves | Alias appliqué | Nœuds finaux | Zéro |
| ---: | ---: | ---: | ---: | --- | ---: | :---: |
| 53 | 2 | 1 | 0/3 | aucun | 58 | non |
| 249 | 3 | 3 | 0/9 | aucun | 56 | non |
| 260 | 4 | 5 | 1/15 | `v9 -> v8` | 21 | non |
| 481 | 3 | 3 | 1/9 | `v7 -> v6` | 1 | oui |

Aucune tentative n'a dépassé le budget PCT.

La ligne 260 confirme qu'une relation vraie ne suffit pas nécessairement à
expliquer tout le résidu. La ligne 481 est entièrement expliquée par l'égalité
`v6 == v7` :

```text
2*(v3 & v6) - 2*(v3 & v7) == 0
```

## Gate

P7d-lite résout un seul NG supplémentaire. Avec 369, la couverture
diagnostique atteint donc deux cas sur cinq, mais le gate d'industrialisation
n'est pas encore franchi :

- ne pas modifier `hide_in_var` ;
- ne pas activer ces substitutions dans le chemin normal ;
- examiner d'abord le coût et la fréquence de ces alias sur le corpus ;
- conserver LowBit comme piste séparée.

53 et 249 ne présentent aucun alias certifié dans l'espace testé. 260 nécessite
au moins une relation ou une structure supplémentaire après `v8 == v9`.
