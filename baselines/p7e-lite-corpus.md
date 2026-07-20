# P7e-lite minimal — diagnostic corpus

Commande :

```text
cargo run -p rumba-core --release --features parse --example p7e_lite_corpus -- 3
```

La mesure exécute le chemin normal sur les 41 000 cas, puis lance P7e-lite
uniquement sur les résultats classés NG. La collecte de trace et la fermeture
P7e-lite sont chronométrées séparément. P7e-lite reste désactivé dans le
simplificateur normal.

## Couverture

| Corpus | NG avant | Résolus | NG après |
| --- | ---: | ---: | ---: |
| Loki tiny | 82 | 13 | 69 |
| QSynth EA | 19 | 19 | 0 |
| Total | 101 | 32 | 69 |

La baisse globale est de 32 NG, soit 31,7 %. Les cinq régressions cibles sont
résolues automatiquement et font partie des 19 résolutions QSynth.

## Temps

| Passage | Baseline | Trace | Fermeture | Surcoût total |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3 549,141 ms | 17,299 ms | 712,473 ms | 20,562 % |
| 2 | 3 606,332 ms | 18,433 ms | 769,533 ms | 21,850 % |
| 3 | 3 830,398 ms | 17,488 ms | 713,147 ms | 19,075 % |

Médiane des ratios :

- fermeture seule : 20,075 % ;
- trace et fermeture : 20,562 %.

La collecte diagnostique coûte environ 0,5 % du temps de référence. Le surcoût
provient donc de l'énumération exhaustive et des certifications exactes de la
fermeture minimale. La couverture est positive, mais cette version ne satisfait
pas une cible de surcoût inférieure à 5 % pour une activation corpus générale.
