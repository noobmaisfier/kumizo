#!/usr/bin/env bash
# Installe les BINAIRES précompilés d'aurora (barre + lockscreen + widgetctl).
#
#   curl -fsSL https://raw.githubusercontent.com/noobmaisfier/kumizo/main/install-aurora-bin.sh | sh
#
# Options (flags ou env) :
#   --version vX.Y.Z   version à installer (défaut : dernière release, env AURORA_VERSION)
#   --to DIR           destination (défaut : ~/.local/bin)
#   --no-sway          ne touche pas à la config sway
#
# Quitte proprement si la release n'existe pas encore pour la version
# demandée (les binaires sont publiés par GitHub Actions à chaque tag `v*`).

set -eu

REPO="noobmaisfier/kumizo"
API="https://api.github.com/repos/$REPO"
VER="${AURORA_VERSION:-latest}"
DEST="${HOME}/.local/bin"
NO_SWAY=0

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VER="${2:?}"; shift 2 ;;
        --to) DEST="${2:?}"; shift 2 ;;
        --no-sway) NO_SWAY=1; shift ;;
        -h|--help)
            echo "usage: install-aurora-bin.sh [--version vX.Y.Z] [--to DIR] [--no-sway]";
            exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

say() { printf '%s\n' "$*"; }
die() { printf 'ERREUR : %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "il manque '$1'"; }

need curl
need tar
need sha256sum

# -- Version ---------------------------------------------------------------
if [ "$VER" = "latest" ]; then
    VER=$(curl -fsSL "$API/releases/latest" \
        | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4) \
        || die "impossible de joindre l'API GitHub (aucune release ?)"
    [ -n "$VER" ] || die "aucune release trouvée sur $REPO"
fi
say "version : $VER"

# -- Téléchargement + vérification ------------------------------------------
tgz="aurora-$VER-linux-x86_64.tar.gz"
base="https://github.com/$REPO/releases/download/$VER/$tgz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fSL -o "$tmp/$tgz" "$base" || die "téléchargement impossible.

  $base
  La release n'existe pas (encore) pour $VER : les binaires sont publiés
  par GitHub Actions à chaque tag v*. En attendant, installe depuis
  les sources :
  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install-aurora.sh | sh"
curl -fsSL -o "$tmp/$tgz.sha256" "$base.sha256" \
    || die "somme de contrôle introuvable pour $VER"
(cd "$tmp" && sha256sum -c "$tgz.sha256") || die "somme de contrôle invalide"

# -- Installation -----------------------------------------------------------
mkdir -p "$DEST"
tar -xzf "$tmp/$tgz" -C "$DEST"
chmod +x "$DEST/aurora" "$DEST/widgetctl" "$DEST/aurora-lockscreen"
say "installé dans $DEST : aurora, widgetctl, aurora-lockscreen"

# -- Sway (sauvegardé avant, idempotent) -------------------------------------
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

# -- Avertissements (non bloquants) ------------------------------------------
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

say "OK. Lance (ou recharge) sway, puis : widgetctl ping"
