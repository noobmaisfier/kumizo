# Note — état du dépôt (25 oct. 2026)

## Où on en est
- Branche de travail : **`main`** (GPUI). La migration `iced` est **en pause** :
  la branche `iced-migration` existe toujours, elle n'est ni fusionnée ni
  poussée.
- Les binaires installés dans `~/.local/bin` (`aurora`, `aurora-lockscreen`,
  `kumizo`) sont la version **GPUI** de `main` (builds du 24 oct. 2026 ;
  `kumizo` du 22 oct.). Le code de `main` est publié en **0.11.0** sur
  crates.io ; les binaires locaux n'ont pas été réinstallés depuis.
- Les binaires *iced* sont sauvegardés en `~/.local/bin/*.iced.bak` : retour
  arrière possible (recette dans le § « Déploiement »).

## 25 oct. 2026 — 0.11.0 : trois paquets au lieu de neuf
- `crates/` supprimé : `wk-ui`, `wk-gpui-ext`, `wk-services`, `wk-compositor`,
  `wk-ipc`, `wk-pam` deviennent des **modules** de `kumizo-kit`
  (`kumizo_kit::{ui,ext,services,compositor,ipc,pam}`), chacun derrière une
  feature (`ipc`, `ext`, `ui`, `services`, `compositor`, `pam`, `app`,
  `layer-shell`).
- Nouvelle disposition : `kumizo-kit/` (SDK), `kumizo-cli/` (package `kumizo`),
  `aurora/` (package `kumizo-aurora`, bins `aurora` + `aurora-lockscreen`
  inchangés).
- Le CLI dépend de `kumizo-kit` avec `default-features = false, features =
  ["ipc"]` : `cargo tree -p kumizo | grep gpui` ne renvoie rien (ni GPUI, ni
  wgpu, ni zbus).
- Version workspace **0.11.0** ; publication crates.io dans l'ordre
  `kumizo-kit` → `kumizo` → `kumizo-aurora` (vérifié : `cargo install --locked
  kumizo` puis `kumizo --help`).
- Les anciens crates `wk-*` **0.1.0** et `kumizo-kit` **0.1.0** ont été
  **yankés** le 25 oct. 2026 (réversible via `cargo yank --undo`). Il ne reste
  donc que `kumizo-kit 0.11.0`, `kumizo 0.11.0`, `kumizo-aurora 0.11.0`.
- Installation du shell : `cargo install --locked --root ~/.local
  kumizo-aurora kumizo` (~15 min, GPUI en release) ; `~/.local/bin` parce que
  les dotfiles sway appellent les binaires en absolu. Plus de binaire
  pré-compilé ni de `curl` dans l'installeur du rice (id-86 v1.13).
- CI (GitHub + GitLab) réduite à la seule tâche `check` : plus de build de
  binaires, plus de release GitHub/GitLab. `install-aurora.sh` **supprimé** :
  `cargo install` est le seul chemin d'installation supporté depuis 0.11.0.
  `cargo test --workspace --lib` : 24 tests verts.

## Historique — migration GPUI → iced (18 oct. 2026, suspendue)
- Migration GPUI → iced complète.
- SDK = `kumizo` (moteur + services + `lock`) · `kumizo-ui` (widgets optionnels
  + champ texte, publiables) · `kumizo_kit::ipc` · `kumizo_kit::pam` · `kumizo-cli`.
- 14 services (dont `compositor` joignable en CLI : `kumizo service compositor …`).
- Commandes **typées** sur tous les services (`Audio::toggle_mute()`,
  `Brightness::set(40)`, `Network::connect(ssid)`, …) — plus de
  `command("…").unwrap()`.
- Fenêtres `reactive` : OSD et toasts s'affichent d'eux-mêmes (modèle +
  subscriptions conservés cachés).
- Lockscreen réel (`ext-session-lock-v1`) via `kumizo::lock`, PAM.
- Lanceur : toutes les apps + vraies icônes ; panneau Tasks : icônes ;
  barre : icône + titre de la fenêtre active (l'`active_window` Sway a été
  dérivé de la liste des fenêtres).
- `cargo check --workspace --all-targets` : 0 erreur / 0 warning ; tests verts.

## En attente (à reprendre sur `iced-migration`)
1. **nami-shell** — shell tiers écrit depuis le GUIDE (archive
   `~/Downloads/nami-shell.tar.gz`, extrait dans `/tmp/opencode/nami-shell`).
   Compile après 4 corrections d'API, tourne (barre, panneau Réglages, lanceur).
   À améliorer : icônes d'apps dans le lanceur, retirer le plafond `.take(8)`.
2. **GUIDE — préciser les types** qui ont fait deviner nami :
   - `Network::ssid() -> Option<&str>` (pas `&str`) ;
   - `Apps::apps()`, `Notifications::toasts()/history()` renvoient des **slices** ;
   - `Apps::launch_id(id)` (le nom `launch` est pris par la méthode d'instance) ;
   - `TextBuffer::value()` (pas `text()`).
3. **Publier / fusionner** `iced-migration` dans `main` quand prêt (jamais fait).

## Reprendre le travail
- Revenir à l'état iced : `git switch iced-migration`
- Rebuild + installer :
  `cargo build --release -p kumizo-aurora -p kumizo` puis copier dans `~/.local/bin`
  (astuce `cp` + `mv` si le binaire tourne déjà).
- Tester sans toucher la session :
  `KUMIZO_SOCKET=/tmp/opencode/x.sock WAYLAND_DISPLAY=wayland-1 ./target/debug/aurora`
- Redémarrer le shell : `pkill -x aurora` puis relancer détaché
  (JAMAIS `pkill -f` avec un motif qui matche sa propre ligne de commande).

## 22 oct. 2026 — barre sans date, lecteur de musique sorti du shell (branche `main`)
- **Top bar** : l'heure seule (plus de date à côté). La pastille horloge
  ouvre toujours le centre de notifications.
- **Lecteur retiré du shell** : `aurora/src/music/` supprimé (avec
  `tests/rockbox_probe.rs`), fenêtre `aurora-music`, service `music`,
  `MUSIC_ENABLED`, et les deps `rockbox-playback` / `lofty` / `resvg`.
  Conséquence : le binaire `aurora` redevient **MIT OR Apache-2.0**
  (il était GPL-2.0 à cause de `rockbox-playback`).
- **Pastille média = MPRIS seul** (navigateur, lecteur externe). Plus de
  branche « lecteur local ».
- `cargo check --workspace --all-targets` : 0 erreur / 0 warning ; tests verts.
- Docs mises à jour : README racine, README d'aurora, `install-aurora.sh`
  (ce dernier a été supprimé le 25 oct. 2026, voir plus haut).

### RAM dans la barre (22 oct. 2026, ~14h36)
- Nouveau service SDK **`Memory`** (`kumizo-kit/src/services/memory.rs`) :
  `/proc/meminfo` relu toutes les 2 s sur l'executor d'arrière-plan,
  `cx.notify()` uniquement quand une valeur change (zéro redessin inutile).
  API : `percent()`, `used()`, `available()`, `total()`, `used_gib()`,
  `total_gib()`, `read_meminfo()`.
- Barre d'aurora : cellule **RAM juste avant la batterie** (icône microchip
  + `4.0/5.7G` = usage/total en Gio, `warn` à 75 %, `danger` à 90 %).
- Release rebuild (1 min 07 s) + réinstallation + redémarrage du shell :
  vérifié en vrai (`4.2G`, cohérent avec `free -h`).

### Déploiement dans la session (22 oct. 2026, ~14h26)
- `cargo build --release -p kumizo-aurora -p kumizo` (6 min 37 s) puis remplacement
  de `~/.local/bin/{aurora,aurora-lockscreen,kumizo}` (cp + mv : le binaire
  tourne, on ne peut pas l'écraser).
- **Retour arrière** : les binaires *iced* sont sauvegardés dans
  `~/.local/bin/{aurora,aurora-lockscreen,kumizo}.iced.bak` — pour revenir :
  `pkill -x aurora` puis `cp ~/.local/bin/aurora.iced.bak ~/.local/bin/aurora`
  (+ les 2 autres) et relancer.
- Config sway : `~/.config/sway/bindings.conf` — binding `Mod+m`
  (`kumizo toggle aurora-music`) supprimé (fenêtre disparue), backup
  `bindings.conf.bak_20261022-141030`. `swaymsg reload` appliqué.
- Vérifié en vrai : barre = heure seule (14:27), pastille MPRIS active
  (Chromium/Instagram), lanceur et centre de contrôle OK via IPC.

## Lecteur séparé — EN PAUSE (`~/.iCode/kumizo-music`)
- Prototype **fonctionnel** : process à part, socket IPC dédié
  (`$XDG_RUNTIME_DIR/kumizo-music.sock`), serveur MPRIS
  `org.mpris.MediaPlayer2.kumizo` (validé en vrai : `playerctl -l` liste
  `kumizo`, aller/retour play/metadata/volume OK).
- Licence : GPL-2.0-or-later (via `rockbox-playback`) — le SDK reste
  MIT/Apache.
- **Suspendu** : probablement abandonné pour l'instant. Le shell n'en dépend
  pas (rien dlié). Reprise = suivi du point 4 du § « En attente ».

## Convention
- Plus de commits automatiques : on commit seulement sur demande explicite.
