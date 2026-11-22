#!/bin/sh
# Інсталятор лаунчера Altherra для Linux (glibc і musl).
#   sh Altherra-Setup-linux-x64.run              — встановити (з вікнами, якщо є zenity/kdialog)
#   sh Altherra-Setup-linux-x64.run --yes        — без запитань
#   sh Altherra-Setup-linux-x64.run --uninstall  — видалити
# Ставиться лише для поточного користувача, права root не потрібні.
set -eu

APP="@APP@"
ID="@ID@"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DEST="$DATA/$ID"
BIN_DIR="$HOME/.local/bin"
DESKTOP="$DATA/applications/$ID.desktop"
ICON="$DATA/icons/hicolor/256x256/apps/$ID.png"

YES=0
MODE=install
for a in "$@"; do
  case "$a" in
    -y|--yes) YES=1 ;;
    --uninstall) MODE=uninstall ;;
    -h|--help) sed -n '2,6p' "$0"; exit 0 ;;
  esac
done

# ---------- Повідомлення: вікно, якщо можна, інакше термінал ----------
GUI=""
if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
  if command -v zenity >/dev/null 2>&1; then GUI=zenity
  elif command -v kdialog >/dev/null 2>&1; then GUI=kdialog
  fi
fi

say() {
  case "$GUI" in
    zenity) zenity --info --title="$APP" --width=380 --text="$1" 2>/dev/null || true ;;
    kdialog) kdialog --title "$APP" --msgbox "$1" 2>/dev/null || true ;;
    *) printf '%s\n' "$1" ;;
  esac
}

fail() {
  case "$GUI" in
    zenity) zenity --error --title="$APP" --width=380 --text="$1" 2>/dev/null || true ;;
    kdialog) kdialog --title "$APP" --error "$1" 2>/dev/null || true ;;
    *) printf 'Помилка: %s\n' "$1" >&2 ;;
  esac
  exit 1
}

ask() {
  [ "$YES" = 1 ] && return 0
  case "$GUI" in
    zenity) zenity --question --title="$APP" --width=400 --ok-label="$2" --cancel-label="Скасувати" --text="$1" 2>/dev/null ;;
    kdialog) kdialog --title "$APP" --yes-label "$2" --no-label "Скасувати" --yesno "$1" 2>/dev/null ;;
    *)
      [ -t 0 ] || return 0
      printf '%s [Т/н] ' "$1"
      read -r ans || return 1
      case "$ans" in н|Н|n|N|ні|no) return 1 ;; *) return 0 ;; esac
      ;;
  esac
}

# ---------- Видалення ----------
if [ "$MODE" = uninstall ]; then
  ask "Видалити лаунчер $APP? Світи, налаштування й акаунти в теці гри лишаться." "Видалити" || exit 0
  rm -f "$BIN_DIR/$ID" "$DESKTOP" "$ICON"
  rm -rf "$DEST"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
  say "$APP видалено. Файли гри лишилися в ~/.altherra — їх можна видалити вручну."
  exit 0
fi

# ---------- Встановлення ----------
[ "$(uname -m)" = x86_64 ] || fail "Цей інсталятор лише для 64-бітних x86 систем."

if [ -e /lib/ld-musl-x86_64.so.1 ] || { command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; }; then
  LIBC=musl
else
  LIBC=gnu
fi

if [ -x "$DEST/$ID" ]; then
  ask "$APP уже встановлено. Оновити до нової версії?" "Оновити" || exit 0
else
  ask "Встановити лаунчер $APP?

Він зʼявиться в меню програм. Потрібна Java, Minecraft і моди лаунчер поставить сам." "Встановити" || exit 0
fi

TMP=$(mktemp -d "${TMPDIR:-/tmp}/altherra-setup.XXXXXX")
trap 'rm -rf "$TMP"' EXIT INT TERM

# Дані інсталятора — після рядка-маркера в кінці цього файлу.
LINE=$(awk '/^__ALTHERRA_PAYLOAD__$/ { print NR + 1; exit }' "$0")
[ -n "$LINE" ] || fail "Інсталятор пошкоджено. Скачай його ще раз."
tail -n +"$LINE" "$0" | tar xzf - -C "$TMP" || fail "Не вдалося розпакувати інсталятор. Скачай його ще раз."
[ -f "$TMP/$ID-$LIBC" ] || fail "В інсталяторі немає збірки для $LIBC."

mkdir -p "$DEST" "$BIN_DIR" "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
# Новий файл кладемо поруч і перейменовуємо: запущений лаунчер так не зламається.
cp "$TMP/$ID-$LIBC" "$DEST/.$ID.new"
chmod 755 "$DEST/.$ID.new"
mv -f "$DEST/.$ID.new" "$DEST/$ID"
cp "$TMP/icon.png" "$ICON"
# Для видалення достатньо самого скрипта, без вкладених файлів.
head -n "$((LINE - 1))" "$0" > "$DEST/uninstall.run"
ln -sf "$DEST/$ID" "$BIN_DIR/$ID"

cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=$APP
GenericName=Minecraft Launcher
Comment=Лаунчер сервера Altherra
Exec=$DEST/$ID
Icon=$ID
Terminal=false
Categories=Game;
Keywords=minecraft;altherra;launcher;
Actions=Uninstall;

[Desktop Action Uninstall]
Name=Видалити $APP
Exec=sh "$DEST/uninstall.run" --uninstall
EOF
chmod 644 "$DESKTOP"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$DATA/icons/hicolor" >/dev/null 2>&1 || true

if [ "$YES" = 1 ]; then
  printf 'Встановлено: %s (%s)\n' "$DEST/$ID" "$LIBC"
  exit 0
fi
if ask "$APP встановлено. Його можна знайти в меню програм.

Запустити зараз?" "Запустити"; then
  (cd "$HOME" && nohup "$DEST/$ID" >/dev/null 2>&1 &)
fi
exit 0
__ALTHERRA_PAYLOAD__
