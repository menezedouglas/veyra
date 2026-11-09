# Integração distribuída Veyra

`veyra-tests` é um peer de laboratório com dados sintéticos, não um cliente desktop. Cada execução cria um cofre em RAM e um transporte WebRTC nativo; o supervisor inicia dois processos independentes. O controle herdado por stdin/stdout troca documentos públicos de identidade, SDP e aprovações de teste. O conteúdo não passa pelo supervisor.

```sh
cargo build --locked -p veyra-tests
python3 tests/integration/run.py
```

A prova exige que as estatísticas do par ICE efetivamente selecionado indiquem rota direta. Ambos os processos executam Noise XX com prologue CBOR que vincula papéis, documentos assinados, execuções e digest do convite sintético. O supervisor compara os códigos derivados do transcript antes de aprovar explicitamente os dois lados. Essa aprovação automática existe apenas neste teste; o produto exige ação do usuário e comparação por canal independente.

Cada direção usa oferta HPKE descartável independente, envelope assinado e ACK autenticado. O teste abre uma vez, recusa a segunda abertura e retransmite o envelope original depois do consumo. O ACK repetido deve ser idêntico e nenhuma pendência pode reaparecer. A conexão é a mesma nas duas direções.

Para Coturn, forneça um `TransportConfig` em JSON por stdin:

```sh
python3 tests/integration/run.py --relay < /caminho/configuracao-sintetica-de-teste.json
```

Nesse teste, `relay_only` deve ser `true`, a lista `ice_servers` deve conter TURN autenticado e o par ICE selecionado deve indicar relay. O arquivo do exemplo só deve conter credenciais sintéticas de laboratório; clientes do produto não devem gravar credenciais do usuário em arquivos. Credenciais operacionais não fazem parte do código nem são passadas em argumentos. O laboratório Docker de `infrastructure/development` oferece um Coturn isolado.

A prova cobre dois processos Linux na rede de laboratório. Não demonstra NATs arbitrários, conectividade externa automática, TURN por TCP/TLS, Windows↔Linux, separação desktop–daemon–cofre nem segurança contra processo privilegiado. Os testes de IPC do cofre são independentes desta prova de transporte. Não existe substituição por mock ou transporte TCP para as mensagens.

## Fluxo do produto com processos separados

O teste `crates/daemon/tests/session_process.rs` usa as mesmas APIs do coordenador desktop: dois processos `veyra-daemon` e dois processos `veyra-vault`, cada um autenticado pelos pipes herdados. O processo de rede recebe apenas a interface de custódia fechada `VaultNetworkPort`; abrir, verificar confiança e selar texto são operações exclusivas do coordenador local. Os testes comuns também recusam esses comandos no esquema de custódia da rede.

```sh
sh tests/integration/run_product.sh
sh tests/integration/run_product.sh --relay
```

No Windows, o mesmo teste de produto pode ser executado com named pipes reais:

```powershell
python tests/integration/run_product_windows.py --report target/windows-product-report.json
```

O harness seleciona a toolchain MSVC fixada, compila os executáveis e identifica
o teste pelo artefato JSON do Cargo. `--no-build --test-binary <caminho>` permite
reutilizar um teste já compilado; `--daemon`, `--vault` e `--signaling` aceitam
executáveis explícitos. Os clientes são iniciados com `PATH` vazio para não
depender de Node ou Rust na execução. O relatório registra somente resultados
dos checks e hashes dos executáveis, sem mensagens, convites, fingerprints,
SDP ou credenciais.

`scripts/local-operator.py` fornece o serviço WSS real desse laboratório e do
modo dev. Ele escuta apenas em loopback, cria certificados temporários e entrega
um bundle público de CA aos processos filhos, preservando as raízes já
confiadas. Não instala CA no sistema. EOF no controle, `stop` ou interrupção
encerram o serviço e removem apenas os arquivos temporários desta execução.
Esse operador local não oferece TURN nem conectividade entre computadores.

O script cria uma CA e um certificado TLS de operador temporários, inicia o serviço WSS real e fornece a CA pública aos processos de laboratório. As chaves TLS do operador são arquivos de configuração temporários, distintos das chaves E2EE dos usuários. O script limpa esses arquivos e encerra o servidor ao sair. Nenhuma identidade de usuário ou mensagem é gravada.

O modo `--relay` exige o serviço `coturn-rest-laboratory` do Compose de desenvolvimento e deve rodar na rede Docker `veyra-infra_laboratory`, onde esse hostname de laboratório pode ser resolvido. O cliente começa com a lista ICE vazia e `relay_only=true`: o serviço WSS emite credenciais TURN REST de até cinco minutos, e o Coturn real valida essas credenciais. O segredo público sintético usado pelo script pertence somente ao operador de laboratório; não é uma credencial do cliente distribuído. O Coturn deve executar sem a opção `userdb`, para manter o driver de banco desabilitado.

As duas rotas foram executadas em 9 de novembro de 2026: direto passou em 1,31 s; relay com emissão automática de credenciais passou em 7,52 s. Cada execução verificou os códigos dos dois lados, exigiu aprovação explícita, enviou nos dois sentidos, recusou segunda abertura e descartou pendências após revogação. Os tempos são observações deste laboratório, não garantias de desempenho.

A primeira execução de relay automático encontrou uma configuração incorreta do Coturn (`userdb=/dev/null`), que ativava SQLite e impedia a validação do segredo estático. A configuração foi corrigida removendo `userdb`; o ensaio acima passou sem banco. Esses resultados não substituem os testes Windows↔Linux, redes/NATs externos, renovação de credenciais TURN, troca de operador ou retomada após mudança de rede.

## Núcleo com binários instalados

O harness também permite testar os executáveis instalados, mantendo o servidor
WSS e o coordenador do laboratório separados:

```sh
sh tests/integration/run_product.sh --installed
sh tests/integration/run_product.sh --installed --relay
```

Os caminhos padrão desse modo são `/usr/bin/veyra-daemon` e
`/usr/bin/veyra-vault`. Para outro prefixo de instalação, defina
`VEYRA_TEST_DAEMON_BIN` e `VEYRA_TEST_VAULT_BIN` com caminhos absolutos antes de
executar o script. Sem essas opções, o teste comum usa os binários de Cargo.

O modo `--installed` compila as ferramentas de laboratório e executa o teste
compilado diretamente com `PATH` vazio. Os dois daemons e os dois cofres herdam
esse ambiente, usando somente os caminhos absolutos configurados. O script não
reconstrói nem substitui os executáveis instalados e compara seus SHA-256 antes e
depois do ensaio. `VEYRA_TEST_REQUIRE_EMPTY_PATH=1` é a asserção interna desse modo.

Em 9 de novembro de 2026, os clientes instalados com a correção do binding e o
reap explícito no harness passaram em direto (0,78 s, rota selecionada `Direct`) e relay UDP com
credenciais automáticas (6,77 s, rota selecionada `Relay`). Ambos os ensaios
verificaram os hashes antes e depois, com os clientes herdando `PATH` vazio:

```text
veyra-daemon 09e6539146ed458a6d81a64dca204ac93582e2add531ce716542f9be17e83d68
veyra-vault  498e8513de48b56b08e40228b54e8e32541985054ed2ed214672a979acedeeee
```

Antes dessa correção, o daemon de hash
`89bd19c7fd8de603b3df569b979e6e34abf88499bb9cf8f800ba87f1d175a93c`
passou um ensaio direto em 0,83 s e um relay em 6,71 s, mas também apresentou
falhas de consulta `Unknown` nos dois modos. A última repetição de relay desse
pacote falhou em 9,65 s: mesmo depois da entrega, a rota continuou `Unknown`
durante os três segundos de espera. Essas falhas não foram contadas como sucesso.

A investigação identificou o erro no binding `datachannel` 0.16.1: a consulta do
par ICE usa buffers com o maior dos dois tamanhos, mas `ffi_string` recusava o
preenchimento após o primeiro NUL no candidato menor. O patch local lê a string
até o primeiro NUL e possui teste de regressão. A atualização periódica das
estatísticas, sozinha, não explicava a falha. O harness aguarda até três segundos
por uma rota efetivamente conhecida e só então exige `Direct` ou `Relay`; não
foi observada uma transição tardia de `Unknown` para rota conhecida nos ensaios
de diagnóstico que falharam.

Na primeira rodada pós-correção do binding (direto 0,80 s, relay 6,77 s), a
conferência detectou um novo processo de cofre já encerrado, em estado zombie e
adotado pelo PID 1 do builder. O harness enviava `End` sem aguardar explicitamente
o filho. O teste foi corrigido para recuperar o `Arc` único em até um segundo e
chamar o `VaultClient::end` existente, que aguarda o processo. Esse zombie
anterior foi preservado, sem intervenção no PID 1 ou no host.

Depois das duas repetições com esse ajuste, não houve novos processos vivos ou
zombies em relação à baseline de 39 zombies preexistentes. A porta WSS 18443
estava fechada, não restavam processos WSS/daemon/cofre vivos nem diretórios
`/tmp/veyra-product-tls.*`. O Coturn REST compartilhado foi reiniciado apenas
para a prova relay e parado ao final; o builder e as redes Docker foram preservados.

A prova cobre o núcleo instalado com `PATH` vazio para os clientes. O coordenador
de teste e o operador WSS continuam sendo ferramentas de laboratório. Ela não
valida a composição ou leitura pela interface gráfica, nem conclui o gate de
relay desktop ou os testes Windows.
