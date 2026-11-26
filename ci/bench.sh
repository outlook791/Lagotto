#!/bin/sh
# Banco di prova di Lagotto: crea immagini di dischi con file noti, ne
# cancella una parte, recupera con `lagotto` e misura.
# Controlla anche la regola numero uno: l'immagine sorgente non cambia.
#
#   ci/bench.sh path/to/lagotto
#
# Serve: mtools, dosfstools, e2fsprogs, sleuthkit, testdisk, ImageMagick.
set -eu
LAGOTTO=$(readlink -f "${1:?usage: ci/bench.sh path/to/lagotto}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
export XDG_CONFIG_HOME="$work/config"
"$LAGOTTO" --accept-risk >/dev/null

# File noti: foto vere (JPEG/PNG), un PDF finto ma valido, un testo.
mkdir src
i=0
for spec in "640x480 plasma:fractal" "800x600 gradient:red-blue" "1024x768 plasma:" "320x240 gradient:green-yellow" "500x500 plasma:"; do
    set -- $spec
    convert -seed $i -size "$1" "$2" "src/IMG_000$i.jpg"
    i=$((i + 1))
done
convert -size 300x200 gradient:navy-white src/schema.png
printf '%%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%%%EOF\n' > src/contratto.pdf
seq 1 2000 | sed 's/^/riga /' > src/appunti.txt
( cd src && sha256sum * | sort ) > expected.sha

fail=0
report() { printf '%-8s %s\n' "$1" "$2"; }

check() { # check <nome> <immagine> [opzioni di lagotto]
    name=$1; img=$2; shift 2
    chmod 0444 "$img"
    before=$(sha256sum "$img" | cut -d' ' -f1)
    rm -rf "out-$name"; mkdir "out-$name"
    "$LAGOTTO" recover "$img" "out-$name" "$@" >"log-$name" 2>&1 || { cat "log-$name"; fail=1; return; }
    after=$(sha256sum "$img" | cut -d' ' -f1)
    if [ "$before" != "$after" ]; then
        report "$name" "FAIL: the source image changed"; fail=1; return
    fi
    # Quanti dei file cancellati sono tornati identici (contenuto, non nome).
    find "out-$name" -type f -exec sha256sum {} \; | cut -d' ' -f1 | sort -u > "got-$name"
    total=0; same=0
    for h in $(cut -d' ' -f1 "deleted-$name"); do
        total=$((total + 1))
        grep -q "$h" "got-$name" && same=$((same + 1))
    done
    report "$name" "$same/$total deleted files recovered identical; source unchanged"
    [ "$same" -eq "$total" ] || fail=1
}

# FAT32 (chiavette, schede SD).
truncate -s 64M fat.img
mkfs.vfat -F 32 -n PROVA fat.img >/dev/null
mmd -i fat.img ::/DCIM ::/Documenti
for f in src/*.jpg src/*.png; do mcopy -i fat.img "$f" ::/DCIM/; done
mcopy -i fat.img src/contratto.pdf src/appunti.txt ::/Documenti/
mdel -i fat.img ::/DCIM/IMG_0000.jpg ::/DCIM/IMG_0002.jpg ::/DCIM/schema.png ::/Documenti/contratto.pdf
( cd src && sha256sum IMG_0000.jpg IMG_0002.jpg schema.png contratto.pdf ) > deleted-fat
check fat fat.img

# FAT32 formattata di nuovo: i nomi sono persi, serve la ricerca senza nomi.
cp fat.img formatted.img; chmod 0644 formatted.img
mkfs.vfat -F 32 -n NUOVA formatted.img >/dev/null
( cd src && sha256sum IMG_0001.jpg IMG_0003.jpg IMG_0004.jpg appunti.txt ) > deleted-formatted
check formatted formatted.img --carve

# ext4 (dischi Linux): riempito senza montare, poi cancellazione con debugfs.
# Attenzione: debugfs cancella in modo più "gentile" del sistema vero
# (non azzera la mappa dei blocchi), quindi questo caso è più facile della
# realtà. Serve a provare la catena, non a misurare ext4.
truncate -s 64M ext4.img
mkfs.ext4 -q -F -d src ext4.img
for f in IMG_0001.jpg appunti.txt; do debugfs -w -R "rm /$f" ext4.img >/dev/null 2>&1; done
( cd src && sha256sum IMG_0001.jpg appunti.txt ) > deleted-ext4
check ext4 ext4.img || true

exit $fail
