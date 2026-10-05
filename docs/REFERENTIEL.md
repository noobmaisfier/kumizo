# Référentiel WidgetKit — pièges GPUI, erreurs à ne pas répéter

Notes concrètes accumulées en faisant compiler et tourner ce projet contre
`gpui` (commit `a846890`, version `0.2.2`). Tout ce qui suit a été **vérifié**
soit par le compilateur, soit en lisant la source de gpui, soit à l'écran.

Le projet ne compile pas « pour de vrai » tant que ces points ne sont pas
respectés : la crate n'a pas de version stable, l'API change à chaque commit de
`main`.

---

## 1. Le piège n°1 : `primary_display()` ne marche pas sur Wayland

**Symptôme.** Une barre layer-shell centrée et coupée (800 px au lieu de la
largeur de l'écran), alors que le code de calcul de taille est correct.

**Cause.** Sur Wayland, gpui renvoie `None` *inconditionnellement* :

```rust
// crates/gpui_linux/src/linux/wayland/client.rs:1208
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

### Un socket par daemon

`UnixListener::bind` **unlink** le fichier existant avant de lier. Deux daemons
non nommés sur le même chemin se volent donc le socket : seul le dernier lancé
répond, silencieusement. D'où `socket_path(nom)` et un nom de daemon explicite.

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
# lire la vraie API gpui, toujours
grep -rn 'fn primary_display' ~/.cargo/git/checkouts/zed-*/crates/gpui_linux/src/

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
./target/release/widgetctl --socket topbar hide topbar; grim a.png
./target/release/widgetctl --socket topbar show topbar; grim b.png
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

## 9. Checklist avant de déclarer un changement fonctionnel

- [ ] `cargo check --workspace --all-targets` : 0 erreur, **0 warning**
- [ ] `cargo test --doc` passe (les exemples `no_run` compilent vraiment)
- [ ] le changement est **visible à l'écran**, vérifié par une capture nette
- [ ] pour hide/show : **différence de pixels** entre les deux états
- [ ] le daemon répond encore après le changement (`widgetctl ping`)
- [ ] les cas d'erreur sont testés (fenêtre inconnue, action inconnue, socket mort)
- [ ] pas de fichier de test parasite laissé dans `examples/`