# Testes e evidências

Ambientes observados: Linux amd64 em container Debian 12 isolado, Rust 1.99.1,
GTK 3.24.38, WebKitGTK 2.50.6, Node 24.22.2; e Windows x64 com Rust 1.99.1 MSVC.
Nenhum dado real de usuário foi usado. Os números Linux abaixo preservam as
execuções históricas; a evidência Windows está separada por escopo e perfil.
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
  padrão e no desktop com `native-libnice`. A compilação cruzada Windows GNU
  histórica foi complementada pela execução MSVC descrita abaixo.
* Guards de código/configuração verificam APIs de persistência/log no frontend,
  nos crates e no coordenador Tauri, CSP e capabilities. A única exceção para
  escrita de dados do aplicativo é o módulo nativo `contacts.rs`, que guarda o
  catálogo público de contatos. Logging e storage do navegador continuam
  proibidos nesse módulo. Esses guards não são auditoria do SO/WebView.

## Windows: núcleo com named pipes

Em 9 de novembro de 2026, `run_product_windows.py` executou o teste de produto
com WSS TLS real, dois processos `veyra-daemon` e dois `veyra-vault` em Windows
x64. Os binários eram **MSVC debug**, identificados pelos hashes no
[relatório preservado](../tests/integration/windows-product-report.json).
Os **12/12 checks passaram**, com `failure: null`: pareamento Noise, código de
transcript coincidente, aprovação explícita do coordenador de teste, mensagens
HPKE nos dois sentidos, recusa de abertura sem confirmação e de segunda
abertura, rota ICE direta e descarte após revogação. Os clientes usaram caminhos
absolutos e `PATH` vazio; não executaram Node ou Rust.

Também passaram oito testes do crate IPC e seis testes de processos Windows:
transferência por named pipes, ACL estreita do logon, recusa de ACL permissiva
e de identidades de servidor incorretas, cliente/instância adicional recusados,
stdio anônimo recusado, bootstrap truncado/EOF, ciclo real do cofre e ponte
daemon–coordenador–cofre para registrar somente a chave pública de transporte.
O desafio permanece nos handles herdados. O daemon não recebe o handle do
cofre nem autoridade de `OpenOnce`.

O teste completo das bibliotecas expôs cinco falhas de memória no cofre quando
executado em paralelo: a quota inicial do working set não comportava todas as
páginas dedicadas. A implementação passou a preparar e verificar 2 MiB no
próprio processo com `Get/SetProcessWorkingSetSizeEx`, preservando flags e um
máximo rígido explícito. Após a correção, o teste com **128 páginas simultâneas**
passou com VirtualLock e exclusão WER reais; **62 testes das bibliotecas do
workspace** e Clippy também passaram. O token restrito do ambiente de automação
negou o ajuste com Win32 1314; a validação ocorreu no contexto Windows normal,
sem retirar a checagem nem aceitar memória desbloqueada. A relação da quota
com o mínimo de working set está documentada em
[VirtualLock](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtuallock)
e [SetProcessWorkingSetSizeEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsizeex).

A observação gráfica da janela nativa debug confirmou **Identidade ativa** e o
banner do operador local de desenvolvimento. Essa observação cobre abertura e
início da sessão; criação de convites, duas UIs e mensagens pela GUI Windows
continuam sem validação completa.

A CA operacional era sintética, confiada somente pelos processos da fixture;
não houve instalação de certificado no SO nem desativação da validação TLS.
O relatório não contém conteúdo, convites, fingerprints, SDP ou segredos.
Esse ensaio comprova o núcleo no mesmo host e na rota direta. Não comprova GUI
Windows, instalador instalado, TURN Windows, interoperabilidade Windows↔Linux,
redes externas, suspensão ou auditoria de resíduos do WebView/SO.

## Windows: conexão direta sem serviços externos

Em 9 de novembro de 2026, o teste `direct_session_process` passou em Windows
x64 com a toolchain Rust 1.99.1 MSVC debug: **1 passou, 0 falhou, 0 ignorado**,
em **37,43 s**. O ensaio usou dois daemons e dois cofres reais, candidatos
somente de loopback e listas vazias de servidores ICE e sinalização. Nenhum
operador WSS, STUN, TURN ou certificado de laboratório foi necessário.

A mesma prova passou com os workers **release** e o coordenador de teste debug
em **36,86 s**, com `PATH` vazio nos clientes. Os sidecars preparados para o
instalador têm os mesmos hashes dos workers testados; os hashes e limites de
escopo estão no [relatório direto Windows](../tests/integration/windows-direct-product-report.json).
A janela debug também confirmou identidade ativa e diagnóstico **Direto · sem
servidor**. Isso cobre inicialização/diagnóstico, sem substituir duas UIs ou
instalação em outra máquina.

A resposta manual foi devolvida após **35 segundos**, ultrapassando o timeout
ICE anterior de 30 segundos sem perder a negociação. Também passaram recusa de
convite próprio/adulterado e de resposta repetida, bloqueio de envio antes da
aprovação, comparação de códigos e aprovações explícitas, mensagens HPKE nos
dois sentidos, confirmação para abertura e recusa da segunda abertura,
descarte após revogação e novo pareamento da mesma identidade com código novo
e nova aprovação. A rota observada foi `Direct`.

```powershell
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-msvc'
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --locked -p veyra-daemon --test direct_session_process -- --nocapture
```

Esta evidência cobre o núcleo no mesmo computador. Não valida duas interfaces
gráficas, computadores distintos, LAN, passagem por NAT, interoperabilidade
entre sistemas, instalação ou ausência de resíduos do SO/WebView. O fluxo de
uso e seus limites estão em [conexão direta](direct-connection.md).

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

No Windows, com a toolchain MSVC fixada, Python 3 e OpenSSL disponíveis:

```powershell
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-msvc'
cargo test --locked -p veyra-ipc
cargo test --locked -p veyra-daemon --test vault_process --test windows_process_ipc
python tests/integration/run_product_windows.py --report target/windows-product-report.json
```

Esse último comando também é apropriado para a etapa de produto Windows no CI.
Ele compila com lockfile, descobre o executável de teste pelo JSON do Cargo e
inicia o operador local real. `--no-build --test-binary <caminho>` reutiliza
artefatos explícitos; `--daemon`, `--vault` e `--signaling` permitem selecionar
os executáveis. Não se deve descrever uma nova build de release ou instalada
como validada apenas por reutilizar o relatório debug preservado.

O mesmo ensaio também passou com os quatro executáveis em **release**, após a
correção da quota de memória Windows: [relatório de release](../tests/integration/windows-product-release-report.json).
Os 12 checks passaram com `failure: null`, caminho direto e `PATH` vazio nos
clientes. Os hashes identificam exatamente os executáveis usados. Esse ensaio
do núcleo cobre processos reais no mesmo host; não é teste de instalação nem
de duas interfaces gráficas completas.

## Não comprovado

Automação WebDriver do Tauri foi tentada e falhou ao criar contexto incognito.
O modo efêmero não foi desligado para obter um teste verde. Veja
[desktop-qa.md](desktop-qa.md). A automação alternativa AT-SPI/X11 passou mantendo
incognito. QA gráfico completo Windows, Windows↔Linux, instalação Windows,
troca de rede/NATs externos,
suspensão/hibernação, crash do cofre durante visualização e auditoria de todos os
perfis/caches/logs/containers/journal continuam pendentes.

O fluxo WSS completo de produto está separado em
`crates/daemon/tests/session_process.rs`, `tests/integration/run_product.sh`
e `tests/integration/run_product_windows.py`.
O teste é ignorado por padrão e foi executado explicitamente com servidor WSS
TLS, dois daemons e dois cofres reais, nas rotas direta e Coturn UDP no Linux.
No Windows, a execução preservada cobre a rota direta. Na prova
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
