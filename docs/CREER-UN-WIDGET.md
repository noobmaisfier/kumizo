# Créer un widget WidgetKit — guide self-sufficient

Ce document suffit, à lui seul, pour produire n'importe quel widget du
dépôt `widgetkit-v1`. Il ne contient aucun exemple de widget : uniquement
la recette, les signatures exactes et les règles. Si une information manque
pour avancer, la chercher dans le code (chemins donnés), jamais l'inventer.

Prérequis avant toute chose (ce guide est volontairement autonome : aucun
autre document n'est nécessaire) :

1. Environnement : Linux + Wayland (Sway ici), Rust stable, `cargo`
   utilisable **hors-ligne** (`--offline`, voir §3 point 1).
2. Règle du dépôt : **ne jamais committer sans demande explicite.**
3. GPUI est **épinglé** sur le snapshot crates.io `gpui-pre` `=0.3.8`
   (voir le `Cargo.toml` du workspace et `docs/REFERENTIEL.md` § procédure
   de bump). Ne jamais lancer `cargo update -p gpui-pre
   -p gpui-pre-platform` hors d'un bump volontaire, cela casse la
   compilation. Ne jamais deviner une API GPUI de mémoire : vérifier
   dans les sources locales (`~/.cargo/registry/src/*/gpui-pre-*/`).

---

## 1. Les trois concepts (à comprendre avant d'écrire une ligne)

Un widget WidgetKit, c'est toujours la combinaison de trois choses :

1. **Une fenêtre** (`LayerWindow`) : une surface layer-shell déclarée dans
   `main()` via le builder `WidgetKit`. Position, taille, couche, marges.
2. **Une vue** : une struct Rust qui implémente `gpui::Render`. Sa méthode
   `render` construit l'arbre d'éléments à chaque frame.
3. **Un service** (optionnel mais le cas général) : une source de données
   réactive (`Clock`, `Battery`, `Audio`, `Brightness`, `Mpris`, `Network`, `Tray`,
   `Compositor`, ou un service sur mesure). Le service pousse les mises à
   jour, la vue se contente d'afficher.

Règle d'or : **`render` est une fonction pure de l'état.** Jamais d'appel
système, jamais de D-Bus, jamais de lecture de fichier dans `render`. Toute
donnée vient d'un champ de la struct ou d'un service lu via `entity.read(cx)`.
Le seul moyen de redessiner est `cx.notify()` (appel direct dans la vue, ou
dans la fermeture passée à `update`, ou émis par un service observé).

---

## 2. Recette pas à pas : un widget = une vue dans le shell unique

WidgetKit est **mono-instance** : un seul processus (un seul init GPU, une
seule pile de polices, un seul socket) héberge toutes les fenêtres et tous les
widgets. **Ne jamais créer un binaire par widget.** Un widget est une struct
Rust qui implémente `Render`, ajoutée au shell existant.

Séparer les rôles :

- **Widget** = vue GPUI (`impl Render`). Réutilisable, composable.
- **Fenêtre** = surface layer-shell (`LayerWindow`) qui héberge *une* vue
  racine. Une vue racine peut contenir autant de widgets que voulu.
- **Shell** = le `main()` : il déclare services et fenêtres, une seule fois.

Exception volontaire : le **lockscreen** reste un binaire séparé (mots de
passe, clavier exclusif, aucun IPC). Ne pas le fusionner dans le shell.

### Étape 1 — Choisir où vit le widget

- Widget dans une fenêtre existante (ex : une pastille dans la barre) :
  écrire la vue et l'ajouter comme `.child(...)` de la vue racine de cette
  fenêtre. Aucune nouvelle fenêtre.
- Nouvelle surface (popup, panneau, OSD) : écrire la vue racine et déclarer
  une nouvelle fenêtre dans le `main()` du shell (étape 2).

Le shell de référence est `examples/shell/src/main.rs`. Pour démarrer un
shell personnel, copier ce crate (`examples/shell/Cargo.toml` donne les
dépendances) ; pour ajouter un widget au dépôt, modifier ce crate. Seule
dépendance nécessaire en plus si une vue utilise des timers :
`async-io = { workspace = true }`.

Dépendances disponibles au workspace (réutiliser, ne rien ajouter sans
raison) : `gpui`, `gpui_platform`, `zbus` (4.4, `async-io` uniquement),
`async-io`, `async-channel`, `futures-lite`, `futures-util` (`std` seul),
`serde` (+derive), `serde_json`, `image` (0.25, **PNG seul**),
`smallvec`, `time`, `thiserror`, `anyhow`, `tracing`, `log`, `bitflags`,
`libc`.

### Étape 2 — Le `main()` : services, fenêtres, `run()`

```rust
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Clock>()     // un appel par service, AVANT les fenêtres
        .window(
            LayerWindow::new("<id>")...,
            |cx: &mut gpui::App| cx.new(MaVue::new),
        )
        .window(                // autant de fenêtres que nécessaire, même processus
            LayerWindow::new("<popup>")....start_hidden(true),
            |cx: &mut gpui::App| cx.new(MonPopup::new),
        )
        .run();
}
```

Méthodes du builder (`crates/widgetkit/src/app.rs`), dans l'ordre d'appel
recommandé :

- `WidgetKit::new()` — point de départ.
- `.service::<S>()` — enregistre un service (`S: Service`). Appeler pour
  **chaque** service que les vues consomment, sinon `cx.service::<S>()`
  panique au démarrage avec un message explicite. L'ordre est indifférent.
  Les services sont instanciés **une fois** et partagés par toutes les
  fenêtres.
- `.window(config, builder)` — déclare une fenêtre nommée (`config.id`).
  Le `builder` reçoit `&mut gpui::App` et retourne `Entity<V>`
  (`V: Render`), typiquement `|cx| cx.new(MaVue::new)`.
- `.ipc(true/false)` — serveur IPC (défaut : activé). Désactivé seulement
  pour le lockscreen.
- `.run()` — ne retourne jamais. Initialise les services, lie le **socket
  unique** (`$XDG_RUNTIME_DIR/widgetkit.sock`), ouvre les fenêtres une fois
  qu'un écran est annoncé. Si une instance répond déjà sur ce socket, le
  second lancement affiche une erreur et **quitte** (pas de double shell).
  Un socket périmé après un crash est détecté et remplacé.


### Étape 3 — Configurer la fenêtre (`LayerWindow`)

`LayerWindow::new("<id>")` puis chaînage (chaque setter existe, voir
`crates/wk-gpui-ext/src/layer_window.rs`). `LayerWindow::new()` démarre avec des
valeurs neutres (ancres vides, zone exclusive 0, dimensions à `None`). Le shell configure
librement et explicitement ce dont il a besoin :

- `layer(Layer::Top)` — `Background | Bottom | Top | Overlay`. Barres et
  docks : `Top`. Popups et OSD : `Overlay` (au-dessus même du plein écran).
- `anchor(...)` — bitflags `TOP | BOTTOM | LEFT | RIGHT`, combinables avec
  `|`. Constantes : `TOP_BAR` (= `TOP|LEFT|RIGHT`), `BOTTOM_BAR`,
  `LEFT_BAR`, `RIGHT_BAR`, `FULLSCREEN`. **Opposés combinés = étirement** :
  une barre ancrée `LEFT|RIGHT` s'étire sur toute la largeur de l'écran ; un rail
  ancré `TOP|BOTTOM` s'étire sur toute la hauteur.
- `exclusive_zone(pixels)` — pixels réservés (les fenêtres maximisées sont
  repoussées). Barre/rail : la taille du panneau. Popup/OSD : `0` (défaut).
- `margins(Margins::new(top, right, bottom, left))` — ordre des arguments :
  haut, droite, bas, gauche. `Margins::zero()` (défaut), `Margins::all(v)`,
  `Margins::symmetric(horizontal, vertical)` existent aussi.
- `width(f32)` / `height(f32)` — dimensions explicites en pixels logiques.
  Quand non spécifié (`None`) : s'étire sur l'écran si des ancres opposées sont
  présentes (`LEFT|RIGHT` pour la largeur, `TOP|BOTTOM` pour la hauteur), sinon
  repli par défaut (800x32 px).
- `size(width, height)` — raccourci pour définir largeur et hauteur en un appel.
- `namespace("wk-...")` — défaut `wk-<id>`. Sert aux règles du compositeur
  (flou, ombres). Le garder préfixé `wk-`.
- `keyboard_interactivity(...)` — `None` (défaut, barres/docks),
  `OnDemand`, `Exclusive` (lanceurs, saisie).
- `monitor(MonitorTarget::Primary)` — ciblage d'écran (défaut : `Primary`).
- `transparent(true)` — fond transparent (défaut `true`).
- `start_hidden(true)` — **ouvre sans surface** : la fenêtre naît cachée,
  `show` la crée, `hide` la détruit. Pour les popups uniquement : une
  fenêtre cachée normalement garde sa surface et **mange les clics** de sa
  zone. Défaut `false`.

Configurations par cas (adapter les chiffres, garder la structure) :

- Barre haute : `layer(Top)`, `anchor(TOP_BAR)`, `exclusive_zone(hauteur)`,
  `height(hauteur)`, marges fines.
- Dock bas centré : `layer(Top)`, `anchor(BOTTOM)`, `exclusive_zone(hauteur)`,
  `width` + `height` explicites, marge basse.
- OSD/popup éphémère : `layer(Overlay)`, ancre du coin visé (`BOTTOM` seul
  = centré horizontalement ; `TOP | RIGHT` = coin haut-droit),
  `exclusive_zone(0)`, `width` + `height` explicites, marges de dégagement,
  `start_hidden(true)` pour un popup.
- Overlay / Modal plein écran (Launcher, Powermenu) : `layer(Overlay)`,
  `anchor(Anchor::FULLSCREEN)`, `exclusive_zone(0)`,
  `keyboard_interactivity(KeyboardInteractivity::Exclusive)`,
  `.size(None, None)` (**indispensable** pour annuler le `DEFAULT_HEIGHT` de 32 px
  et s'étendre sur la totalité de l'écran), `start_hidden(true)`.
- Panneau latéral : `LEFT_BAR` ou `RIGHT_BAR` + `width` explicite.

### Étape 4 — La vue : struct + `new` + `Render`

```rust
struct MaVue {
    clock: Entity<Clock>,   // un champ par service consommé
    visible: bool,           // état local éventuel
}

impl MaVue {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),  // panique si non enregistré (cf étape 2)
            visible: false,
        }
    }
}

impl Render for MaVue {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);  // abonne la vue : chaque notify() redessine
        div().child(clock.time_hms())
    }
}
```

Règles :

- `cx.service::<S>()` dans `new` **abonne automatiquement** la vue : tout
  `notify()` du service redessine. `try_service` existe pour ne pas paniquer.
- `entity.read(cx)` dans `render` donne `&S` (lecture seule). Pour écrire :
  `entity.update(cx, |s, cx| { s.methode_mut(); cx.notify(); })` —
  `update` ne notifie **pas** tout seul, l'appel à `cx.notify()` dans la
  fermeture est obligatoire, sinon rien ne redessine.
- `cx.observe(&entity, |vue, service, cx| { ... })` dans `new` pour réagir
  finement (ex : allumer un flag `visible` quand une valeur change puis
  l'éteindre après un délai). Terminer par `.detach()`.
- Timers dans une vue : `cx.spawn(async move |vue, cx| {
  async_io::Timer::after(Duration::from_...(..)).await;
  let _ = vue.update(cx, |v, cx| { ...; cx.notify(); }); }).detach();`
  (nécessite `async-io` dans le `Cargo.toml` du crate du shell).
- Les fermetures (`move ||`) prennent chaque `Entity` par clone séparé :
  cloner un `Entity` est gratuit, le compilateur l'exige par capture.
- `render` retourne **un seul type** : deux branches qui construisent des
  `div()` différents (avec/sans `.id()`) ne compilent pas ensemble.
  Uniformiser les branches.

### Étape 5 — Interagir : clics, survol, état

- Clic gauche : `.id("unique").cursor_pointer().on_click(|_, _, cx| {...})`.
  **`.id()` est obligatoire** : sans lui, `on_click` n'existe pas sur `Div`
  (erreur `E0599`). L'id doit être unique par élément.
- Clic droit (ou autre bouton) :
  `.on_mouse_down(gpui::MouseButton::Right, |_, _, cx| {...})`.
- Survol : `.hover(|style| style.bg(...))`.
- Champ texte : GPUI n'en fournit pas — utiliser `wk-ui` (partagé,
  headless) : `Entity<TextBuffer>` dans la vue (`TextBuffer::masked()`
  pour un mot de passe), touches routées via `b.handle_key(ev)`
  (`Submit`/`Cancel`/`Changed`/`Ignored`), dessin avec `text_input(...)`
  / `text_area(...)` et vos couleurs (`TextStyle`). Détail et règles
  dans `docs/SHELL.md` §7 « Text Fields ».
- Couleurs Catppuccin du dépôt (rester cohérent) : fond `rgb(0x1e1e2e)`,
  texte `rgb(0xcdd6f4)`, accent `rgb(0x89b4fa)`, vert `rgb(0xa6e3a1)`,
  rouge `rgb(0xf38ba8)`, jaune `rgb(0xf9e2af)`.
- Tailles : `px(13.0)`, `w(px(...))`, `h(px(...))`, espacements
  `gap_2()`, `px_3()`, `py_1()`, arrondis `rounded_md()` / `rounded_lg()`.

### Étape 6 — Texte : borner, jamais trancher

Un texte sans borne pousse le reste de la barre hors écran. Règles :

- Borner par la mise en page : `.max_w(px(...)).truncate()` sur le `div`
  qui porte le texte. `truncate()` = `overflow_hidden` + pas de retour
  ligne + `…` final, géré par GPUI au caractère près.
- **Ne jamais trancher une `String` par index d'octets**
  (`&s[..27]`) : ça coupe au milieu d'un caractère UTF-8 (accents, emoji)
  et ça panique ou corrompt. Si une limite en caractères est vraiment
  voulue, itérer sur `.chars()`, jamais sur les octets.
- Ordres de grandeur utilisés : titre média 260 px, titre de fenêtre 220 px.
- Barres de progression : conteneur `w_full().h(px(8.)).rounded_full()`
  + remplissage `.w(gpui::DefiniteLength::Fraction(ratio))`.

### Étape 7 — Images bitmap (icônes d'appli, pochettes, pixmaps)

GPUI peint des images via `gpui::img(source)`. Pour des pixels en mémoire :

- Construire un `image::ImageBuffer<image::Rgba<u8>, Vec<u8>>` avec
  `ImageBuffer::from_raw(largeur, hauteur, octets)`.
- **Convertir d'abord en BGRA** : GPUI attend les octets dans l'ordre
  B,G,R,A. Depuis du RGBA : `for px in octets.chunks_exact_mut(4) {
  px.swap(0, 2); }`. Depuis de l'ARGB (dbusmenu, tray) : inverser chaque
  mot de 4 octets.
- Envelopper : `Frame::new(buffer)` (`image::Frame`), puis
  `Arc::new(RenderImage::new(smallvec![frame]))` (`gpui::RenderImage`,
  `smallvec::smallvec!`). Afficher : `gpui::img(mon_arc.clone())`
  (dimensionner avec `.w()` / `.h()`, ex. 20 px pour une icône de barre).
- PNG et SVG :
  - **PNG** (fichier, `icon-data` dbusmenu) : `image::open(chemin)` puis `to_rgba8()`,
    inverser RGBA en BGRA.
  - **SVG** (icônes FreeDesktop) : parsé et rendu à la volée via `resvg` vers un
    `tiny_skia::Pixmap`, converti en BGRA, puis emballé dans un `RenderImage`.
    Mis en cache mémoire pour zéro coût lors des re-rendus.
- Glyphes d'icônes (police Nerd Font présente : JetBrainsMono) : utiliser
  **uniquement** des codepoints vérifiés — rendus une fois avec `pango-view`
  dans la police locale, puis relus en hexadécimal dans le source
  (`U+Fxxxx`) pour écarter toute substitution. Ne jamais deviner un
  codepoint. En cas de doute : texte ou formes de base.

### Étape 8 — Services : lire, écrire, réagir

Référence complète des services fournis (`crates/wk-services/src/`,
réexportés par `widgetkit::prelude::*`). Tous vivent dans des `Entity` et
notifient leurs observateurs via `cx.notify()`.

**`Clock`** — horloge. Méthodes : `time_hms()`, `time_hm()`,
`date_formatted()`, `to_rfc3339()`, `hour()`, `minute()`, `second()`,
`datetime()`, `set_resolution(ClockResolution::Seconds | Minutes)`.
Réveil exact à la seconde (ou minute) suivante, zéro drift.

**`Battery`** — batterie (UPower D-Bus + sysfs). Méthodes : `percent()`,
`state() -> BatteryState`, `is_charging()`, `energy_rate_watts()`,
`time_to_empty()`, `time_to_full()`, `read_sysfs()`.
`BatteryState` : `Charging | Discharging | Empty | FullyCharged |
PendingCharge | PendingDischarge | Unknown`, avec `as_str()` et
`icon(percent)` (glyphes de niveau + charge).

**`Audio`** — volume (PipeWire `wpctl`, repli PulseAudio `pactl`).
Méthodes : `volume()`, `is_muted()`, `sink_name()`, `icon()`,
`set_volume(pct)`, `increase_volume(pas)`, `decrease_volume(pas)`,
`toggle_mute()`, `query_state()`. Actions IPC : `toggle_mute`, `mute`,
`unmute`, `volume_up`, `volume_down`, `volume_set`.

**`Mpris`** — lecteur média (Spotify, Firefox, mpv…). Méthodes :
`title()`, `artist()`, `album()`, `display_label()` (`"artiste - titre"`),
`status() -> PlaybackStatus`, `player_name()`, `play_pause()`, `next()`,
`previous()`, `stop()`. `PlaybackStatus` : `Playing | Paused | Stopped`,
avec `icon()` et `is_playing()`. Actions IPC : `play_pause`,
`play-pause`, etc.

**`Network`** — Wi-Fi (NetworkManager, iwd, repli `/proc/net/wireless`,
auto-détection). Méthodes : `backend() -> WifiBackend`,
`ssid() -> Option<&str>`, `is_connected()`, `signal_percent()`,
`display_label() -> String` (SSID ou `"Disconnected"`), `icon()` (arcs
vérifiés). `WifiBackend` : `NetworkManager | Iwd | Fallback`.
Pilotage (NetworkManager + iwd, `false` + `warn!` sur le repli) :
`has_radio()` (faux seulement si NM sans périphérique wifi),
`device()` (périphérique NM, station iwd),
`networks() -> &[WifiNetwork]` (`ssid`, `strength`, `secured`,
`active`, triés actif d'abord), `wifi_enabled()` (`WirelessEnabled`
ou `Adapter.Powered`), `request_scan()`,
`set_enabled(on)`, `connect(ssid)` (profils/connus + réseaux ouverts),
`connect_with_password(ssid, password)` (PSK jamais stocké : profil
one-shot côté NM, agent passphrase one-shot côté iwd),
`disconnect()`. Actions IPC : `scan`, `on`, `off`, `toggle`,
`connect <ssid>`, `disconnect` (pas de mot de passe sur le CLI).
Boucles : NM 100 % événementielle (signaux + debounce 750 ms) ; iwd sur
signaux `State` + `Scanning` + refresh RSSI/liste 15 s en roaming (le
signal seul ne suit pas un déplacement, voir `docs/REFERENTIEL.md` §9).

**`Tray`** — hôte system tray (StatusNotifierItem + dbusmenu). Méthodes :
`items() -> Vec<TrayItem>` (icônes visibles, `Passive` exclu),
`menu() -> Option<OpenMenu>`,
`activate(id)`, `open_menu(id)`, `close_menu()`,
`click_menu_item(id)`. `TrayItem` : `id`, `title`, `icon:
Option<Arc<RenderImage>>`, `attention`. `TrayMenuItem` : `id`, `label`,
`enabled`, `separator`, `toggle: Option<bool>`, `children` (récursif).
`id` accepte la clé exacte, le nom de bus ou un fragment de titre.
Actions IPC : `list`, `activate`, `menu-open`, `menu-click`, `menu-close`.
Ne gère **aucune** fenêtre : la vue shell observe `menu()` et ouvre/ferme
elle-même sa fenêtre `traymenu` (`start_hidden`).

**`Compositor`** — Sway/Hyprland/Niri + repli (`CompositorKind`,
auto-détection, même pattern que `WifiBackend`). Méthodes :
`kind()`, `workspaces() -> &[Workspace]`, `active_workspace_id()`,
`active_window() -> Option<&ActiveWindow>`,
`windows() -> &[AppWindow]`, `focus_window(con_id: i64)`,
`switch_workspace(id)` (met le surlignage à jour de façon optimiste),
`close_window()`. `Workspace` : `id`, `name`, `is_active`, `is_urgent`,
`window_count`, `monitor`. `ActiveWindow` : `title`, `class`,
`workspace_id`. `AppWindow` : `id: i64`, `title`, `class`, `workspace_id`,
`workspace_name`, `is_focused`.

**`Brightness`** — luminosité d'écran 100% pilotée par événements noyau
(`sysfs_notify` sur `actual_brightness` + `NETLINK_KOBJECT_UEVENT`, zéro polling).
Méthodes : `percent()`, `icon()`, `device()`, `set_percent(pct)`, `increase(step)`,
`decrease(step)`.

**`Apps`** — catalogue d'applications FreeDesktop (`.desktop` de `/usr/share/applications`
et `~/.local/share/applications`). Résout et décode les icônes PNG et SVG vers le GPU.
Méthodes : `apps() -> &[AppInfo]`, `search(query) -> Vec<AppInfo>` (recherche
fuzzy avec scoring en temps réel), `find_icon(class_or_app_id) -> Option<Arc<RenderImage>>`,
`launch(app)`. `AppInfo` : `id`, `name`, `comment`,
`exec`, `icon: Option<Arc<RenderImage>>`, `categories`, `keywords`.

**`Notifications`** — serveur natif de notifications de bureau FreeDesktop (`org.freedesktop.Notifications`).
Remplace les démons externes (dunst, mako, swaync) dans le même processus.
Méthodes : `toasts() -> &[Notification]`, `history() -> &[Notification]`, `unread_count() -> usize`,
`is_dnd() -> bool`, `toggle_dnd()`, `dismiss_toast(id)`, `remove_history(id)`, `clear_history()`,
`invoke_action(id, key)`. `Notification` : `id`, `app_name`, `summary`, `body`,
`app_icon: Option<Arc<RenderImage>>`, `urgency: Urgency`, `actions: Vec<NotificationAction>`.
Ne gère **aucune** fenêtre : la vue shell observe `toasts()` et ouvre/ferme
elle-même sa fenêtre (voir les exemples `shell` et `aurora`), ce qui garde les
noms de fenêtres propres au shell.

**`Clipboard`** — gestionnaire de presse-papier Wayland natif via `wl-clipboard`.
Surveille les copies de texte, déduplique, conserve l'historique et gère la ré-injection.
Méthodes : `items() -> &[ClipboardItem]`, `search(query) -> Vec<ClipboardItem>`,
`copy_item(id)`, `copy_text(text)`, `remove_item(id)`, `clear()`.
`ClipboardItem` : `id`, `text`, `timestamp`, `preview()`, `line_count()`, `char_count()`.

### Étape 9 — Écrire un service sur mesure

Quand aucune source existante ne convient (nouveau capteur, nouveau
protocole), créer `crates/wk-services/src/<nom>.rs` :

```rust
// État : struct simple, Clone + Default si possible.
pub struct MonService { /* champs */ }
// Getters publics immuables pour les vues.
impl MonService {
    pub fn valeur(&self) -> ... { ... }
}
// Le trait : init lance la tâche de fond, ipc_action expose le contrôle CLI.
impl Service for MonService {
    fn init(cx: &mut Context<Self>) -> Self { ... }
    fn ipc_action(&mut self, cx: &mut Context<Self>, action: &str, payload: Option<&str>) -> bool { ... } // défaut: false
}
```

Contrat (voir `crates/wk-services/src/service.rs`) :

- `init` construit l'état initial **sans bloquer** (lecture sysfs ou
  valeur par défaut), puis `cx.spawn(async move |entité, cx| { ... }).detach()`
  avec la boucle d'écoute.
- La tâche de fond met à jour via `entité.update(cx, |s, cx| { s.champ =
  ...; cx.notify(); })`, **uniquement quand la valeur change** (comparer
  avant de notifier : chaque `notify()` redessine).
- Préférer les événements (flux D-Bus, `receive_property_changed`,
  `pactl subscribe`) au polling. Si polling il y a : cadence justifiée en
  commentaire, lecture bon marché (sysfs, un appel D-Bus), `notify()`
  conditionnel.
- `ipc_action` retourne `true` si l'action est reconnue (`widgetctl
  service <nom> <action> [payload]`), `false` sinon — jamais de succès
  silencieux. Le nom court sur le CLI est le nom du type en minuscules.
- Enregistrer le module dans `crates/wk-services/src/lib.rs` (`pub mod`
  + `pub use`) **et** dans `crates/widgetkit/src/prelude.rs`.
- Pour appeler du D-Bus depuis une méthode **synchrone** (clic) :
  `async_io::block_on(async { ... })` avec le code async habituel. Local
  et rapide ; ne jamais y mettre d'attente longue.
- Ne jamais laisser une boucle d'écoute en bloquer une autre : deux flux
  infinis (ex. enregistrements + surveillances) vivent dans **deux tâches
  séparées**, jamais dans le même `while`/`select`.
- Borner tout appel réseau/D-Bus qui peut pendre : timeout explicite
  (le timeout D-Bus par défaut fait 25 s mesurés — il gèle tout le reste).

### Étape 9 bis — Sources ad-hoc sans écrire de Service (`Source<T>`)

Écrire un `Service` complet est disproportionné si vous avez seulement besoin
d'une valeur ponctuelle (sortie d'un script bash, fichier `/sys`, ou messages
poussés sur un socket UNIX). Utilisez [`Source<T>`](file:///home/alamine/.iCode/kumizo/crates/wk-services/src/source.rs)
du prelude, qui s'encapsule dans une `Entity<Source<T>>` standard GPUI :

```rust
use widgetkit::prelude::*;

// 1. Processus : chaque ligne de stdout devient un T (auto-kill du processus avec PDEATHSIG)
let ticker = ProcessSource::new("sh")
    .args(["-c", "while true; do date +%s; sleep 1; done"])
    .restart_after(Duration::from_secs(1))
    .lines(cx, |line| Some(line.to_string()))
    .track(cx); // re-notifie la vue parente à chaque valeur

// 2. Fichier : relu dès qu'un script le modifie (inotify IN_CLOSE_WRITE + IN_MOVED_TO)
let temp = Source::watch_file(cx, "/tmp/temperature", |text| text.trim().parse::<i32>().ok())
    .track(cx);

// 3. Fichier noyau sans inotify (/proc, /sys) : ajouter un intervalle de poll
let cpu = FileSource::new("/proc/loadavg")
    .poll_every(Duration::from_secs(2))
    .watch(cx, |text| text.split_whitespace().next().map(str::to_string))
    .track(cx);

// 4. Socket UNIX : chaque ligne envoyée par un client devient un T
let inbox = Source::listen_unix(cx, "/tmp/mon-widget.sock", |line| (!line.is_empty()).then(|| line.to_string()))
    .track(cx);
```

Dans la vue (`render`) :
- Lecture : `let val = self.temp.read(cx).value();` (`Option<&T>`).
- Statut / Erreur : `self.temp.read(cx).error()` (`Option<&str>`) et `.state()` (`SourceState`).
- Compteur de mises à jour : `self.temp.read(cx).updates()` (`u64`).

**Nettoyage automatique** : quand l'entité `Source<T>` est détruite (ex. fermeture de la fenêtre), le processus enfant reçoit `SIGTERM` et le fichier de socket UNIX est supprimé du disque.

### Étape 10 — Fenêtres : afficher, cacher, contrôler

- Depuis un widget (cas général) : appel direct en mémoire, sans IPC,
  via le trait `WindowControlExt` du prelude :
  `.on_click(|_, _, cx| { cx.toggle_window("calendar"); })`, ou
  `cx.show_window("..")` / `cx.hide_window("..")`. `Context<V>` déréférence
  vers `App`, donc les mêmes méthodes marchent dans une vue. Les appels ne
  bloquent jamais l'appelant (thread dédié + timeout 5 s) ; un nom de fenêtre
  inconnu est journalisé en `warn!`.
- Pour un popup déclenché par un service (ex. le menu du tray) : le service
  ne touche pas aux fenêtres. La vue observe l'état du service
  (`cx.observe(&service, ...)`) et ouvre/ferme sa propre fenêtre
  (`cx.show_window("..")` / `cx.hide_window("..")`). Le service doit
  `cx.notify()` après chaque changement d'état, sinon l'observer ne se
  déclenche pas.
- Runtime (scripts, raccourcis du compositeur) :
  `widgetctl show|hide|toggle <fenêtre>`, `reload`, `status`. Un seul socket :
  `widgetkit.sock` dans `XDG_RUNTIME_DIR`.
- `hide` détruit la surface mais garde l'état (`Entity` + config) :
  `show` la recrée. `toggle` alterne selon l'état réel (handle vérifié
  contre les fenêtres vivantes, pas de succès fantôme).
- Pattern OSD (contenu éphémère dans une fenêtre permanente) : fenêtre
  toujours ouverte, champ `visible`, `render` vide quand caché, auto-hide
  par `Timer`. Réservé aux petites surfaces : une surface ouverte capte les
  clics de toute sa zone même transparente.

---

## 3. Vérification obligatoire (rien n'est « fini » avant)

Dans l'ordre, tout doit passer :

1. `cargo check --workspace --all-targets` : **0 erreur, 0 warning.**
   Les warnings sont traités comme des erreurs (imports, champs morts,
   durées de vie). `--offline` si le réseau est incertain.
2. `cargo test` concerné (`-p wk-services` pour la logique pure : parsing,
   conversions, seuils). Les fonctions pures se testent sans matériel.
3. `cargo build --workspace --release` (les binaires debug font ~600 Mo,
   les release ~40 Mo).
4. Redémarrer le shell (un seul processus) : tuer l'ancien
   (`pkill -x shell` ou PID exact — **jamais** `pkill -f` avec un motif
   présent dans sa propre ligne de commande, ça se suicide). Pas besoin de
   supprimer le socket : une instance morte est détectée et son socket
   remplacé, tandis qu'une instance vivante fait quitter le nouveau
   lancement (vérifier `widgetctl ping` avant de relancer). Relancer en
   détaché (`setsid ... &` + `disown`, pas un simple `&` qui meurt entre
   deux appels), attendre ~12 s (GPU + `wl_output`).
5. Capturer (`grim out.png`) et **regarder l'image** : vérifier que le
   widget y figure vraiment avant toute mesure (une capture en plein écran
   ou verrouillée ne montre rien et invalide tout test).
6. Mesurer au pixel, pas à l'œil : différence entre deux états
   (avant/après `hide`/`show`), position d'un surlignage, marge restante.
   Un script PNG pur (zlib + struct, pas de dépendance) suffit.
7. Logs : le daemon écrit sur stderr (`tracing`, niveau `RUST_LOG=debug`
   pour le détail). Si un comportement est douteux, lire le log avant de
   conclure — les services y annoncent backend, items suivis, menus lus.
8. Cas d'erreur : fenêtre inconnue, action inconnue, socket mort, bus
   absent, item qui disparaît — tester chaque repli, pas seulement le
   chemin nominal.

---

## 4. Référence rapide

**Builder** : `new → service* → window* → [ipc] → run`.

**`LayerWindow`** : `new(id) → layer → anchor → exclusive_zone →
margins → width/height → namespace → keyboard_interactivity → monitor →
transparent → start_hidden`.

**Contrôle de fenêtres depuis le code** : `cx.show_window(id)`,
`cx.hide_window(id)`, `cx.toggle_window(id)` (trait `WindowControlExt`,
dans le prelude).

**`widgetctl`** : `ping`, `status`, `show|hide|toggle <fenêtre>`, `reload`,
`service <nom> <action> [payload]`. Une seule instance, un seul socket :
aucune option `--socket`.

**Couleurs du dépôt** : fond `0x1e1e2e`, texte `0xcdd6f4`, accent
`0x89b4fa`, vert `0xa6e3a1`, rouge `0xf38ba8`, jaune `0xf9e2af`.

**Services et leurs méthodes** : voir §2 étape 8 (exhaustif à ce jour ;
si une méthode manque, c'est le code qui fait foi, pas ce guide).

---

## 5. Pièges GPUI et règles de méthode (sans ça, rien ne marche)

Tout ce qui suit a été appris à coups d'échecs réels sur ce dépôt. C'est
normatif, pas décoratif.

**API GPUI (vérifiée sur les sources locales `gpui-pre =0.3.8`, à
revérifier à chaque bump volontaire — voir `docs/REFERENTIEL.md`) :**

- `App::primary_display()` retourne `None` **inconditionnellement** sur
  Wayland (codé en dur côté `gpui-pre-linux`). Pour la taille d'un écran :
  `cx.displays()`. `LayerWindow` gère déjà ça : ne construire des
  `WindowOptions` à la main que si `LayerWindow` ne suffit pas, et dans ce
  cas tous les ~17 champs doivent être renseignés (`..Default::default()`
  pour le reste).
- `on_click` prend **3 arguments** (`|_, _, cx|`) et n'existe que sur un
  élément avec `.id()` (erreur `E0599` sinon). Idem pour tout handler
  nécessitant un état.
- `cx.spawn` prend une fermeture `async` à **2 arguments** (`|vue, cx|`).
  Les types `Context` portent une durée de vie explicite
  (`Context<'_, V>`).
- `Styled::scale` n'existe pas. Toute méthode d'effet visuel doit être
  vérifiée dans les sources locales avant usage.
- Toute dépendance utilisée doit être **déclarée** dans le `Cargo.toml`
  du crate qui l'emploie (`anyhow`, `async-channel`, etc.) : le workspace
  ne les propage pas.
- `QuitMode::Explicit` est vital pour un daemon (déjà géré par `run()`) :
  sans lui, fermer la dernière fenêtre tue le processus.
- `AnyWindowHandle` survit à `remove_window()` : un handle n'est pas une
  preuve de vie, toujours vérifier contre `cx.windows()` avant de s'en
  servir.

**Règles de méthode (s'appliquent à chaque widget) :**

- Preuve avant affirmation : une capture n'est une preuve que si le widget
  y figure **visiblement et identifiablement**. Une capture en plein écran,
  verrouillée ou vide invalide tout test — la refaire, pas l'interpréter.
- Le test fiable d'un afficher/cacher est la **différence de pixels entre
  les deux états**, pas la présence d'une couleur.
- Ne jamais conclure qu'un correctif marche parce qu'il compile, ni parce
  que le daemon répond `Success`. Seul l'effet observable compte.
- Ne jamais écrire de code silencieux : action inconnue → `false`, appel
  D-Bus en échec → `warn!`, repli documenté en commentaire.
- Ne jamais conclure depuis des logs absents : si un comportement est
  douteux et que le log est vide, c'est l'observabilité qui manque, pas la
  preuve que tout va bien (`RUST_LOG=debug`, stderr du daemon).
- L'utilisateur qui regarde l'écran est l'observateur de référence : en cas
  de désaccord avec une mesure automatique, c'est la mesure qu'on remet en
  cause d'abord.

## 6. En cas de doute

1. Relire la section correspondante ci-dessus (§5 d'abord).
2. Lire le code cité (chemins exacts dans ce guide) : le code fait foi,
   pas la mémoire.
3. Vérifier l'API amont dans les sources locales (`~/.cargo/registry/src/*/gpui-pre-*/`)
   plutôt que de mémoire : GPUI est un snapshot épinglé (`=0.3.8`), et la
   mémoire d'une autre version est fausse.
