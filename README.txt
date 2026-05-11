anfr_annuaire
=============

Scraper de l'annuaire des radioamateurs ANFR + outils de visualisation.


Architecture
------------

    anfr_scrape  ──► data/indicatifs.json       ┐
                     data/indicatifs_vides.json ┤
                                                ├──► anfr_stats ──► dist/stats.html
                                                ┘
                     data/indicatifs.json       ─┐
                     data/cp_geo.json (cache)   ─┴──► anfr_map  ──► dist/map.html

Trois binaires Cargo, deux dossiers de données :

    src/             code Rust
      main.rs        anfr_scrape — scraper ANFR (lit l'annuaire APEX)
      bin/map.rs     anfr_map    — rend une carte Leaflet par code postal
      bin/stats.rs   anfr_stats  — rend un dashboard de statistiques
      bin/*.html     templates HTML inlinés via include_str!

    data/            entrées et état (scraping, cache, session)
      session.json                 état APEX (cookie + tokens), à bootstrapper
      indicatifs.json              records scrapés (sortie scraper, entrée map/stats)
      indicatifs_vides.json        indicatifs sans titulaire, avec date de check
      cp_geo.json                  cache code postal → (commune, lat, lon)

    dist/            sorties HTML (publiables sur GitHub Pages)
      map.html
      stats.html


Build
-----
    cargo build --release

Produit les trois binaires dans target/release/ :
    anfr_scrape, anfr_map, anfr_stats


Bootstrap session (anfr_scrape)
-------------------------------
1. Ouvre https://annuaire-amateurs.anfr.fr/ords/f?p=2003:312 dans un navigateur.
2. Fais une recherche pour initialiser la région du rapport.
3. Copie les valeurs depuis l'onglet Réseau (requête vers /ords/wwv_flow.ajax)
   dans data/session.json :

    {
      "p_instance":  "<depuis l'URL p_context=2003:312:XXX>",
      "p_request":   "PLUGIN=<ajaxIdentifier>",
      "x01":         "<worksheet_id>",
      "x02":         "<report_id>",
      "protected":   "<pPageItemsProtected>",
      "salt":        "<pSalt>",
      "cookie":      "<entête Cookie complet>"
    }

Une fois la première session initialisée, le scraper sait la renouveler tout
seul (--renew, et auto-renew quand la session expire en cours de scrape).


anfr_scrape — usage
-------------------
    ./target/release/anfr_scrape [FLAGS]

Principaux flags :
    --start <IND>          indicatif de départ                    [F4AAA]
    --end <IND>            indicatif de fin (inclus)              [F4ZZZ]
    --delay <SEC>          délai entre requêtes                   [1.0]
    --renew                renouvelle data/session.json au démarrage
    --waf-cooldown <SEC>   attente après rejet WAF, sinon arrêt   [0]
    --pause-every <N>      pause longue tous les N indicatifs     [500]
    --pause-duration <SEC> durée de la pause longue               [60]
    --max-retries <N>      essais max par indicatif               [6]
    --session <PATH>       fichier de session              [data/session.json]
    --output <PATH>        fichier de sortie               [data/indicatifs.json]
    --vides <PATH>         fichier indicatifs vides  [data/indicatifs_vides.json]
    --update-validate      ré-interroge les records réels existants (validation)
    --update-discover      ré-interroge les indicatifs vides connus (redécouverte)
    --older-than <N>       en mode update, ne traite que les entrées non vérifiées depuis N jours

    --help                 voir tous les flags

Exemples :
    Reprise normale :
        ./target/release/anfr_scrape

    Renouveler la session puis scraper avec retry WAF de 30 min :
        ./target/release/anfr_scrape --renew --waf-cooldown 1800

    Plage restreinte :
        ./target/release/anfr_scrape --start F4IMA --end F4IMZ


anfr_map — usage
----------------
Génère une carte Leaflet (HTML autonome) des opérateurs par code postal.
Géocode via https://geo.api.gouv.fr/communes en utilisant data/cp_geo.json
comme cache persistant.

    ./target/release/anfr_map [FLAGS]

    --input <PATH>     fichier source                  [data/indicatifs.json]
    --output <PATH>    fichier HTML de sortie          [dist/map.html]
    --cache <PATH>     cache CP → coordonnées          [data/cp_geo.json]
    --delay-ms <N>     délai entre appels géo (ms)     [50]


anfr_stats — usage
------------------
Génère un dashboard HTML autonome : répartition par classe d'indicatif (FX),
par région, par département, top communes, top prénoms, et estimation du
nombre de femmes (par liste de prénoms français usuels).

    ./target/release/anfr_stats [FLAGS]

    --input <PATH>     fichier des opérateurs   [data/indicatifs.json]
    --output <PATH>    fichier HTML de sortie   [dist/stats.html]


Déploiement GitHub Pages
------------------------
- L'action .github/workflows/pages.yml déploie dist/ vers GitHub Pages à
  chaque push sur main qui touche dist/ (ou via workflow_dispatch).
- Prérequis côté repo : Settings → Pages → Source = "GitHub Actions".
- Le scraper et les données restent locaux (data/ est gitignored — PII et
  session). Workflow type :
    1. localement : ./target/release/anfr_scrape  (mise à jour data/)
    2. localement : ./target/release/anfr_stats && ./target/release/anfr_map
    3. git add dist/ && git commit && git push
    4. l'action publie automatiquement


Comportement du scraper face aux erreurs
----------------------------------------
- Erreur réseau / 5xx : retry avec backoff exponentiel (jusqu'à --max-retries).
- Session APEX expirée : auto-renew (jusqu'à 3 renew consécutifs).
- WAF (rejet) : arrêt immédiat par défaut, ou attente --waf-cooldown puis retry
  (jusqu'à 5 cooldowns consécutifs).


Sortie
------
- data/indicatifs.json : tableau JSON, trié par indicatif, champs
  indicatif/nom/prenom/adresse1/adresse2/localite/code_postal/last_checked.
- data/indicatifs_vides.json : carte indicatif → date du dernier check, pour
  ne pas ré-interroger les indicatifs absents lors d'une reprise.
- data/session.json : mis à jour à chaque renouvellement.
- dist/*.html : artefacts publiables (GitHub Pages, etc.).
