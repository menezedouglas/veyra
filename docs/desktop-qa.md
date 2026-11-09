# QA desktop Linux — evidências e limites

Verificações executadas em 9 de novembro de 2026 no container isolado `veyra-dev-20261109`: Debian 12 amd64, Rust 1.99.1, GTK 3.24.38 e WebKitGTK 2.50.6. Os testes usam somente identidades e conteúdo sintéticos; nenhum dado de usuário foi fornecido.

## Primeira execução de desenvolvimento: ciclo de sessão

`cargo build --locked -p veyra-desktop -p veyra-daemon --features tauri/custom-protocol` passou. `custom-protocol` incorpora o frontend compilado, sem depender de Vite. A compilação também emitiu warnings na dependência GLib vendorizada; o sucesso não equivale a auditoria.

`tests/desktop/native_accessibility.py` controla o aplicativo real por AT-SPI/X11 sob Xvfb e D-Bus privado, mantendo `incognito=true`. Não intercepta `invoke`, não substitui comandos e não cria peers ou mensagens de demonstração.

A execução passou **16 checks**, registrados em `tests/desktop/linux-accessibility-report.json`:

- janela nativa e documento de acessibilidade disponíveis, com as cinco abas;
- sessão inicialmente inativa;
- clicar “Iniciar sessão” iniciou um processo filho real `veyra-vault` e exibiu uma impressão digital gerada no núcleo;
- pendências vazias sem mensagens de demonstração;
- diagnóstico sem transporte estabelecido e sem inventar rota direta ou relay;
- identificador de execução exibido nas configurações;
- “Sair e descartar sessão” encerrou o cofre, removeu a identidade da interface acessível e não deixou cofre vivo ou zumbi observado;
- nenhuma ocorrência integral da impressão digital ou runtime ID nos arquivos inspecionados ou nos 2.740 bytes de stdout/stderr capturados em RAM.

Essa primeira execução local não incluiu convite, conexão remota ou mensagem; as provas bidirecionais e do pacote instalado aparecem nas seções seguintes. A [captura nativa após encerramento](../tests/desktop/artifacts/linux-fixture.png) foi inspecionada visualmente; não contém identidade ativa nem corpo de mensagem. Nenhuma captura foi feita com conteúdo de mensagem.

## Duas UIs de desenvolvimento: fluxo direto bidirecional

`tests/desktop/two_desktops.py` passou **19 checks reais**, registrados em `tests/desktop/two-desktops-report.json`, com `failure: null`. A prova abriu duas janelas Tauri, cada uma com seu cofre e daemon separados, e um operador WSS local na porta 19443. O certificado do operador foi assinado por uma CA sintética de laboratório, confiada explicitamente pelos processos via `SSL_CERT_FILE`; não se desabilitou validação TLS.

O teste configurou o endpoint em cada UI, iniciou as duas sessões, criou um convite real e transferiu seu texto em RAM pelo AT-SPI/X11. Tokens e corpos foram enviados ao xdotool por stdin, sem colocá-los em argumentos ou arquivos. O pareamento completou Noise, exibiu códigos de transcript iguais e exigiu marcar a comparação e aprovar a identidade em ambas as janelas. As duas rotas exibidas vieram do transporte e indicaram P2P direto.

Nas duas direções, o teste digitou um texto sintético no compositor, enviou pelo núcleo, confirmou a limpeza imediata do compositor e observou uma pendência cujo corpo não aparecia antes da autorização. “Abrir uma vez” abriu a confirmação local; somente “Consumir e abrir” liberou o texto real decifrado pelo cofre HPKE. Transferir o foco para a outra janela removeu o corpo e o cartão consumido, sem histórico nem nova abertura disponível pela UI. Encerrar as duas sessões terminou os cofres.

Os oito arquivos de perfil observados — os quatro caminhos abaixo em cada instância — não continham as strings integrais dos textos sintéticos, convite ou fingerprints usados como marcadores. Tampouco houve correspondência nos 2.732 bytes de saída de processos capturados em RAM. Não foram feitas capturas de tela com corpo, convite ou identidade ativos nesse teste.

A prova com binários de desenvolvimento cobre uma rota direta em um único container Linux. Ela não exercita a instalação de um pacote; a instalação Debian e os testes posteriores dos binários instalados estão documentados abaixo. Permanecem fora desse harness NATs externos, rota TURN desktop, Windows e ausência universal de vestígios. O fluxo por relay no core/processos deve ser relatado separadamente.

Reprodução no ambiente de desenvolvimento, com frontend e três binários compilados:

```sh
python3 tests/desktop/two_desktops.py --binary "$(pwd)/target/debug/veyra" --signaling "$(pwd)/target/debug/veyra-signaling" --port 19443 --report /tmp/veyra-two-desktops-report.json
```

## Arquivos realmente observados

Apesar de `incognito=true`, o stack Tauri/WebKit criou quatro arquivos no XDG de dados isolado:

| Caminho relativo | Tamanho |
| --- | ---: |
| `data/app.veyra.desktop/WebKitCache/Version 17/salt` | 8 bytes |
| `data/app.veyra.desktop/hsts-storage.sqlite` | 12.288 bytes |
| `data/app.veyra.desktop/mediakeys/v1/salt` | 8 bytes |
| `data/app.veyra.desktop/storage/salt` | 8 bytes |

A inspeção da primeira sessão leu os quatro arquivos, cada um abaixo do limite de 16 MiB, sem correspondência integral das duas strings públicas da identidade usadas como marcadores. Os testes bidirecionais também buscaram as strings integrais dos corpos sintéticos e do convite nos dois perfis. A aplicação não implementa banco de mensagens; a presença do SQLite acima vem do WebKit e deve constar da análise de persistência. Modo efêmero não significa ausência de arquivos.

Buscar os marcadores integrais dos corpos nesses arquivos não detecta todas as representações, fragmentos ou cópias possíveis. Não foram inspecionadas chaves privadas, todos os diretórios do usuário, buffers de acessibilidade, GPU, swap, hibernação, dumps ou processos privilegiados. Os testes descartam seus diretórios de fixture após a inspeção. Apagar a fixture de laboratório não é apresentado como política de persistência do produto.

## Bloqueio do WebDriver preservado

Foram instalados `webkit2gtk-driver` 2.50.6 e `tauri-driver` estável 2.1.0 (`cargo install --version 2.1.0 --locked`). `tests/desktop/native_smoke.py`, um harness Python stdlib sem mocks, falhou na criação da sessão: `session not created: The remote session failed to create a new browsing context.` Nenhuma identidade foi iniciada por esse harness.

Tauri Runtime Wry 2.12.3 configura automação no contexto fornecido ao builder. Wry 0.57.1, ao receber `incognito=true`, cria outro contexto efêmero e ignora o contexto fornecido, sem herdar a automação. A constatação veio do código efetivamente compilado, em Tauri Runtime Wry próximo de 4713–4742 e Wry WebKitGTK próximo de 259–269. O modo efêmero não foi desativado para tornar o teste verde.

Fontes primárias: [Wry 0.57.1](https://docs.rs/crate/wry/0.57.1/source/src/webkitgtk/mod.rs), [Tauri Runtime Wry 2.12.3](https://docs.rs/crate/tauri-runtime-wry/2.12.3/source/src/lib.rs) e [Tauri — WebDriver manual](https://v2.tauri.app/develop/tests/webdriver/manual-setup/).

## Reprodução

Com frontend compilado e dependências de desenvolvimento instaladas no ambiente Linux de teste:

```sh
cargo build --locked -p veyra-desktop -p veyra-daemon --features tauri/custom-protocol
python3 tests/desktop/native_accessibility.py --binary "$(pwd)/target/debug/veyra" --report /tmp/veyra-desktop-qa-report.json
```

O harness requer `python3-pyatspi`, Xvfb, D-Bus e xdotool. Cria perfis XDG/TMP com permissões restritas, mantém respostas e saída de processos em RAM limitada e emite apenas nomes de checks, contagens e caminhos de arquivos. Não imprime fingerprints, tokens, corpos ou chaves.

Esses comandos usam binários de desenvolvimento e não instalam pacotes. A instalação Debian foi exercitada posteriormente, conforme a próxima seção. O instalador Windows, a execução Windows e a interoperabilidade desktop Windows↔Linux permanecem sem teste neste ambiente. Evidência de transporte/core em outros testes não substitui essa cobertura.

## Verificação do pacote Debian instalado

Após a instalação do pacote `veyra` **0.1.0** otimizado em release, os dois harnesses foram executados novamente contra **`/usr/bin/veyra`**, com os sidecars instalados vizinhos e a biblioteca nativa em `/usr/lib/veyra`. O PATH dos processos foi limitado a `/usr/bin:/bin`; `node` e `rustc` não estavam disponíveis nesse PATH.

A build instalada inclui a chave Noise privada mantida no daemon, o vínculo somente da chave pública no cofre, a recusa do bootstrap Windows sem named-pipe ACL e o patch de padding NUL do binding datachannel. A QA foi repetida após essas revisões, sem alterar o produto durante os testes.

Resultados: **16/16 checks de sessão/cofre** e **19/19 checks entre duas UIs, nas duas direções**, ambos com `failure: null`. Os relatórios são `tests/desktop/linux-installed-accessibility-report.json` e `tests/desktop/two-installed-desktops-report.json`. Seus campos `execution` registram a origem `installed-debian-release`, versão, revisão e hashes dos binários conferidos antes e após a execução. O operador WSS da fixture foi `/work/target/debug/veyra-signaling`; os clientes e seus workers eram os instalados pelo Debian.

| Artefato instalado/testado | SHA-256 |
| --- | --- |
| `/usr/bin/veyra` | `4de886d5374d476f2b14cccd9a49fcc326813e12474cc6be8ec16c184af3115d` |
| `/usr/bin/veyra-daemon` | `09e6539146ed458a6d81a64dca204ac93582e2add531ce716542f9be17e83d68` |
| `/usr/bin/veyra-vault` | `498e8513de48b56b08e40228b54e8e32541985054ed2ed214672a979acedeeee` |
| `artifacts/Veyra_0.1.0_amd64.deb` | `071859d476897a39eba379731eb75c699937f3b0ac112cdcab1252d5a67bb388` |

O fonte modificado de datachannel acompanha esse pacote em `/usr/lib/Veyra/_up_/_up_/_up_/infrastructure/licenses/datachannel-0.16.1-veyra-source.tar.gz`. O arquivo dentro do Debian e o instalado foram comparados byte a byte com o archive do workspace: SHA-256 `e8f89801ca38756fc5638fe4721237241efd4bf28d95b597295373d676f8f478`. Essa inclusão não depende de repositório remoto publicado e não constitui uma conclusão de auditoria jurídica.

O matcher de acessibilidade reconhece o contador real no nome “Mensagens pendentes 1”. Essa correção ficou somente no harness. Não foram capturadas telas de conteúdo. Os oito arquivos WebKit observados e as limitações de escopo descritas acima continuam aplicáveis. Esses resultados comprovam os fluxos exercitados nessa build Linux; não comprovam compatibilidade Windows, NATs externos, ausência universal de vestígios ou auditoria independente.
