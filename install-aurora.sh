#!/usr/bin/env bash
# Installe aurora (barre + lockscreen + kumizo).
#
#   curl -fsSL https://raw.githubusercontent.com/noobmaisfier/kumizo/main/install-aurora.sh | sh
#
# Par défaut : binaire précompilé de la dernière release si disponible,
# sinon compilation automatique depuis les sources (~10 min).
#
# Options (flags ou env) :
#   --bin            binaire uniquement (échoue s'il n'existe pas)
#   --source         sources uniquement (compile, ~10 min la 1re fois)
#   --version vX.Y.Z version du binaire (défaut : dernière release, env AURORA_VERSION)
#   --ref REF        branche/tag à compiler (défaut : main, env AURORA_REF)
#   --src DIR        dossier du dépôt (défaut : ./ si c'est le dépôt, sinon ~/src/kumizo)
#   --to DIR         destination des binaires (défaut : ~/.local/bin)
#   --sway           écrit aussi dans la config sway (sauvegardée avant).
#                    Par défaut le script NE MODIFIE AUCUN fichier de config :
#                    il affiche juste les 2 lignes à ajouter.
#   --check          vérifie seulement les prérequis, sans rien installer

set -eu

REPO="noobmaisfier/kumizo"
API="https://api.github.com/repos/$REPO"
MODE="auto"
VER="${AURORA_VERSION:-latest}"
REF="${AURORA_REF:-main}"
SRC=""
DEST="${HOME}/.local/bin"
DO_SWAY=0
CHECK=0

while [ $# -gt 0 ]; do
    case "$1" in
        --bin) [ "$MODE" = "source" ] && { echo "conflit : --bin avec --source/--ref/--src" >&2; exit 2; }; MODE="bin"; shift ;;
        --source) [ "$MODE" = "bin" ] && { echo "conflit : --source avec --bin/--version" >&2; exit 2; }; MODE="source"; shift ;;
        --version) [ "$MODE" = "source" ] && { echo "conflit : --version avec --source" >&2; exit 2; }; VER="${2:?}"; MODE="bin"; shift 2 ;;
        --ref) [ "$MODE" = "bin" ] && { echo "conflit : --ref avec --bin/--version" >&2; exit 2; }; REF="${2:?}"; [ "$MODE" = "auto" ] && MODE="source"; shift 2 ;;
        --src) [ "$MODE" = "bin" ] && { echo "conflit : --src avec --bin/--version" >&2; exit 2; }; SRC="${2:?}"; [ "$MODE" = "auto" ] && MODE="source"; shift 2 ;;
        --to) DEST="${2:?}"; shift 2 ;;
        --sway) DO_SWAY=1; shift ;;
        --check) CHECK=1; shift ;;
        -h|--help)
            echo "usage: install-aurora.sh [--bin] [--source] [--version vX.Y.Z] [--ref REF] [--src DIR] [--to DIR] [--sway] [--check]";
            exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done
[ "$MODE" = "bin" ] && [ -n "${AURORA_REF:-}" ] && { echo "conflit : --bin avec AURORA_REF" >&2; exit 2; }

say() { printf '%s\n' "$*"; }
die() { printf 'ERREUR : %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# -- Prérequis --------------------------------------------------------------
# Paquets de compilation (noms Debian ; cf. .github/workflows/ci.yml).
APT_PKGS="build-essential pkg-config libpam0g-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libasound2-dev git curl"
missing=""
for c in curl tar sha256sum git gcc pkg-config; do
    have "$c" || missing="$missing $c"
done
if ! have cargo; then
    if [ "$CHECK" -eq 1 ] || [ "$MODE" = "bin" ]; then
        missing="$missing cargo(rustup)"
    else
        say "Rust absent : installation via rustup…"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --default-toolchain stable --profile minimal \
            || die "rustup a échoué"
        export PATH="${HOME}/.cargo/bin:${PATH}"
    fi
fi
if [ -f /etc/debian_version ] && have apt-get; then
    for p in $APT_PKGS; do
        dpkg -s "$p" >/dev/null 2>&1 || missing="$missing(apt:$p)"
    done
    if [ -n "$missing" ] && echo "$missing" | grep -q "apt:"; then
        if [ "$CHECK" -eq 1 ] || [ "$MODE" = "bin" ]; then
            : # simple constat en mode binaire / check
        else
            say "paquets manquants, installation via sudo apt-get…"
            sudo apt-get update -qq && sudo apt-get install -y -qq $APT_PKGS \
                || die "apt-get a échoué"
            missing=$(echo "$missing" | tr ' ' '\n' | grep -v "apt:" | tr '\n' ' ')
        fi
    fi
elif [ "$CHECK" -eq 1 ]; then
    say "hors Debian : installe l'équivalent de : $APT_PKGS"
fi
if [ -n "$missing" ]; then
    # En mode auto, un manque côté compilation n'empêche pas le binaire.
    if [ "$MODE" = "source" ] || [ "$CHECK" -eq 1 ]; then
        die "prérequis manquants :$missing"
    fi
fi
[ "$CHECK" -eq 1 ] && { say "prérequis OK"; exit 0; }
[ -z "$missing" ] && say "prérequis OK"

# -- Binaire -----------------------------------------------------------------
try_bin() {
    local ver="$VER"
    if [ "$ver" = "latest" ]; then
        ver=$(curl -fsSL "$API/releases/latest" \
            | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4) || return 1
        [ -n "$ver" ] || return 1
    fi
    say "binaire : version $ver"
    local tgz="aurora-$ver-linux-x86_64.tar.gz"
    local base="https://github.com/$REPO/releases/download/$ver/$tgz"
    local tmp
    tmp=$(mktemp -d)
    curl -fSL -o "$tmp/$tgz" "$base" 2>/dev/null || { rm -rf "$tmp"; return 1; }
    curl -fsSL -o "$tmp/$tgz.sha256" "$base.sha256" \
        || { rm -rf "$tmp"; die "somme de contrôle introuvable pour $ver"; }
    (cd "$tmp" && sha256sum -c "$tgz.sha256") \
        || { rm -rf "$tmp"; die "somme de contrôle invalide"; }
    mkdir -p "$DEST"
    tar -xzf "$tmp/$tgz" -C "$DEST"
    chmod +x "$DEST/aurora" "$DEST/kumizo" "$DEST/aurora-lockscreen"
    rm -rf "$tmp"
    say "installé dans $DEST : aurora, aurora-lockscreen, kumizo (binaire $ver)"
    return 0
}

# -- Sources -------------------------------------------------------------------
build_source() {
    say "compilation depuis les sources ($REF)"
    if [ -z "$SRC" ]; then
        if [ -f ./aurora/Cargo.toml ]; then
            SRC="$PWD"
        else
            SRC="${HOME}/src/kumizo"
        fi
    fi
    if [ ! -f "$SRC/aurora/Cargo.toml" ]; then
        say "clone dans $SRC…"
        mkdir -p "$(dirname "$SRC")"
        git clone --depth 1 --branch "$REF" \
            "https://github.com/$REPO.git" "$SRC" \
            || die "clone impossible"
    fi
    cd "$SRC"
    say "dépôt : $SRC"
    say "compilation (long la première fois)…"
    cargo build --release -p kumizo-aurora -p kumizo || die "cargo build a échoué"
    mkdir -p "$DEST"
    cp target/release/aurora target/release/aurora-lockscreen target/release/kumizo "$DEST/"
    chmod +x "$DEST/aurora" "$DEST/aurora-lockscreen" "$DEST/kumizo"
    say "installé dans $DEST : aurora, aurora-lockscreen, kumizo (compilé)"
}

# -- Installation ---------------------------------------------------------------
case "$MODE" in
    bin)
        try_bin || die "pas de binaire pour $VER (les binaires sont publiés
par GitHub Actions à chaque tag v*). Relance avec --source pour compiler."
        ;;
    source)
        build_source
        ;;
    auto)
        if try_bin; then
            :
        else
            say "pas de binaire dispo : bascule sur la compilation ($REF)…"
            build_source
        fi
        ;;
esac

# -- Sway : affiché par défaut, écrit seulement avec --sway --------------------
if [ "$DO_SWAY" -eq 0 ]; then
    say "ajoute à ta config sway :"
    say '  exec ~/.local/bin/aurora'
    say "(ou relance avec --sway pour les ajouter automatiquement)"
else
    cfg="${HOME}/.config/sway/config"
    if [ ! -f "$cfg" ]; then
        die "pas de config sway ($cfg) : crée-la puis ajoute à la main :
  exec ~/.local/bin/aurora"
    fi
    bak="${cfg}.bak_aurora_$(date +%Y%m%d-%H%M%S)"
    cp "$cfg" "$bak" && say "sauvegarde sway : $bak"
    grep -qF 'exec ~/.local/bin/aurora' "$cfg" \
        || echo 'exec ~/.local/bin/aurora' >> "$cfg"
    say "sway : lancement en place (recharge sway pour appliquer)"
fi

# -- Avertissements (non bloquants) ----------------------------------------------
case ":${PATH}:" in
    *":${HOME}/.local/bin:"*) ;;
    *) say "AVERTISSEMENT : ~/.local/bin pas dans le PATH" ;;
esac
fc-list 2>/dev/null | grep -qi "jetbrains.*nerd" \
    || say "AVERTISSEMENT : police JetBrainsMono Nerd Font absente (icônes)"
command -v playerctl >/dev/null 2>&1 \
    || say "AVERTISSEMENT : playerctl absent (pastille web)"
command -v pactl >/dev/null 2>&1 && command -v wpctl >/dev/null 2>&1 \
    || say "AVERTISSEMENT : ni pactl ni wpctl (service audio)"
[ "${XDG_SESSION_TYPE:-}" = "wayland" ] \
    || say "AVERTISSEMENT : session non-Wayland détectée (le shell exige Wayland+Sway)"

say "OK. Lance (ou recharge) sway, puis : kumizo ping"
