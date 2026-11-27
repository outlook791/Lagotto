#!/bin/sh
# Firma il pacchetto RPM e prepara il repository di Lagotto (per dnf e Discover).
# Uso (in Fedora, dopo scripts/build-rpm.sh):
#   RPM_SIGNING_KEY="$(cat chiave-privata.asc)" scripts/sign-and-repo.sh
# Risultato: dist/*.rpm firmati e site/rpm/ pronto per GitHub Pages.
set -eu
cd "$(dirname "$0")/.."
if [ -z "${RPM_SIGNING_KEY:-}" ]; then
    echo "::error::The signing key is missing (secret RPM_SIGNING_KEY). / Manca la chiave di firma (secret RPM_SIGNING_KEY)." >&2
    exit 1
fi
dnf -y -q install rpm-sign createrepo_c gnupg2 >/dev/null
export GNUPGHOME
GNUPGHOME=$(mktemp -d)
trap 'rm -rf "$GNUPGHOME"' EXIT
printf '%s\n' "$RPM_SIGNING_KEY" | gpg --batch --quiet --import
fpr=$(gpg --batch --with-colons --list-secret-keys | awk -F: '/^fpr/ {print $10; exit}')
rpmsign --define "_gpg_name $fpr" --addsign dist/*.rpm >/dev/null

site=site/rpm
rm -rf site
mkdir -p "$site"
gpg --batch --armor --export "$fpr" > "$site/RPM-GPG-KEY-lagotto"
# Controllo: la firma si verifica con la chiave pubblica che pubblichiamo.
check=$(mktemp -d)
rpm --dbpath "$check" --initdb
rpm --dbpath "$check" --import "$site/RPM-GPG-KEY-lagotto"
for f in dist/*.rpm; do
    rpm --dbpath "$check" -K "$f" | grep -qiE 'signatures OK|pgp.*OK|digests signatures OK' || {
        echo "::error::Signature check failed for $f / Firma non valida per $f" >&2
        rpm --dbpath "$check" -Kv "$f" >&2
        exit 1
    }
done
rm -rf "$check"

cp dist/*.rpm "$site/"
cp packaging/lagotto.repo "$site/lagotto.repo"
createrepo_c --quiet "$site"
cat > site/index.html <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>Lagotto</title>
<p>Lagotto packages for Fedora. See <a href="https://github.com/outlook791/Lagotto#readme">the guide</a>.</p>
<p>Pacchetti di Lagotto per Fedora. Vedi <a href="https://github.com/outlook791/Lagotto/blob/main/README.it.md">la guida</a>.</p>
HTML
touch site/.nojekyll
ls "$site"
