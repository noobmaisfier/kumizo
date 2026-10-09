# Référentiel WidgetKit — pièges GPUI, erreurs à ne pas répéter

Notes concrètes accumulées en faisant compiler et tourner ce projet contre
`gpui`. Le moteur est **épinglé** sur le snapshot crates.io `gpui-pre` `=0.3.8`
(snapshot de `zed@279fe07`, nom de lib `gpui`), et non plus sur la branche
`main` de Zed. Tout ce qui suit a été **vérifié** soit par le compilateur, soit
en lisant la source de gpui, soit à l'écran.

GPUI n'a pas d'API stable : chaque snapshot peut casser l'API. On épingle donc
une version **exacte**, et on ne bouge que par un bump volontaire.

### Procédure de bump (`gpui-pre`)

1. Choisir la nouvelle version `gpui-pre` (`=X.Y.Z`).
2. Mettre à jour **ensemble** `gpui` et `gpui_platform` dans le `Cargo.toml` du
   workspace (`package = "gpui-pre"` / `"gpui-pre-platform"`, même `=X.Y.Z`).
3. `cargo update -p gpui-pre -p gpui-pre-platform` puis
   `cargo check --workspace --all-targets`.
4. Corriger les nouveaux pièges d'API (ce document).
5. Règle : **une seule** version `gpui-pre` dans tout le graphe de dépendances —
   sinon cargo compile deux GPUI et leurs types cessent d'être interchangeables.

---

## 1. Le piège n°1 : `primary_display()` ne marche pas sur Wayland

**Symptôme.** Une barre layer-shell centrée et coupée (800 px au lieu de la
largeur de l'écran), alors que le code de calcul de taille est correct.

**Cause.** Sur Wayland, gpui renvoie `None` *inconditionnellement* :

```rust
// gpui-pre-linux-0.3.8/src/linux/wayland/client.rs:1208 (snapshot épinglé,
// ~/.cargo/registry/src/*/gpui-pre-linux-0.3.8/...)
pub(crate) fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
    None
}
```

`displays()`, en revanche, est implémenté et expose `bounds()`.

**Règle.** Pour connaître la taille d'un écran, toujours passer par
`displays()`, jamais par `primary_display()`.

```rust
let bounds = cx.displays().first().map(|d| d.bounds());
```

Piège subtil associé : les globals `wl_output` arrivent **asynchrone** après le
démarrage de la boucle. Dans la closure de démarrage, `displays()` est encore
vide. Il faut attendre :

```rust
async fn wait_for_displays(cx: &mut gpui::AsyncApp) {
    for _ in 0..100 {
        if cx.update(|cx| !cx.displays().is_empty()) {
            return;
        }
        cx.background_executor().timer(Duration::from_millis(20)).await;
    }
}
```

> J'ai d'abord « corrigé » le mauvais endroit : j'attendais les displays mais
> lisais ensuite `primary_display()`, toujours `None`. L'attente servait à rien.
> Lire la source de gpui avant de deviner coûte 2 minutes.

---

## 2. Les pièges de compilation

### `Application::new()` n'existe plus

Le point d'entrée passe par la crate `gpui_platform` (à ajouter en dépendance,
même source git que `gpui`) :

```rust
gpui_platform::application()
    .with_quit_mode(gpui::QuitMode::Explicit)
    .run(|cx: &mut gpui::App| { /* ... */ });
```

### `QuitMode::Default` tue le daemon

Sur Linux, le mode par défaut **quitte l'application dès que la dernière fenêtre
est fermée** :

```rust
// crates/gpui/src/app.rs:3060
let quit_on_empty = match cx.quit_mode {
    QuitMode::Explicit => false,
    QuitMode::LastWindowClosed => true,
    QuitMode::Default => cfg!(not(target_os = "macos")),   // <- true sur Linux
};
if quit_on_empty && cx.windows.is_empty() {
    cx.quit();
}
```

Un shell qui perd une fenêtre doit survivre. Toujours forcer `Explicit`.

### `Context<'_, V>` — le lifetime est explicite

```rust
impl<V: Render> ServiceContextExt<V> for Context<'_, V> { /* ... */ }
```

Sans le `'_`, erreur `E0726: implicit elided lifetime not allowed here`.

### `cx.spawn` prend 1 ou 2 arguments selon le receiver

C'est le piège le plus coûteux, parce que l'erreur est un `E0282` trompeur
(« type annotations needed ») qui pointe sur le paramètre de la closure.

```rust
// Sur Context : 2 arguments, le premier est un WeakEntity
cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
    this.update(cx, |entity, cx| { /* ... */ }).ok();
});

// Sur App / AsyncApp : 1 argument
cx.spawn(async move |cx| { /* ... */ });
```

Et surtout : **c'est une closure `async`, pas une closure qui retourne un bloc
`async`**. Les deux écritures se ressemblent, seule la seconde marche.

```rust
// NE MARCHE PAS (E0282)
cx.spawn(|this, mut cx| async move { /* ... */ });

// MARCHE
cx.spawn(async move |this, cx| { /* ... */ });
```

### `this.update(cx, …)`, pas `this.update(&mut cx, …)`

`cx` est déjà `&mut AsyncApp`. Écrire `&mut cx` donne `&mut &mut AsyncApp` et
casse l'inférence de `C: AppContext`.

### `on_click` prend 3 arguments et exige un élément *stateful*

```rust
fn on_click(self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self
```

Le 2ᵉ argument est l'ancien `cx`. C'est lui qui permet d'updater une entité :

```rust
.on_click(move |_, _window, cx| {
    entity.update(cx, |e, _| { /* ... */ });   // cx : &mut App
})
```

Et `on_click` n'existe que sur `Stateful<E>` (implémenté pour `Stateful<E>`
seulement), **pas** sur un `div()` nu. Il faut un `.id(...)` avant :

```rust
div()
    .id("battery")          // sans ça : no method named `on_click`
    .on_click(move |_, _window, _cx| { /* ... */ })
```

`impl<T: Into<SharedString>> From<(ElementId, T)> for ElementId` existe, donc
`.id(format!("workspace-{id}"))` fonctionne.

### Le système de style n'a pas de `scale`

`Styled` ne propose **pas** `.scale(1.08)`. Il n'y a aucun champ transform dans
`Style`. Supprimer l'effet, ou l'animer autrement.

`hover` prend `StyleRefinement` **par valeur** :

```rust
.hover(|s| s.bg(rgb(0x313244)))   // |s: StyleRefinement| -> StyleRefinement
```

`StyleRefinement` est généré par le derive `Refineable` sur `Style`, il n'est pas
déclaré explicitement : le chercher avec `grep 'pub struct StyleRefinement'`
échoue, il faut chercher `impl Styled for StyleRefinement`.

### `hover style already set` — Ne pas appeler `.hover(...)` deux fois

GPUI panique inconditionnellement au runtime (`hover style already set`) si la
méthode `.hover(...)` est appelée plusieurs fois sur la même vue ou le même
`div()` (par exemple si une fonction utilitaire comme `tile(id)` pré-définit
déjà un `.hover(...)` et qu'un appelant y réapplique un `.hover(...)`).

**Règle.** Garder les helpers d'éléments de base neutres (sans hover par défaut),
ou réserver l'appel à `.hover(...)` au niveau terminal du composant.

### `WindowOptions` : 17 champs, pas 10

Et `WindowBackground` n'existe plus, c'est `WindowBackgroundAppearance` :

```rust
use gpui::{WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions};

WindowOptions {
    titlebar: None,
    window_background: WindowBackgroundAppearance::Transparent,
    kind: WindowKind::LayerShell(layer_shell::LayerShellOptions {
        namespace: "wk-topbar".into(),
        anchor: layer_shell::Anchor::TOP | layer_shell::Anchor::LEFT | layer_shell::Anchor::RIGHT,
        exclusive_zone: Some(px(36.)),
        keyboard_interactivity: layer_shell::KeyboardInteractivity::None,
        ..Default::default()
    }),
    ..Default::default()   // indispensable, sinon E0063
}
```

`WindowKind::LayerShell` est un variante **tuple** qui prend des options, et le
module `layer_shell` n'existe que sous
`#[cfg(all(target_os = "linux", feature = "wayland"))]`.

---

## 3. Fenêtres : qui implémente quoi

`Entity::update` exige `C: AppContext`. Les implémenteurs sont :

| Implémente `AppContext` | Ne l'implémente **pas** |
|---|---|
| `App`, `AsyncApp`, `AsyncWindowContext`, `HeadlessAppContext`, `TestAppContext`, `VisualTestContext`, `VisualTestAppContext` | **`Window`** |

Conséquences :

- Dans `Render::render(&mut self, window: &mut Window, cx: &mut Context<Self>)`,
  utiliser `cx` (le `Context`) et non `window`.
- `Entity::read` exige `&App` : `entity.read(cx)` fonctionne par déréférencement
  depuis `Context`, mais pas depuis `&mut Window`.

```rust
impl Entity<T> {
    pub fn read<'a>(&self, cx: &'a App) -> &'a T;
    pub fn update<R, C: AppContext>(&self, cx: &mut C, f: impl FnOnce(&mut T, &mut Context<T>) -> R) -> R;
}
```

`WeakEntity::update` renvoie `Result<R>` (l'entité a pu être libérée).

### Un handle ne prouve pas qu'une fenêtre existe

`remove_window()` marque la fenêtre morte, mais l'`AnyWindowHandle` qu'on
conserve n'est qu'un id : il survit à la fenêtre. Un `show` qui se contente de
faire `activate_window()` sur un handle périmé **répond « Success » sans rien
afficher**.

Toujours vérifier avant de réutiliser un handle :

```rust
let alive = cx.update(|cx| cx.windows().contains(&handle));
if alive {
    // ok
} else {
    // recréer la surface
}
```

C'est exactement le bug qui rendait `show` mensonger.

---

## 4. Cycle de vie des surfaces layer-shell

Une surface layer-shell **n'est pas étirée par le compositeur**. Sway respecte
les ancres et les marges pour la *position*, mais si le client demande 800 px il
obtient 800 px, centré. C'est au client de demander la largeur de l'output.

D'où la règle : si `anchor` contient à la fois `LEFT` et `RIGHT`, et qu'aucune
largeur explicite n'est donnée, demander `output_width - marge_g - marge_d`.

Hiérarchie de priorité à respecter dans `to_gpui_options_with_display` :

1. `width` explicite → toujours respecté
2. sinon, ancre gauche+droite → `bounds.width - marges`
3. sinon → défaut (800 px)

L'ordre 1 avant 2 est une correction : ignorer une largeur explicite sous
prétexte de « la barre s'étire » revient à interdire à l'utilisateur de
dimensionner son panneau.

Attention aussi : ne **pas** mettre les marges dans `origin`. Le compositeur
s'occupe du placement ; les marges seront comptées deux fois.

---

## 5. Erreurs qui n'étaient pas liées à GPUI

### Dépendances utilisées sans être déclarées

`cargo` ne les signale qu'à la compilation. Vérifié dans ce dépôt :

| Crate | Manquant |
|---|---|
| `wk-gpui-ext` | `anyhow` |
| `wk-compositor` | `anyhow` |
| `wk-ipc` | `anyhow` |
| `wk-services` | `async-channel` |

Le workspace les déclarait dans `[workspace.dependencies]`, mais un crate ne les
hérite que s'il les liste explicitement.

### Conflit d'emprunt dans le serveur IPC

```rust
let reader = BufReader::new(&sock);          // emprunt immuabletenu
for line in reader.lines() {
    sock.write_all(...)?;                     //.err[E0502] : emprunt mutable
}
```

`BufReader` garde `&sock` pendant toute la boucle. Solution : dupliquer le
descripteur.

```rust
let mut writer = sock.try_clone()?;   // fd dupliqué
let reader = BufReader::new(sock);     // consommé par le reader
```

### Le `UnixListener` doit être détenu par le propriétaire

Si seul le thread d'accept possède le listener, le socket est fermé quand ce
thread se termine : le daemon devient muet alors que le processus vit encore
(symptôme observé après ~20 min).

```rust
pub struct IpcServer {
    _listener: UnixListener,   // garde le socket vivant
    socket_path: PathBuf,
    is_running: Arc<AtomicBool>,
}
```

### Un seul socket, une seule instance

Le shell est **mono-instance** : un processus, un socket
(`$XDG_RUNTIME_DIR/widgetkit.sock`, surchargeable avec `WIDGETKIT_SOCKET`).
Historique du piège : `UnixListener::bind` **unlink** le fichier existant
avant de lier, donc deux daemons sur le même chemin se volaient le socket et
seul le dernier lancé répondait, silencieusement. L'ancienne parade
(`socket_path(nom)` + nom de daemon explicite) est supprimée.

Garde actuelle dans `IpcServer::bind` : si le fichier existe, on envoie un
`Ping`. Une réponse = une instance vivante → `bind` échoue (« already
running ») et `WidgetKit::run` quitte avec un message. Pas de réponse =
socket périmé (crash) → il est supprimé puis recréé. Ne jamais supprimer à
la main `widgetkit.sock` d'un shell qui tourne : il continuerait de vivre sans
être joignable.

Le lockscreen reste hors de ce schéma (`.ipc(false)`) : il ne lie aucun
socket, donc ne peut ni entrer en conflit avec le shell ni être piloté à
distance.

### zbus : se méfier de docs.rs

`Proxy::new` prend `&Connection`. Les docs de docs.rs affichent `Connection` et
la signature lisible induit en erreur. La source locale fait foi :

```bash
sed -n '570,590p' ~/.cargo/registry/src/*/zbus-*/src/proxy/mod.rs
```

De même, `receive_property_changed()` renvoie directement un `PropertyStream`,
pas un `Result` : ne pas écrire `.next()` sur un `Result`.

---

## 6. Erreurs de méthode — les miennes

À relire avant de conclure qu'un truc marche.

**J'ai déclaré « le topbar fonctionne » sur la foi de captures d'écran qui ne le
contenaient pas.** Ce que je mesurais était la barre de titre d'une fenêtre ou un
terminal. Un détecteur maison based sur « pixels sombres » validait n'importe
quoi de sombre.

Règles à appliquer :

1. **Une capture n'est une preuve que si l'objet recherché y est
   identifiable.** Avant de mesurer, confirmer visuellement que le widget est
   dans l'image.
2. **Le test fiable pour hide/show est la différence de pixels entre les deux
   états**, pas la présence d'une couleur. C'est ce test qui a finalement
  révélé la vérité (252 pixels qui diffèrent, exactement dans la plage du
   topbar).
3. **Une capture faite pendant que l'utilisateur est en plein écran ne montre
   rien.** Faire masquer les fenêtres concurrentes d'abord, ou demander.
4. **Ne pas conclure qu'un correctif a marché parce qu'il compile.** Ici, le
   code compilait, le daemon répondait `Success`… et rien ne s'affichait.
5. **L'utilisateur est l'observateur de référence.** Il avait raison de dire
   « je vois un topbar » quand mes captures disaient le contraire : la vérité
   était dans le test le plus rigorous.

---

## 7. Les outils

```bash
# lire la vraie API gpui, toujours (snapshot épinglé, pas le main amont)
grep -rn 'fn primary_display' ~/.cargo/registry/src/*/gpui-pre-linux-0.3.8/src/

# lire une dépendance locale plutôt que docs.rs
sed -n '570,590p' ~/.cargo/registry/src/*/zbus-*/src/proxy/mod.rs

# mesurer la conso idle (CPU en ticks de /proc, HZ=100)
python3 - <<'PY'
import time, os, re
HZ = os.sysconf('SC_CLK_TCK')
def cpu():
    for d in os.listdir('/proc'):
        if not d.isdigit(): continue
        try: cl = open(f'/proc/{d}/cmdline','rb').read().decode(errors='ignore')
        except OSError: continue
        if 'target/release/shell' in cl:
            s = open(f'/proc/{d}/stat').read()
            f = s[s.rindex(')')+2:].split()
            rss = int(open(f'/proc/{d}/statm').read().split()[1]) * os.sysconf('SC_PAGE_SIZE')
            return int(d), (int(f[11])+int(f[12]))/HZ, rss/2**20
    return None
a = cpu(); time.sleep(15); b = cpu()
if a and b:
    print(f"pid {b[0]}  CPU idle {(b[1]-a[1])/15*100:.2f}%  RSS {b[2]:.0f} MB")
PY

# PSS plutôt que RSS : les libs sont partagées entre processus
awk '/Pss/{s+=$2} END{printf "PSS total: %.0f MB\n", s/1024}' /proc/*/smaps_rollup

# test hide/show par différence de pixels
./target/release/widgetctl hide topbar; grim a.png
./target/release/widgetctl show topbar; grim b.png
```

---

## 8. Coût réel mesuré sur cette machine

| | RSS | PSS |
|---|---|---|
| 1 widget en release | ~89 Mo | — |
| 3 widgets, 3 processus | 326 Mo | **210 Mo** |
| 3 widgets, 1 processus (`shell`) | **~89 Mo** | ~89 Mo |

Décomposition d'un process : binaire 41 Mo (debug) / libs 31 Mo / Mesa-Vulkan
30 Mo / **heap 7 Mo**.

Deux conclusions :

- La **somme des RSS tripl-compte** ~116 Mo de libs partagées. Utiliser PSS.
- Le poids est dans le runtime GPU + polices, pas dans le code. « C'est du Rust
  donc light » est vrai pour le heap (7 Mo), pas pour le framework.
- D'où l'intérêt du mono-processus : un seul init GPU/polices.

Sur l'idle CPU : `Clock` réveille une fois par seconde et redessine, d'où
~0,4 % CPU pour une barre qui affiche l'heure. Ce n'est pas du « zero idle CPU »
au sens strict — c'est 1 Hz.

---

## 9. Workspaces Sway : la liste doit suivre le compositeur

Deux bogues réels, trouvés parce que l'utilisateur a demandé si `1 2 8 10`
était toujours juste (ça ne l'était pas : Sway avait `1 2 3 4 9 10`).

**Bogue 1 — liste figée au démarrage.** `fetch_workspaces()` n'était appelé
qu'une fois, et `WorkspaceActivated` ne re-cochait que l'actif. Créer ou
détruire un workspace ne changeait rien à l'affichage. Le `8` affiché
n'existait plus : le chiffre venait du démarrage, pas du fallback (`1-5`).

**Fix (option a) :** `CompositorEvent::WorkspacesChanged(Vec<Workspace>)`
(`types.rs`). Le listener Sway ré-interroge le compositeur (`GET_WORKSPACES`)
sur `init`/`empty`/`destroy` et envoie la liste fraîche ; `compositor.rs`
**remplace** la liste au lieu de la patcher. Les flags `is_active` du fetch
sont adoptés (vérité synchrone du compositeur) et `active_workspace_id` est
re-synchronisé ; repli sur l'ancien id seulement si rien n'est marqué.

**Bogue 2 — le piège `focused: false` sur un événement `focus`.** Séquence
capturée en direct pour un retour `8 → 1` :

```
focus  name='1' focused=False   <- le quirk : ignoré par l'ancien code
window focus (container set)    <- à filtrer, sinon activation num=0
empty  name='8' focused=False
```

Sway émet `focus` avec `focused: false` quand le switch détruit aussi un
workspace. L'ancien code exigeait `focused == true` → activation perdue →
**plus aucun workspace surligné**. Règle : **le champ `change` est
l'autorité, pas le flag `focused` du snapshot.** Tout `change == "focus"`
avec `current.type == "workspace"` active `current.num`. Le filtre
`container.is_some()` (événements window) et `kind != "workspace"` restent
obligatoires — sans eux, un focus de fenêtre fabrique une activation `num=0`.

**Vérification (pixel, pas à l'œil) :** cycle `1 → 8 → 1` avec pilule bleue
(`b>180, b>r+40`) : base x=33-56 (le `1`), après création x=89-112 (le `8`),
après destruction x=33-56 (le `1`). Les trois états sont exacts.

### Batterie : écouter `State`, pas seulement `Percentage`

L'icône ne suivait pas le branchement du chargeur. Cause : `battery.rs`
n'écoutait que les changements UPower de `Percentage`. Brancher/débrancher
change `State` (1↔2), pas le niveau — donc aucun `cx.notify()`, icône figée,
alors que le widget (`battery.read(cx)` dans le render) était bien abonné.
De plus la boucle sysfs de secours (30 s) était inatteignable tant que le
flux D-Bus vivait. Fix : deux flux `receive_property_changed`
(`Percentage: f64`, `State: u32`) multiplexés par `select!`, avec
`BatteryState::from_upower` partagé entre lecture initiale et temps réel.
Vérifié : débranchement vu par l'utilisateur + retour à `Charging`
(capture et sysfs concordants, 38 % vert).

### Luminosité : service dédié, et le mute qui n'allumait pas l'OSD

**Le mute ne montrait aucun OSD.** L'observateur de `VolumeOsd` ne comparait
que le niveau (`volume == last_volume → return`) : un mute à volume
constant ne déclenchait rien. Fix : comparer aussi `is_muted`, et factoriser
le réveal + auto-hide (1,8 s) dans `reveal_osd()`. Vérifié en capture :
« Muted » rouge avec barre au clic.

**Service `Brightness`** (`crates/wk-services/src/brightness.rs`) sur le modèle
`Audio` : lecture sysfs (`/sys/class/backlight`, premier périphérique trié),
écriture via `brightnessctl set <pct>%` (permissions udev OK ici, sinon
`warn!`), mise à jour optimiste comme `switch_workspace`. Écoute 100%
événementielle (plus de sondage) : `sysfs_notify` sur `actual_brightness` +
uevents netlink `NETLINK_KOBJECT_UEVENT`, zéro CPU idle — voir `mod watch`
dans le source, qui fait foi. Repli : `WIDGETKIT_BRIGHTNESS_POLL_MS` pour les
drivers qui ne notifient jamais, sondage 2 s seulement si aucune source
d'événements n'existe.
Actions IPC : `up` / `down` (±5) / `set <pct>`.

L'OSD est partagé : `OsdKind { Volume, Brightness }`, la carte affiche
l'icône, le libellé, le % et la barre du dernier changement. Pas de pastille
topbar pour la luminosité (aucun toggle sensé au clic — ne pas mettre un
clic mort).

**Glyphes : la preuve par le rendu, encore.** Les codepoints MDI 7.4
`brightness-1..7` (`U+F00DA..F00E0`, CSS officiel) rendent des **phases de
lune** dans la Nerd Font installée (MDI plus ancien) — vérifié avec
`pango-view`. Icône unique `weather-sunny` (`U+F0599`, soleil confirmé au
rendu), le niveau étant porté par le % et la barre. Règle rappelée :
**la CSS officielle ne fait pas foi pour la police locale.**

Vérifié : `down` 33424 → 30146 sur le matériel, OSD « Brightness » soleil +
56 % + barre en capture, niveau restauré après test. Le binaire `osd`
autonome reçoit le même traitement (service + observateur + rendu).

### Wi-Fi : pas de lib unificatrice, deux backends natifs

Aucun crate Rust n'abstrait NetworkManager + iwd (`zbus` n'est que le
transport). Donc `WifiBackend { NetworkManager, Iwd, Fallback }` avec
auto-détection sur les noms D-Bus — même pattern que `CompositorKind`.
La méthode `iwctl ... | grep RSSI` d'un ami marchait, mais fork+exec,
parsing de texte humain et pas d'événements : le natif D-Bus donne les
mêmes chiffres (`Station.State`, `ConnectedNetwork → Network.Name`,
`StationDiagnostic.GetDiagnostics.RSSI`) avec des événements.
iwd n'a pas d'`ObjectManager` : la station est trouvée en parcourant
l'arbre `Introspect` (profondeur ≤ 4). Icônes : arcs MDI `wifi-strength-1..4`
(`U+F091F/F0922/F0925/F0928`) + `wifi-off` (`U+F05AA`), choisis par
l'utilisateur. Les codepoints **Nerd Font ne se devinent pas** : ils ont été
vérifiés en deux temps — rendu réel avec `pango-view` dans
`JetBrainsMono Nerd Font`, puis relecture des octets du fichier source
(`U+Fxxxx`) pour écarter toute substitution de caractère.
`display_label()` retourne un `String` car un `&str` emprunté au guard de
`read(cx)` ne peut pas s'échapper de `render` (E0521).
Backend NM désormais **testé live** (machine passée sous NetworkManager) :
la boucle NM est événementielle comme iwd (`MessageStream` sur
`org.freedesktop.NetworkManager`, même pattern que le Bluetooth), avec un
debounce de 750 ms pour coalescer les rafales de scan — pas de polling.
Nuance iwd : les signaux `State` disent connecté/déconnecté mais le RSSI
ne bouge pas tout seul — la boucle iwd relit donc le diagnostic toutes
les 15 s (sans ça, l'icône de signal reste figée quand on se déplace
avec le portable). Ce refresh est assumé et documenté dans le code
(`iwd_loop`), pas un oubli.
Pilotage UI + IPC : `networks()` (liste triée, actif d'abord),
`request_scan()`, `set_enabled()`, `connect()` (profils + réseaux ouverts),
`connect_with_password()` (PSK jamais stocké), `disconnect()` ;
IPC `scan`, `on`, `off`, `toggle`, `connect <ssid>`, `disconnect`.
Pilotage iwd (testé live, machine passée sous iwd + NM inactif) :
`Station.GetOrderedNetworks()` (signal en 1/100 dBm → même
`rssi_to_percent`, la tuile et la liste concordent), `Station.Scan()`
(résultats via le watch `Scanning` de la boucle),
`Network.Connect()` (connus + ouverts sans secret), `Station.Disconnect()`,
radio via `Adapter.Powered` (lecture + écriture, adaptateur = parent de
la station). Mot de passe : agent `net.connman.iwd.Agent` servi par le
shell (`IwdAgent`, `RegisterAgent` à l'init) — `connect_with_password`
dépose le PSK dans un slot one-shot, `RequestPassphrase` le consomme
après avoir vérifié le `Name` du réseau (pas de fuite vers un autre
SSID), le slot est vidé dans tous les cas. Sans secret déposé, l'agent
échoue proprement et `connect()` retourne `false` (la carte ouvre alors
la ligne mot de passe). 8021x/entreprise non implémenté volontairement :
méthodes absentes → `UnknownMethod` + échec net, documenté dans le code.
Détail vérifié live : `Network.Name` arrive avec un espace de bourrage
(`"DIONE FAMILY "`) — trimmé côté lecture comme côté liste, sinon la
tuile et le panneau divergent. Connect/disconnect iwd non testés par
l'auteur (couperait sa propre connexion) : à valider côté utilisateur.

### Tray SNI : watcher, pixmaps, dbusmenu — et le double déballage

Le daemon est l'hôte (`org.kde.StatusNotifierWatcher`) : il reçoit les
`RegisterStatusNotifierItem` et suit les items (titre, statut, icône, menu).
Icônes décodées depuis `IconPixmap` (`(i, i, ay)` en **ARGB**) vers le BGRA
attendu par GPUI, puis `img(RenderImage)`.

Trois pièges, tous rencontrés en vrai :

1. **Un item déjà lancé ne se ré-enregistre pas.** Au démarrage il faut
   scanner le bus pour les noms exposant l'interface, sinon Telegram/MEGAsync
   présents avant le shell restent invisibles.
2. **Ne pas partager la boucle `rx.recv()` avec l'écoute
   `NameOwnerChanged`.** Les deux sont des flux infinis ; les mettre dans le
   même `select!`/`while` parqué les enregistrements derrière un flux qui ne
   se réveille qu'à la mort d'un item. Deux tâches séparées.
3. **Le double déballage des enfants dbusmenu.** `av` (les enfants) est un
   tableau de variantes : selon que le nœud vient d'être déballé par
   `deserialize` (racine) ou reçu encore boxé (enfant), `TryFrom<Value>`
   reçoit `Value::Structure` **ou** `Value::Value(Structure)`. Ne gérer que le
   second cas faisait passer 3 enfants → **0 entrée** : la fenêtre s'ouvrait
   avec le titre et rien dedans. Le `TryFrom` accepte maintenant les deux.

**Symptôme à retenir :** menu vide avec le bon titre = parsing du layout à
zéro, pas un bug de l'application. Le layout brut se lit sans outil maison :

```sh
busctl --user get-property <bus> /StatusNotifierItem org.kde.StatusNotifierItem Menu
busctl --user call <bus> /MenuBar com.canonical.dbusmenu GetLayout iias 0 1 0
```

(`icon-data` est un **PNG encodé**, pas des pixels : `137 80 78 71` = `\x89PNG`.)

**`ContextMenu(x, y)` est implémenté.** La spec SNI prévoit que le panneau
appelle cette méthode avant de lire `Menu` (l'appli peut peupler son menu à
la demande). `open_menu` suit le flux complet : `ContextMenu` (ignoré si
inconnu), relecture de `Menu`, `AboutToShow`, `GetLayout`.

4. **Le scan de démarrage peut se geler sur une connexion morte.** Pour
   rattraper les items déjà lancés au démarrage, il faut sonder les noms
   uniques `:1.x` (une appli déjà en cours ne se ré-enregistre pas). Or
   **une seule connexion Qt qui ne répond jamais suffit à bloquer le scan** :
   `Introspect` attend le timeout D-Bus complet, soit **25 s mesurés**, et
   comme la boucle est séquentielle sur ~89 noms, les items suivants ne
   sont jamais atteints. Symptôme : plus aucune icône après un redémarrage
   du shell alors que les applies tournent. Fix : `with_timeout(3s, …)`
   autour de l'`Introspect` (`select` sur `async_io::Timer`).

**`AddMatch` ne suffit pas pour observer le bus.** Pour voir le trafic des
autres clients D-Bus il faut `BecomeMonitor`, pas seulement `AddMatch` —
j'ai perdu du temps sur cette piste. Et **un `Event` vers un id de menu
inexistant ne remonte aucune erreur** : Qt le traite sans valider l'id, donc
« aucune erreur » ne prouve pas que l'action a eu un effet visible. Ne pas
s'en servir comme test.

**Effet réel des entrées de menu : vérifié par l'utilisateur.** « Show
status » ouvre bien la page status de MEGAsync, « Settings » ouvre bien les
réglages. C'est la seule preuve fiable : un clic depuis l'interface, observé
par la personne, pas un log. Le parcours complet fonctionne donc — icône,
menu, `Event`, action.

### Les logs ne partaient nulle part

`tracing` était utilisé partout (`warn!`, `info!`) mais **aucun subscriber
n'était installé** : tout était silencieusement jeté. Deux diagnostics ont
été faits à l'aveugle à cause de ça. `WidgetKit::run` installe désormais
`tracing_subscriber::fmt().with_max_level(level)` ; `RUST_LOG=debug` monte le
niveau. C'est ce qui a permis de lire `Tray: menu …: 3 child(ren), 3 visible`
et de trancher le bogue ci-dessus en une minute.

---

## 10. Pièges de fenêtrage Wayland, UX et PAM

### Dimensionnement automatique et ancres Wayland (`LayerWindow`)

**Comportement.** `LayerWindow::new()` démarre avec des dimensions non définies (`width: None`, `height: None`), des ancres vides et aucune zone exclusive (`exclusive_zone: 0`).

**Règle.**
- Pour une **barre horizontale** : déclarer `.anchor(Anchor::TOP_BAR)` (ou `Anchor::TOP | Anchor::LEFT | Anchor::RIGHT`), `.height(32.0)` et `.exclusive_zone(32)`. La largeur s'étire automatiquement sur tout l'écran.
- Pour un **rail / panneau latéral** : déclarer `.anchor(Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM)`, `.width(72.0)` et `.exclusive_zone(72)`. La hauteur s'étire automatiquement sur tout l'écran car `height` est `None`.
- Pour un **plein écran / modal** (`Anchor::FULLSCREEN` ou toutes ancres combinées) : aucune dimension n'est requise, la surface couvre l'ensemble de l'écran par expansion naturelle des 4 ancres.
- Pour un **popup / OSD flottant** : définir explicitement `.width(...)` et `.height(...)` (ou `.size(...)`), sans ancres opposées.

```rust
// Exemple plein écran (propre, zéro bidouille de taille) :
LayerWindow::new("launcher")
    .layer(Layer::Overlay)
    .anchor(Anchor::FULLSCREEN)
    .start_hidden(true)
```

### Le debounce de l'auto-masquage OSD (Tokens de génération)

**Symptôme.** L'OSD de volume ou luminosité clignote ou disparaît brusquement
au milieu d'ajustements consécutifs au clavier.

**Cause.** Lancer un `Timer::after(...)` asynchrone à chaque appui sans annuler
le précédent fait que le premier timer expire à `t=2s` et masque la fenêtre,
même si l'utilisateur a appuyé à `t=1.8s`.

**Règle.** Utiliser un jeton de génération (`generation: u64`) incrémenté à chaque
ajustement. Le timer ne masque la fenêtre que si sa génération est toujours la plus
récente :

```rust
osd.generation = osd.generation.wrapping_add(1);
let current_gen = osd.generation;
cx.spawn(async move |this, cx| {
    async_io::Timer::after(Duration::from_millis(2000)).await;
    let _ = this.update(cx, |osd, cx| {
        if osd.generation == current_gen {
            osd.visible = false;
            cx.notify();
        }
    });
}).detach();
```

### Codes et sémantique Linux-PAM (`<security/pam_appl.h>`)

- `PAM_CRED_EXPIRED` = 16, `PAM_CRED_ERR` = 17, `PAM_CONV_ERR` = 19 (18 est
  `PAM_NO_MODULE_DATA`).
- `PAM_CRED_INSUFFICIENT` (8) indique un manque de privilèges du processus
  appelant (ex. lecture de `/etc/shadow`), jamais un mauvais mot de passe
  utilisateur.
- Mémoire `pam_conv(3)` : les réponses doivent être allouées via `libc::calloc`
  et `libc::strdup` car PAM les libère lui-même avec `free(3)`.

### Sources de données événementielles (`Source<T>`) — garanties et pièges

1. **`inotify` et tronquage de fichier** :
   Ne pas écouter `IN_MODIFY` ou `IN_CREATE` pour surveiller un fichier modifié par redirection shell (`echo val > fichier`). La redirection tronque le fichier avant d'écrire son contenu, générant un événement sur un fichier momentanément vide.
   **Règle** : Écouter `IN_CLOSE_WRITE | IN_MOVED_TO` pour ne relire le fichier que lorsque l'écrivain a terminé de fermer ou remplacer atomiquement le fichier. Pour les pseudo-fichiers noyau (`/proc`, `/sys`) qui ne lèvent aucun événement `inotify`, utiliser `.poll_every(...)`.

2. **Processus enfants et nettoyage des orphelins (`PR_SET_PDEATHSIG`)** :
   Tout processus enfant (`ProcessSource`) est configuré avec `libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM)` dans `pre_exec`. Si le processus principal subit un arrêt brutal (`SIGKILL`), le noyau Linux envoie automatiquement `SIGTERM` aux enfants pour ne laisser aucun script orphelin en arrière-plan.

3. **Inférence de type dans `cx.spawn` pour les entités génériques** :
   Sur une méthode générique `Source<T>`, `cx.spawn(async move |this, cx| ...)` échoue avec `E0282` (« type annotations needed »). Toujours annoter explicitement :
   `this: gpui::WeakEntity<Source<T>>, cx: &mut gpui::AsyncApp`.

---

## 11. Checklist avant de déclarer un changement fonctionnel

- [ ] `cargo check --workspace --all-targets` : 0 erreur, **0 warning**
- [ ] `cargo test --doc` passe (les exemples `no_run` compilent vraiment)
- [ ] le changement est **visible à l'écran**, vérifié par une capture nette
- [ ] pour hide/show : **différence de pixels** entre les deux états
- [ ] le daemon répond encore après le changement (`widgetctl ping`)
- [ ] les cas d'erreur sont testés (fenêtre inconnue, action inconnue, socket mort)
- [ ] pas de fichier de test parasite laissé dans `examples/`