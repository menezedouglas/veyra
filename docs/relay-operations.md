# Operação da sinalização e dos retransmissores Veyra

O cliente usa [conexão direta sem servidores](direct-connection.md) por padrão.
Este documento descreve o modo WSS/TURN opcional, que exige a infraestrutura
de um operador. A ausência de um operador não impede criar convites diretos.

Nenhum endpoint público foi provisionado. Distribuir um binário não cria
sinalização WSS, certificados, capacidade TURN ou autorização para usar um relay
de terceiros. O serviço deve ser provisionado e configurado por quem distribui
o aplicativo; a experiência normal do cliente não exige instalar infraestrutura
nem digitar endereços de servidores.

## Configuração automática do cliente

Depois de publicar a sinalização WSS e o Coturn descritos abaixo, configure
o endpoint público uma vez no ambiente de compilação da distribuição:

```powershell
# Substitua pelo domínio real provisionado e com certificado válido.
$env:VEYRA_DEFAULT_SIGNALING_ENDPOINT = 'wss://seu-dominio/v1/rendezvous'
npm.cmd run desktop:build:windows
```

O endereço público fica embutido no executável. Não inclua senhas, tokens,
segredo TURN ou chaves TLS nessa variável. Para desenvolvimento ou uma
instalação administrada, `VEYRA_SIGNALING_ENDPOINT` configura o endereço na
execução do aplicativo. A prioridade é: seleção explícita nas opções avançadas,
variável de execução e endereço da distribuição. Uma configuração presente
inválida é recusada; não se tenta outro operador silenciosamente.

O botão **Iniciar sessão** usa essa configuração automaticamente. Para receber
um convite sem uma sessão conectada, o cliente valida formato, prazo, endereço,
binding assinado e fingerprint antes de iniciar os componentes locais e usar
o endpoint do convite. Uma sessão já conectada mantém seu operador; um convite
de outro operador é recusado nessa sessão. O endereço no convite não comprova
a identidade humana nem substitui a verificação do código pelos participantes.

TURN não requer configuração manual no desktop: após o attach, o WSS fornece
URLs e credenciais temporárias. O segredo usado para emiti-las permanece
somente no operador. Ter um endpoint configurado não comprova disponibilidade;
a conexão real é estabelecida durante o pareamento.

No aplicativo distribuído, sem um endpoint de distribuição ou de execução,
a interface usa o modo direto: a oferta e a resposta acompanham os códigos
de pareamento, sem conexão WSS. Um convite WSS recebido antes de iniciar uma
sessão ainda pode selecionar o operador opcional. Linux e Windows possuem o IPC de produto implementado;
o [ensaio Windows do núcleo](../tests/integration/windows-product-report.json)
passou em MSVC debug, sem validar instalação ou GUI.

## Operador local automático no desenvolvimento

`npm.cmd run desktop:dev` no Windows, ou `pnpm desktop:dev` no Linux, prepara
o cofre, o daemon e a sinalização. Quando `VEYRA_SIGNALING_ENDPOINT` e
`VEYRA_DEFAULT_SIGNALING_ENDPOINT` não estão definidos, o supervisor inicia
`scripts/local-operator.py` com o binário de sinalização real somente quando
`VEYRA_DEV_LOCAL_OPERATOR=1`. Por padrão, o desenvolvimento também usa o modo
direto sem operador.
Uma variável explicitamente definida, mesmo vazia ou inválida, não é substituída
silenciosamente por esse operador local.

O listener usa somente `127.0.0.1` e uma porta disponível. O helper cria uma CA
e um certificado de um dia em diretório temporário exclusivo, verifica o
handshake TLS e devolve endpoint e caminho do bundle público. O supervisor
fornece `VEYRA_SIGNALING_ENDPOINT` e `SSL_CERT_FILE` apenas aos subprocessos do
app; o bundle inclui as raízes já confiadas além da CA temporária. Não há
alteração do repositório de certificados do SO. Fechar o app envia `stop`/EOF,
encerra o serviço e remove o material TLS dessa execução. O console informa
explicitamente o escopo local.

Esse modo serve para pareamento entre instâncias na mesma máquina. Ele não
inclui Coturn, não publica domínio e não atende clientes de outros computadores.
Para usar um serviço externo, configure o endpoint real conforme a seção
anterior. Cada helper possui endpoint e CA próprios: dois launchers WSS
independentes não compartilham operador nem confiança TLS automaticamente.
O instalador não inicia esse helper de desenvolvimento nem distribui seus
certificados. A operação pública ainda exige provisionar WSS/TURN, domínio,
TLS válido e credenciais operacionais próprias.

## Sinalização WSS

O binário `veyra-signaling` recebe endereço de escuta, caminhos do certificado e
da chave TLS e, opcionalmente, caminho de uma configuração pública de TURN:

```sh
veyra-signaling 127.0.0.1:8443 /run/tls/fullchain.pem /run/tls/privkey.pem /etc/veyra/turn-issuer.json
```

O único caminho é `/v1/rendezvous`. O convite e a identidade são enviados no corpo
do WebSocket, nunca na URL. A implementação recusa query string e Origin de
navegador; o cliente é Rust nativo. Não há cadastro, busca de usuários, mailbox,
redis, SQLite ou armazenamento offline.

As mensagens de sinalização têm esquema JSON fechado. As assinaturas são sobre
um array CBOR determinístico de dez campos com domínio `Veyra/signaling/v1`,
versão, sala, documento de identidade, destinatário, sequência, expiração, papel,
operação e payload. O documento também vincula assinatura Ed25519, Noise público
e execução. O servidor verifica consistência criptográfica, não identidade humana.
Os peers ainda precisam verificar a fingerprint/transcript e aprovar localmente.

Limites implementados:

| Recurso | Limite |
| --- | --- |
| WebSockets simultâneos | 64 |
| Salas + tombstones de replay | 128 |
| Participantes por sala | 2 |
| Vida da sala | até 300 s; deadline original não renovado |
| Fila por peer | 8 pacotes |
| Mensagem/frame WebSocket | 64 KiB |
| Payload de sinalização | 48 KiB |
| Buffers WebSocket leitura/escrita | 8 KiB / 16 KiB; escrita máxima 128 KiB |
| Primeiro pacote autenticado | 10 s |
| Orçamento de mensagens por socket | 128 |
| Timeout de escrita encaminhada | 5 s |

Salas consumidas ou desconectadas deixam apenas tombstone até o prazo original,
para impedir recriar o mesmo convite. Ao esgotar capacidade, a operação é recusada.
Esses limites reduzem crescimento de RAM; não constituem defesa completa contra
DDoS de rede. O operador ainda precisa provisionar banda e proteger a borda.

## Credenciais TURN temporárias

O quarto argumento opcional aponta para JSON com somente `urls` (até quatro
endpoints `stun:`/`turn:`/`turns:`, incluindo ao menos um TURN) e `auth_secret_file`. Esse último caminho aponta para
um arquivo protegido contendo apenas o mesmo segredo de alta entropia usado em
`static-auth-secret` no Coturn. No Unix, permissões de grupo/outros são recusadas.
Use um segredo de pelo menos 32 bytes e mantenha-o exclusivo do operador.

Após um attach assinado válido reservar uma sala limitada, a resposta WSS
`attached` contém `turn: null` ou `turn: {urls, username, credential, expires_unix}`.
Pode-se anunciar STUN UDP no mesmo Coturn para priorizar caminhos diretos, além
de TURN UDP, TURN TCP e TURN/TLS. Há uma emissão por socket aceito; o cliente não configura o segredo do operador.
O username é `expiração:identificador_aleatório`, a senha usa HMAC-SHA1 existente
e Base64 conforme TURN REST do Coturn, e a validade é de cinco minutos.
Esse mecanismo autentica a utilização transitória do relay; não é E2EE de
mensagens e não fornece recuperação de sessão.

O segredo do emissor fica em buffer zerorizável e não vai para o cliente. Os
clientes recebem apenas credenciais transitórias. O serviço não possui as chaves
privadas de identidade, Noise ou HPKE dos participantes. Operadores diferentes
devem usar segredos diferentes. A política de admissão pública ainda exige
validação sob abuso real: self-signature e posse do convite não são cadastro.

## Coturn

O serviço WSS possui uma imagem multi-stage em `services/signaling/Dockerfile`.
Rust 1.99.1 e Debian bookworm-slim estão fixados por digests observados do
registro oficial. O runtime recebe o binário, a licença do projeto e os avisos originais Rust/native,
executa como UID/GID 10001 e não inclui Node, toolchain Rust ou banco. A
verificação completa de avisos/licenças das dependências continua sendo parte
da liberação, não uma conclusão jurídica dessa imagem de laboratório.

```sh
docker build -f services/signaling/Dockerfile -t veyra-signaling:0.1.1 .
docker compose -f infrastructure/development/compose.signaling.yaml --profile operator up -d --no-build
```

O segundo comando exige caminhos absolutos reais em
`VEYRA_SIGNALING_TLS_DIRECTORY` e `VEYRA_SIGNALING_OPERATOR_DIRECTORY`.
O primeiro diretório fornece `fullchain.pem` e `privkey.pem`; o segundo fornece
`turn-issuer.json` e o arquivo secreto referenciado por ele. No JSON, usar
`auth_secret_file: "/run/veyra-operator/turn-auth-secret"` e URLs dos operadores
realmente provisionados. O conteúdo desse arquivo deve coincidir com o segredo
REST protegido do Coturn; o emissor recusa menos de 32 bytes e permissões Unix
para grupo/outros. Certificados públicos e configuração pública não são chaves
privadas E2EE de usuários.

Os diretórios são montados read-only. O UID do serviço deve poder ler apenas os
arquivos necessários; pode ser ajustado por `VEYRA_SIGNALING_UID`/`GID` para
corresponder à custódia local do operador. Não tornar a chave privada mundialmente
legível para contornar uma falha de permissões. O padrão publica a porta somente
em 127.0.0.1:8443. Publicação externa exige configurar explicitamente
`VEYRA_SIGNALING_BIND_ADDRESS` e `VEYRA_SIGNALING_PUBLIC_PORT`, DNS e certificado
válido do operador; nenhum domínio operacional é fornecido pelo repositório.

O Compose WSS é separado do laboratório e usa perfil `operator`, para não exigir
certificados ao executar os ensaios locais de Coturn. O runtime tem rootfs
read-only, `/tmp` em tmpfs, core dumps desabilitados, cap drop total, 256 MiB de
RAM, 64 processos/threads, uma CPU e Docker logging desativado. O arquivo
`.dockerignore` exclui credenciais, fixtures TLS, caches e metadados do contexto
de build. Não colocar material de operador em diretórios de código.

Veja `infrastructure/coturn/README.md`, as duas configurações e o Compose de
produção. As portas de referência são 3478 UDP/TCP, 443 TCP mapeada ao listener
TURN/TLS 5349 e 49160–49259 UDP para alocações. Não abrir portas no modem do
usuário desktop; essas portas pertencem à infraestrutura do operador.

Na referência: duas threads de relay, 4 alocações por credencial, 256 totais, 128 KiB/s por sessão,
16 MiB/s agregados, até 20 challenges não autenticados por IP/segundo e allocation lifetime máximo de 600 s. Destinos internos,
link-local, multicast e interfaces de metadados são bloqueados. A configuração
do laboratório tem quotas próprias e aceita IPs privados, exclusivamente em
rede isolada sem portas publicadas.

`no-tcp-relay` proíbe alocações TCP RFC 6062, não a conexão cliente–TURN por
TCP/TLS usada para obter alocações UDP WebRTC. TURN/TLS não é HTTP, e não garante
travessia de proxies HTTP, inspeção corporativa ou todas as redes móveis.

Não definir `userdb`, nem mesmo `/dev/null`: essa opção ativa o driver SQLite.
No Coturn 4.18.0, erro ao buscar segredos no banco pode sobrescrever a seleção do
segredo estático e recusar TURN REST. Sem a opção, o driver fica UNKNOWN/NULL;
`no-dynamic-ip-list` e `no-dynamic-realms` impedem consultas de política dinâmica.
A prova REST após essa correção passou com credenciais HMAC e dois peers reais;
a inspeção do laboratório encontrou `/var/lib/coturn` vazio e somente um pidfile
em `/tmp`, sem arquivo de log ou banco. Isso cobre esses diretórios nesse ensaio,
não todos os componentes do host. CLI e DTLS do cliente TURN estão desligados
por padrão nessa versão; flags antigas `no-cli`/`no-dtls` foram removidas para
não gerar warnings precoces e abrir um log padrão antes da configuração final.

## Backend nativo e TLS

O experimento `webrtc 0.21.1` suporta TURN/UDP; `turns:` e TURN/TCP são recusados.
O backend `native-libnice` usa libdatachannel/libnice existente para DataChannel,
DTLS, ICE e TURN/TCP. A enumeração TURN_TLS da libnice consultada não estabelece
TLS RFC de fato; Veyra não a trata como suporte nativo.

O adaptador `tls_turn.rs` faz handshake TLS com rustls validando cadeia,
hostname/SNI e validade antes de expor uma porta TCP efêmera somente em
127.0.0.1. Libnice conecta ao adapter por TURN/TCP; o adapter encaminha bytes
pela sessão TLS autenticada ao operador. Ele não implementa TURN, ICE, SCTP,
DTLS ou framing proprietário. Cada endpoint tem até oito conexões aceitas,
quatro ativas, buffers de 16 KiB por sentido e tarefas canceladas ao fechar o
peer. Não existe caminho TLS sem validação nem fallback para TCP no mesmo
endpoint em caso de certificado/hostname/handshake inválido.

Indisponibilidade TCP/timeout de um endpoint TLS pode deixar os demais endpoints
explicitamente configurados ou o P2P direto disponíveis; uma falha de
autenticidade continua fatal. O ensaio passou com Coturn real em UDP, TCP e TLS,
e os testes TLS rejeitaram CA não confiável e hostname incorreto. A prova adicional
em [udp-blocked-lab.md](udp-blocked-lab.md) manteve DROP de UDP em um cliente
descartável: TURN/UDP falhou, enquanto TURN/TCP e TURN/TLS passaram. Isso cobre
esse namespace Linux isolado; NATs reais e inspeção corporativa seguem pendentes.

## Compilação e bibliotecas nativas

Em laboratório Debian/Ubuntu, instalar ferramentas de build: `build-essential`,
`cmake`, `pkg-config`, `libssl-dev`, `libnice-dev`, `libclang-dev`. Não são
dependências que o usuário final deve instalar manualmente.

```sh
export CMAKE_TOOLCHAIN_FILE="$PWD/infrastructure/development/libdatachannel-nice.cmake"
cargo build --locked -p veyra-daemon --features native-libnice
cargo build --locked -p veyra-desktop --features native-libnice
```

O guard de build recusa `native-libnice` sem um toolchain CMake que exija
`USE_NICE=ON`. Se datachannel-sys já tiver sido compilado sem essa configuração,
limpar esse pacote antes de reconstruir. A libdatachannel é dinâmica nesse
modo: o build a instala em `target/<profile>/build/datachannel-sys-*/out/lib/`.
No laboratório, `LD_LIBRARY_PATH` aponta exclusivamente para esse diretório;
`scripts/connectivity-lab.py --native --docker-container ...` resolve esse caminho.

O pacote Debian experimental inclui `libdatachannel.so.0.23` em `/usr/lib/veyra`,
com RPATH no daemon, e declara libnice, OpenSSL, certificados de CA e o stack
GTK/WebKit, com suas dependências transitivas. Os três executáveis são instalados
em `/usr/bin`. O staging usa `scripts/prepare-sidecars.py --native-libnice` e a
configuração `apps/desktop/src-tauri/tauri.native-linux.conf.json`.
A instalação release e duas UIs nativas foram testadas em Debian 12, sem Node ou
Rust no PATH dos clientes; veja [desktop-qa.md](desktop-qa.md). Isso não estabelece
compatibilidade Linux universal. O núcleo Windows foi executado com o backend
Rust e named pipes; backend nativo Windows, GUI, instalação e privacidade ainda
exigem evidência própria. Os avisos originais Rust/nativos e as licenças acompanham o pacote;
sua preservação não constitui auditoria jurídica de compatibilidade.

## Logs por camada

| Camada | Referência entregue | Validação operacional restante |
| --- | --- | --- |
| Serviço Axum | Sem access logger; erros genéricos; corpos/URLs de convite não são logados | Verificar wrapper/systemd e coleta externa |
| Rust WebRTC/datachannel | Níveis do facade `log` compilados como off no transporte | Verificar integração futura não reativa logs |
| GLib/libnice | Aviso observado de propriedade `consent-freshness`, sem conteúdo/IPs | Resolver aviso upstream antes de liberar backend nativo |
| Coturn | `no-stdout-log` e `log-file=/dev/null`; sem syslog na referência | Verificar alterações da configuração efetiva |
| Containers | `logging: none`, rootfs read-only, tmpfs limitado | Verificar daemon Docker/journal/rotacionamento do host |
| Proxy reverso | Não fornecido nem necessário para TLS nativo do Axum | Se adotado, remover access/error logging sensível |
| Captura de rede | Não habilitada em produção | Não introduzir captura permanente |
| TLS/configuração do operador | Podem existir protegidos em disco | Excluir de acesso público e backups indevidos |

Um operador pode observar IPs, documentos públicos e SDP/candidatos, copiar
ciphertext e metadados. Assinatura da sinalização não é sua criptografia E2EE.
Não há prova de apagamento de um operador malicioso, nem alegação de anonimato.
