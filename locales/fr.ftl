## Menus

menu-file = Fichier
menu-edit = Édition
menu-table = Tableau
menu-help = Aide
menu-language = Langue
menu-theme = Thème
menu-density = Hauteur des lignes
theme-system = Comme le système
theme-dark = Sombre
theme-light = Clair
density-compact = Compacte
density-normal = Normale
density-comfortable = Confortable

## Commands

command-new = Nouveau
command-open = Ouvrir…
command-save = Enregistrer
command-save-as = Enregistrer sous…
command-quit = Quitter
protect-formulas = Protéger des formules à l'enregistrement
protect-formulas-hint = Les cellules commençant par =, +, - ou @ (hors nombres) sont écrites précédées d'une apostrophe, pour qu'Excel ou LibreOffice n'exécutent pas ces formules en ouvrant le fichier CSV.
command-undo = Annuler
command-redo = Rétablir
command-find = Rechercher
command-inspector = Inspecteur
command-insert-row-above = Insérer une ligne au-dessus
command-insert-row-below = Insérer une ligne en dessous
command-delete-row = Supprimer la ligne
command-insert-col-left = Insérer une colonne à gauche
command-insert-col-right = Insérer une colonne à droite
command-delete-col = Supprimer la colonne
command-sort-ascending = Trier A → Z
command-sort-descending = Trier Z → A
command-about = À propos de CSV Edit
header-row = Première ligne en en-tête

## Window

title-minimize = Réduire
title-maximize = Agrandir
title-restore = Restaurer
title-close = Fermer

## Grid

find-placeholder = Rechercher…
find-count = { $count ->
    [one] { $count } résultat
   *[other] { $count } résultats
}
new-row = Nouvelle ligne
column-menu = Menu de la colonne { $column }

## Columns

cell-reference = L{ $row } C{ $col }
column-labels = Noms des colonnes
column-hide = Masquer cette colonne
column-only = Afficher seulement cette colonne
columns-show-all = Afficher toutes les colonnes
columns-choose = Choisir les colonnes…
columns-title = Colonnes affichées
columns-help = Numéros, lettres ou plages, dans l'ordre voulu.
columns-example = 3, 157, 10-20
columns-invalid = Pas de colonne « { $item } ».
columns-filter = Filtrer par nom
columns-shown = { $shown ->
    [one] { $shown } colonne affichée
   *[other] { $shown } colonnes affichées
} sur { $count }

## Status bar

table-size = { $rows ->
    [one] { $rows } ligne
   *[other] { $rows } lignes
} × { $cols ->
    [one] { $cols } colonne
   *[other] { $cols } colonnes
}
selection-one = 1 cellule sélectionnée
find-results = { $count ->
    [one] { $count } résultat
   *[other] { $count } résultats
} pour « { $text } »
delimiter = Séparateur
delimiter-comma = Virgule
delimiter-semicolon = Point-virgule
delimiter-tab = Tabulation
delimiter-pipe = Barre verticale
delimiter-space = Espace
delimiter-other = Autre
delimiter-poliris = Poliris
line-ending-lf = LF (Linux, macOS)
line-ending-crlf = CRLF (Windows)
line-ending-cr = CR (ancien Mac)
encoding-utf8-bom = UTF-8 avec BOM

## Inspector

inspector-cell = Cellule
inspector-close = Fermer l'inspecteur
inspector-column = Colonne { $letter }
inspector-chars = { $count ->
    [one] caractère
   *[other] caractères
}
inspector-words = { $count ->
    [one] mot
   *[other] mots
}
inspector-brs = { $count ->
    [one] balise BR
   *[other] balises BR
}
inspector-edit = Modifier
inspector-apply = Valider
inspector-copy = Copier
inspector-values = Valeurs
inspector-empty = Cellules vides
inspector-longest = Texte le plus long
inspector-longest-value = { $count } car.
inspector-formula = Formule : ={ $formula }
inspector-formula-hint = La cellule affiche le résultat enregistré dans le classeur : il n'est pas recalculé.
inspector-truncated = Seuls les { $count } premiers caractères sont affichés ; Modifier ouvre le texte entier.

## Files

untitled = Sans titre
untitled-file = sans titre.csv
filter-csv = Fichiers CSV
filter-all = Tous les fichiers
filter-supported = Fichiers CSV et classeurs
filter-workbooks = Classeurs (Excel, LibreOffice)
error-open = Impossible d'ouvrir { $file } : { $error }
error-save = Impossible d'enregistrer { $file } : { $error }
error-encoding = le caractère « { $character } » ({ $code }) en { $cell } ne peut pas s'écrire en Windows-1252. Modifiez-le, ou choisissez UTF-8 dans la barre d'état.

## Import

import-title = Importer { $file }
import-data-rows = { $count ->
    [one] { $count } ligne de données
   *[other] { $count } lignes de données
}
import-reading = Lecture du fichier…
import-too-large = trop grand pour être ouvert : { $rows } lignes × { $cols } colonnes
import-columns = { $count ->
    [one] { $count } colonne
   *[other] { $count } colonnes
}
import-header-line = en-tête sur la ligne { $line }
import-encoding = Jeu de caractères
import-start-line = À partir de la ligne
import-quote = Délimiteur de texte
import-quote-double = Guillemets
import-quote-single = Apostrophes
import-quote-none = Aucun
import-separators = Séparateurs
import-other = Autre
import-other-hint = Texte entre les champs, d'un ou plusieurs caractères (!# dans les fichiers Poliris)
import-header-hint = Fige la première ligne lue en titres
import-merge = Fusionner les séparateurs
import-merge-hint = Ignore les champs vides
import-trim = Supprimer les espaces
import-trim-hint = Début et fin de cellule
import-preview = Aperçu
import-columns-shown = Colonnes { $first } à { $last } sur { $count }
import-no-separator = Aucun séparateur actif : une seule colonne
import-detected = Détecté automatiquement : { $delimiter }, { $encoding }, fins de ligne { $endings }
import-invalid = { $count ->
    [one] { $count } suite d'octets illisible dans ce jeu de caractères, affichée �
   *[other] { $count } suites d'octets illisibles dans ce jeu de caractères, affichées �
}
import-open = Importer
import-sheet = Feuille
import-workbook = Classeur : valeurs seules (les formules donnent leur résultat enregistré), sans macros
import-workbook-too-large = trop grand pour être ouvert (trop de cellules, ou une partie qui se décompresse trop grande)
import-workbook-unreadable = ce n'est pas un classeur lisible ({ $error })
import-workbook-crashed = le fichier n'a pas pu être lu : il est peut-être abîmé

## Dialogs

save-changes-title = Enregistrer les modifications ?
save-changes-text = { $file } contient des modifications non enregistrées.
save-changes-save = Enregistrer
save-changes-discard = Ne pas enregistrer
overwrite-title = Enregistrer par-dessus { $file } ?
overwrite-text = La lecture de ce fichier a laissé de côté des éléments, que l'enregistrer par-dessus fera perdre :
overwrite-blank = { $count ->
    [one] { $count } ligne vide
   *[other] { $count } lignes vides
}
overwrite-trimmed = { $count ->
    [one] espaces supprimés autour de { $count } champ
   *[other] espaces supprimés autour de { $count } champs
}
overwrite-invalid = { $count ->
    [one] { $count } suite d'octets illisible, enregistrée �
   *[other] { $count } suites d'octets illisibles, enregistrées �
}
overwrite-save = Enregistrer par-dessus
overwrite-save-as = Enregistrer sous…
cancel = Annuler
ok = OK
about-text = Un éditeur simple de fichiers CSV.
about-license = Logiciel libre sous licence MIT ou Apache-2.0.
about-third-party = Licences tierces
