# Note — état du dépôt (22 oct. 2026)

## Où on en est
- Branche de travail : **`main`** (GPUI). La migration `iced` est **en pause** :
  la branche `iced-migration` existe toujours, elle n'est ni fusionnée ni
  poussée.
- Les binaires installés dans `~/.local/bin` (`aurora`, `aurora-lockscreen`,
  `kumizo`) sont la version **GPUI** de `main` (release du 22 oct. 2026).
- Les binaires *iced* sont sauvegardés en `~/.local/bin/*.iced.bak` : retour
  arrière possible (recette dans le § « Déploiement »).

## Historique — migration GPUI → iced (18 oct. 2026, suspendue)
- Migration GPUI → iced complète.
- SDK = `kumizo` (moteur + services + `lock`) · `kumizo-ui` (widgets optionnels
  + champ texte, publiables) · `wk-ipc` · `wk-pam` · `wk-ctl`.
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
  `cargo build --release -p aurora -p wk-ctl` puis copier dans `~/.local/bin`
  (astuce `cp` + `mv` si le binaire tourne déjà).
- Tester sans toucher la session :
  `KUMIZO_SOCKET=/tmp/opencode/x.sock WAYLAND_DISPLAY=wayland-1 ./target/debug/aurora`
- Redémarrer le shell : `pkill -x aurora` puis relancer détaché
  (JAMAIS `pkill -f` avec un motif qui matche sa propre ligne de commande).

## 22 oct. 2026 — barre sans date, lecteur de musique sorti du shell (branche `main`)- **Top bar** : l'heure seule (plus de date à côté). La pastille horloge
  ouvre toujours le centre de notifications.
- **Lecteur retiré du shell** : `examples/aurora/src/music/` supprimé (avec
  `tests/rockbox_probe.rs`), fenêtre `aurora-music`, service `music`,
  `MUSIC_ENABLED`, et les deps `rockbox-playback` / `lofty` / `resvg`.
  Conséquence : le binaire `aurora` redevient **MIT OR Apache-2.0**
  (il était GPL-2.0 à cause de `rockbox-playback`).
- **Pastille média = MPRIS seul** (navigateur, lecteur externe). Plus de
  branche « lecteur local ».
- `cargo check --workspace --all-targets` : 0 erreur / 0 warning ; tests verts.
- Docs mises à jour : README racine, README d'aurora, `install-aurora.sh`
  (plus de keybinding `Super+M` vers `aurora-music`).

### RAM dans la barre (22 oct. 2026, ~14h36)
- Nouveau service SDK **`Memory`** (`crates/wk-services/src/memory.rs`) :
  `/proc/meminfo` relu toutes les 2 s sur l'executor d'arrière-plan,
  `cx.notify()` uniquement quand une valeur change (zéro redessin inutile).
  API : `percent()`, `used()`, `available()`, `total()`, `used_gib()`,
  `total_gib()`, `read_meminfo()`.
- Barre d'aurora : cellule **RAM juste avant la batterie** (icône microchip
  + `4.0/5.7G` = usage/total en Gio, `warn` à 75 %, `danger` à 90 %).
- Release rebuild (1 min 07 s) + réinstallation + redémarrage du shell :
  vérifié en vrai (`4.2G`, cohérent avec `free -h`).

### Déploiement dans la session (22 oct. 2026, ~14h26)
- `cargo build --release -p aurora -p wk-ctl` (6 min 37 s) puis remplacement
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
