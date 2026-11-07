#!/bin/sh
# Arranque del contenedor, con las convenciones de linuxserver.io:
#   PUID/PGID  usuario y grupo con los que corre Nebula (los dueños de tus carpetas en el NAS)
#   UMASK      permisos de lo que se crea (022: rwxr-xr-x; 002: también escribe el grupo)
#   TZ         zona horaria (Europe/Madrid…)
set -e

umask "${UMASK:-022}"

if [ "$(id -u)" = "0" ]; then
  PUID="${PUID:-1000}"
  PGID="${PGID:-1000}"
  mkdir -p /config /downloads
  # /config es de Nebula: se le da entero si no es ya suyo. En /downloads solo la carpeta en sí,
  # porque puede ser una carpeta compartida enorme.
  if [ "$(stat -c %u:%g /config)" != "$PUID:$PGID" ]; then
    chown -R "$PUID:$PGID" /config
  fi
  if [ "$(stat -c %u /downloads)" = "0" ]; then
    chown "$PUID:$PGID" /downloads 2>/dev/null || true
  fi
  echo "Nebula: PUID=$PUID PGID=$PGID UMASK=$(umask) TZ=${TZ:-Etc/UTC}"
  exec setpriv --reuid="$PUID" --regid="$PGID" --clear-groups --inh-caps=-all -- "$@"
fi

# Con `--user` o en Kubernetes ya no es root: se arranca tal cual.
exec "$@"
