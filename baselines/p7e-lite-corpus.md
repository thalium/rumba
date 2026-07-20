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

## Temps avant inférence des tables

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

## Après inférence déterministe des tables

Les observations word-level rejettent les tables incompatibles, mais seule une
preuve exacte peut encore certifier une dépendance. Mesure sur trois nouveaux
passages :

| Passage | Baseline | Trace | Fermeture | Surcoût total |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 3 657,497 ms | 17,875 ms | 15,149 ms | 0,903 % |
| 2 | 3 713,391 ms | 17,203 ms | 14,988 ms | 0,867 % |
| 3 | 3 742,556 ms | 16,743 ms | 14,746 ms | 0,841 % |

Compteurs identiques sur les trois passages :

| Compteur | Valeur |
| --- | ---: |
| Tuples de parents | 1 067 |
| Candidats exacts de l'ancienne boucle | 10 220 |
| Tables après canonicalisation, avant filtre | 6 632 |
| Tables rejetées par observations | 6 569 |
| Tables survivantes | 63 |
| Preuves exactes lancées | 62 |
| Preuves exactes réussies | 35 |
| Résolutions au premier tour | 29 |
| Résolutions au second tour | 3 |

Médiane après optimisation :

- fermeture : 14,988 ms, soit environ 47,6 fois plus rapide ;
- surcoût de la fermeture seule : 0,404 % ;
- surcoût complet avec collecte de trace : 0,867 % ;
- réduction des appels exacts : environ 99,4 %.

La couverture reste strictement identique : 32 résolutions, QSynth 19/19 et
Loki 13. Un essai à trois tours n'a changé ni la couverture ni les compteurs ;
la limite reste donc fixée à deux tours.
