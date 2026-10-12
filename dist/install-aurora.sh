#!/usr/bin/env bash
# Installe aurora DEPUIS LES SOURCES (barre + lockscreen + widgetctl).
#
#   curl -fsSL https://raw.githubusercontent.com/noobmaisfier/kumizo/main/dist/install-aurora.sh | sh
#
# Options (flags ou env) :
#   --to DIR     destination des binaires (défaut : ~/.local/bin)
#   --no-sway    ne touche pas à la config sway
#   --check      vérifie seulement les prérequis, sans cloner ni compiler
#   --ref REF    branche/tag à compiler (défaut : main, env AURORA_REF)
#   --src DIR    dossier du dépôt (défaut : ./ si c'est le dépôt, sinon ~/src/kumizo)
#
# Debian/Ubuntu : les paquets de compilation sont installés via apt (sudo).
# Ailleurs : la liste des paquets est affichée, à installer à la main.
# Compte ~10 min pour la première compilation (codecs C de rockbox).

set -eu

REF="${AURORA_REF:-main}"
DEST="${HOME}/.local/bin"
NO_SWAY=0
CHECK=0
SRC=""

while [ $# -gt 0 ]; do
    case "$1" in
        --to) DEST="${2:?}"; shift 2 ;;
        --no-sway) NO_SWAY=1; shift ;;
        --check) CHECK=1; shift ;;
        --ref) REF="${2:?}"; shift 2 ;;
        --src) SRC="${2:?}"; shift 2 ;;
        -h|--help)
            echo "usage: install-aurora.sh [--to DIR] [--no-sway] [--check] [--ref REF] [--src DIR]";
            exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*"; }
die() { printf 'ERREUR : %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# -- Prérequis --------------------------------------------------------------
missing=""
for c in curl git gcc pkg-config; do
    have "$c" || missing="$missing $c"
done
if ! have cargo; then
    if [ "$CHECK" -eq 1 ]; then
        missing="$missing cargo(rustup)"
    else
        say "Rust absent : installation via rustup…"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --default-toolchain stable --profile minimal \
            || die "rustup a échoué"
        export PATH="${HOME}/.cargo/bin:${PATH}"
    fi
fi
have cargo || missing="$missing cargo(rustup)"

# Paquets de compilation (noms Debian ; cf. .github/workflows/ci.yml).
APT_PKGS="build-essential pkg-config libpam0g-dev libwayland-dev libxkbcommon-dev libfontconfig1-dev libasound2-dev git curl"
APT_MISSING=""
if [ -f /etc/debian_version ] && have apt-get; then
    for p in $APT_PKGS; do
        dpkg -s "$p" >/dev/null 2>&1 || APT_MISSING="$APT_MISSING $p"
    done
    if [ -n "$APT_MISSING" ]; then
        if [ "$CHECK" -eq 1 ]; then
            missing="$missing [apt:$APT_MISSING ]"
        else
            say "paquets manquants :$APT_MISSING"
            say "installation via sudo apt-get…"
            sudo apt-get update -qq && sudo apt-get install -y -qq $APT_MISSING \
                || die "apt-get a échoué"
        fi
    fi
elif [ "$CHECK" -eq 1 ]; then
    say "hors Debian : installe l'équivalent de : $APT_PKGS"
fi

if [ -n "$missing" ]; then
    die "prérequis manquants :$missing"
fi
say "prérequis OK"
[ "$CHECK" -eq 1 ] && exit 0

# -- Dépôt -------------------------------------------------------------------
if [ -z "$SRC" ]; then
    if [ -f ./examples/aurora/Cargo.toml ]; then
        SRC="$PWD"
    else
        SRC="${HOME}/src/kumizo"
    fi
fi
if [ ! -f "$SRC/examples/aurora/Cargo.toml" ]; then
    say "clone dans $SRC…"
    mkdir -p "$(dirname "$SRC")"
    git clone --depth 1 --branch "$REF" \
        https://github.com/noobmaisfier/kumizo.git "$SRC" \
        || die "clone impossible"
fi
cd "$SRC"
say "dépôt : $SRC"

# -- Compilation -------------------------------------------------------------
say "compilation (long la première fois)…"
cargo build --release -p aurora -p wk-ctl || die "cargo build a échoué"

# -- Installation -------------------------------------------------------------
mkdir -p "$DEST"
cp target/release/aurora target/release/aurora-lockscreen target/release/widgetctl "$DEST/"
chmod +x "$DEST/aurora" "$DEST/aurora-lockscreen" "$DEST/widgetctl"
say "installé dans $DEST : aurora, aurora-lockscreen, widgetctl"

# -- Sway (sauvegardé avant, idempotent) ---------------------------------------
if [ "$NO_SWAY" -eq 0 ]; then
    cfg="${HOME}/.config/sway/config"
    if [ ! -f "$cfg" ]; then
        say "pas de config sway ($cfg) : ajoute à la main :"
        say '  exec ~/.local/bin/aurora'
        say '  bindsym $mod+m exec ~/.local/bin/widgetctl toggle aurora-music'
    else
        bak="${cfg}.bak_aurora_$(date +%Y%m%d-%H%M%S)"
        cp "$cfg" "$bak" && say "sauvegarde sway : $bak"
        grep -qF 'exec ~/.local/bin/aurora' "$cfg" \
            || echo 'exec ~/.local/bin/aurora' >> "$cfg"
        grep -qF 'widgetctl toggle aurora-music' "$cfg" \
            || echo 'bindsym $mod+m exec ~/.local/bin/widgetctl toggle aurora-music' >> "$cfg"
        say "sway : lancement + Super+M en place (recharge sway pour appliquer)"
    fi
fi

case ":${PATH}:" in
    *":${HOME}/.local/bin:"*) ;;
    *) say "AVERTISSEMENT : ~/.local/bin pas dans le PATH" ;;
esac

say "OK. Lance (ou recharge) sway, puis : widgetctl ping"
