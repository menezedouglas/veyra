# Testes e evidências

Ambiente observado: Linux amd64 em container Debian 12 isolado, Rust 1.99.1,
GTK 3.24.38, WebKitGTK 2.50.6, Node 24.22.2. Nenhum dado real de usuário foi usado.
Docker e ferramentas de desenvolvimento não são requisitos do usuário distribuído.

## Executado

* `cargo test --locked`: **59 passaram, zero falhas e um ignorado por padrão**.
  Inclui vetores RFC 8032 e RFC 9180, adulterações, limites/UTF-8, prazo, replay,
  fan-out, assinatura/ACK, duas aberturas concorrentes, pipe real e cancelamento
  de IPC sem reaproveitar resposta. O teste de produto ignorado foi executado
  separadamente nas duas rotas descritas abaixo; runs repetidos não são somados.
* Dois cofres em processos reais por pipes herdados: geração de execuções novas,
  confiança explícita, oferta/HPKE, recebimento, abertura recusada sem confirmação,
  uma abertura, duplicata idempotente e nenhuma pendência reaparecida.
* A chave privada Noise nasce e permanece no daemon. Testes confirmam que
  `prepare_transport` local não faz RPC, o schema rejeita as antigas respostas
  com chave privada e `BindTransport` só aceita chave pública uma vez antes de
  autorizar peers, preservando identidade Ed25519 e execução.
* `tests/integration/run.py`: dois processos reais, DataChannel confiável e
  bidirecional, Noise XX com bindings e código coincidente, HPKE assinado, ACK,
  consumo uma vez e pendências vazias. Direto e Coturn UDP passaram.
* Backend nativo: direto, TURN UDP, TURN TCP e TURN TLS passaram no laboratório.
  TLS usou validação de CA/hostname; testes negativos rejeitaram CA e hostname
  inválidos. A implementação é um adaptador TLS padrão sobre TURN/TCP libnice,
  não uma alegação de suporte TLS nativo inexistente na biblioteca.
* A prova instalada expôs um erro de C string em `datachannel` 0.16.1: candidatos
  selecionados com comprimentos diferentes eram descartados por padding NUL,
  deixando a rota `Unknown`. O patch local preservado em `vendor/datachannel`
  usa conversão limitada até o primeiro NUL. **Três regressões passaram**, e a
  de comprimentos diferentes falhou com o helper original. O script compila o
  helper e seus testes reais com `rustc --test`; não testa o crate inteiro.
  Depois do patch, direto e TURN/UDP/TCP/TLS passaram novamente com o helper
  SHA-256 `9dc6316c4967fe848d34a8f992b6a7e20719e65c7c37b827d84ced6f36d0cba6`.
  A biblioteca C++ permaneceu idêntica à da prova de firewall; o ensaio com
  UDP bloqueado não foi repetido após esse patch Rust.
* [UDP bloqueado no cliente](udp-blocked-lab.md): em namespace de rede
  descartável, o controle TURN/UDP falhou com DROP contado; TURN/TCP e TURN/TLS
  completaram Noise, HPKE, ACK e abertura única sob as mesmas regras. Somente
  DNS interno teve exceção. Não houve alteração do firewall do host; essa prova
  cobre processos de integração IPv4, sem GUI ou Windows.
* Frontend: **38 testes Vitest**, TypeScript estrito e build Vite passaram.
  Fixtures de comandos ficam apenas em testes; o aplicativo distribuído não
  inclui replies simulados. Prazos incluem tempo gasto no IPC, e descontinuidade
  do relógio oculta o viewer e invalida pendências visuais.
* Desktop Linux instalado: **16/16 checks de ciclo de sessão e 19/19 checks de
  duas UIs bidirecionais** passaram via AT-SPI/X11, com `incognito=true`, comandos
  reais, cofre/daemon separados e Node/Rust ausentes do PATH dos clientes.
  O pacote release Debian inclui os sidecars e a biblioteca nativa com RPATH;
  não exige toolchain do usuário. Veja [desktop-qa.md](desktop-qa.md) para hashes,
  relatórios e arquivos WebKit realmente observados.
* libFuzzer com AddressSanitizer, nightly-2026-11-01, cargo-fuzz 0.13.1:
  **4.938.060 execuções em 31 s**, max_len 16385, sem crash observado. Campanha
  curta a partir de corpus vazio, cobertura parcial; não prova ausência de bugs.
* RustSec atual consultado. Tokio/rand foram atualizados após avisos de unsoundness.
  A auditoria posterior registrou zero vulnerabilidades na categoria principal,
  um aviso de manutenção em proc-macro-error e o aviso de versão de GLib 0.18.5.
  O GLib local contém o backport oficial e **11 testes upstream do iterador
  passaram**. Nenhum advisory foi silenciosamente ignorado; veja `vendor/README.md`.
* pnpm audit foi executado sem advisories na árvore frontend observada.
* `cargo fmt --all -- --check` e Clippy com `-D warnings` passaram no workspace
  padrão e no desktop com `native-libnice`. O IPC e a memória protegida passaram
  por compilação cruzada Windows GNU; isso não valida execução ou named pipes.
* Guards de código/configuração verificam ausência de APIs de persistência/log
  no caminho controlado, CSP e capabilities. Não são auditoria do SO/WebView.

## Reprodução

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
pnpm test
pnpm build
cargo build --locked -p veyra-tests
python3 tests/integration/run.py
sh tests/integration/run_product.sh
sh tests/integration/run_product.sh --relay
sh tests/integration/run_product.sh --installed
sh tests/integration/run_product.sh --installed --relay
python3 scripts/connectivity-lab.py
python3 tests/persistence/check_source.py
python3 tests/security/check_capabilities.py
python3 tests/security/glib_backport.py
python3 tests/security/datachannel_padding.py --prove-original-failure
cargo audit
pnpm audit
cargo +nightly-2026-11-01 fuzz run protocol -- -max_total_time=30 -rss_limit_mb=512 -max_len=16385
```

Para o backend nativo (dependências CMake/libnice/OpenSSL/Clang instaladas):

```sh
CMAKE_TOOLCHAIN_FILE="$PWD/infrastructure/development/libdatachannel-nice.cmake" \
  cargo build --locked -p veyra-tests --features veyra-transport/native-libnice
python3 scripts/connectivity-lab.py --native
```

No ambiente de desenvolvimento, o loader precisa encontrar `libdatachannel.so`
no diretório `out/lib` produzido pelo build. No Debian instalado, o RPATH do
daemon resolve a biblioteca incluída em `/usr/lib/veyra`; isso foi testado com
os executáveis instalados em `/usr/bin`, sem variáveis do compilador.
O script aceita `--docker-container` para rodar peers no container de build numa
rede de laboratório isolada. Não altera firewall do host.

## Não comprovado

Automação WebDriver do Tauri foi tentada e falhou ao criar contexto incognito.
O modo efêmero não foi desligado para obter um teste verde. Veja
[desktop-qa.md](desktop-qa.md). A automação alternativa AT-SPI/X11 passou mantendo
incognito. Execução Windows, Windows↔Linux, instalador Windows,
troca de rede/NATs externos,
suspensão/hibernação, crash do cofre durante visualização e auditoria de todos os
perfis/caches/logs/containers/journal continuam pendentes.

O fluxo WSS completo de produto está separado em
`crates/daemon/tests/session_process.rs` e `tests/integration/run_product.sh`.
O teste é ignorado por padrão e foi executado explicitamente com servidor WSS
TLS, dois daemons e dois cofres reais, nas rotas direta e Coturn UDP. Na prova
relay, os clientes não receberam ICE pré-configurado: as credenciais temporárias
vieram do WSS autenticado e do emissor REST do Coturn. Certificados e segredos
operacionais eram sintéticos de laboratório; não há serviço público implantado.
Após a correção de custódia Noise, ambas as provas passaram novamente: direto
em **1,39 s** e relay automático em **7,43 s** (tempo de cada teste, não latência).
O modo `--installed` também passou nos dois caminhos usando os workers nativos
`/usr/bin/veyra-daemon` e `/usr/bin/veyra-vault`, com PATH vazio e hashes
conferidos antes/depois. O coordenador desse ensaio é o harness Rust; essa prova
do núcleo instalado não substitui o teste de relay pela GUI. Tempos, hashes e
detalhes da espera por telemetria estão em
[tests/integration/README.md](../tests/integration/README.md).
Não confundir testes unitários com prova distribuída, nem compilação com teste
de instalação.
