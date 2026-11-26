#!/bin/sh
# Compila Lagotto, esegue i test e il banco di prova.
set -eu
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cargo build --locked --release
cargo test --locked --release
# Regola numero uno e misure: immagini di prova, sorgente mai modificata.
ci/bench.sh target/release/lagotto
# L'helper rifiuta ciò che non è un disco.
out=$(printf '{"op":"list","path":"/etc/passwd","offset":null}\n{"op":"quit"}\n' | target/release/lagotto-helper serve)
echo "$out" | grep -q '"code":"L104"'
echo "Everything OK"
