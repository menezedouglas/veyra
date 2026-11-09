<p align="center"><img src="assets/brand/logo-dark.svg" alt="Veyra" width="240"></p>

# Veyra

**Mensagens privadas, diretas e efêmeras.** Privado por padrão. Efêmero por design.

Mensageiro desktop experimental, gratuito e Apache-2.0, com núcleo Rust e
interface Tauri 2/Vue 3 em português. Não há contas, histórico, banco de mensagens
ou contatos persistentes. Identidades, ofertas de chaves e filas pertencem à
execução e ficam em RAM. O protocolo composto ainda não teve revisão independente.

**Ainda não é uma release pronta.** Há código e provas reais de troca direta e
por Coturn e um pacote Debian instalado e testado com duas UIs. Windows,
background, reconexão/troca de rede e auditoria completa de persistência ainda
precisam de implementação ou evidência.
Veja a [matriz completa](IMPLEMENTATION_STATUS.md).
O início de sessão no Windows é recusado enquanto o IPC exigido com named pipes
e ACL explícita não estiver implementado.

O [pacote Linux amd64 experimental](artifacts/Veyra_0.1.0_amd64.deb) foi validado
em Debian 12, sem Node ou Rust no PATH do cliente. Confira
[SHA256SUMS](artifacts/SHA256SUMS) e instale com
`sudo apt install ./Veyra_0.1.0_amd64.deb`. Não há endpoint público embutido;
pareamento exige um operador WSS/TURN configurado.

## Desenvolvimento

Requer Rust 1.99.1, Node 24.22.2 e pnpm 12.18.0, fixados no repositório.
Node/pnpm são ferramentas de build; não participam do transporte distribuído.
Linux precisa das [dependências oficiais Tauri](https://v2.tauri.app/start/prerequisites/).
O backend nativo adicional precisa de CMake, libnice e OpenSSL compatíveis.
Compatibilidade Linux universal não é declarada.

```sh
pnpm install --frozen-lockfile
cargo build --locked -p veyra-daemon
pnpm desktop dev
```

Para compilar o frontend e o binário Linux incorporado sem servidor Vite:

```sh
pnpm build
cargo build --locked -p veyra-desktop -p veyra-daemon --features tauri/custom-protocol
./target/debug/veyra
```

O cofre e o daemon devem ficar junto ao executável desktop. Sem operador, a
sessão local mostra **infraestrutura não configurada**. Para parear duas instâncias,
configure o mesmo endpoint WSS real em Configurações, antes de iniciar cada
sessão. O caminho é `/v1/rendezvous`. O certificado precisa ser válido e confiável
pelo sistema. Convites ficam no corpo do protocolo, nunca em URL de navegador.

O backend Rust inicial aceita direto/TURN UDP e **recusa TURN TCP/TLS**. O backend
alternativo `native-libnice` mantém DataChannels libdatachannel/libnice e conecta
TURN TLS com um adaptador TLS padrão que valida certificado e hostname. Consulte
as instruções, limites de empacotamento e evidências em
[operações](docs/relay-operations.md) e [ADRs](docs/adr/).

Não há endpoints públicos distribuídos, serviços TURN gratuitos presumidos ou
chaves operacionais embutidas. A referência de laboratório usa somente fixtures
e certificados de operador sintéticos; não é infraestrutura de produção.

## Validação

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
pnpm test
pnpm build
cargo build --locked -p veyra-tests
python3 tests/integration/run.py
sh tests/integration/run_product.sh
```

Os testes de produto usam servidor WSS e processos cofre/daemon reais. A aprovação
automática de fingerprints existe somente no harness de laboratório. O usuário
precisa comparar os códigos por um canal independente na aplicação.
Veja [testing.md](docs/testing.md) para Coturn UDP/TCP/TLS, fuzzing e limites do QA.

## Privacidade e operação

Uma abertura permitida pelo cliente conforme não impede cópia pelo destinatário.
ACK confirma aceitação em RAM, não leitura ou armazenamento durável. Crash após
aceitação/claim pode perder a mensagem. Relays não guardam mensagens offline.
Veyra não promete anonimato, entrega universal, ausência absoluta de vestígios
do SO ou proteção contra malware no endpoint.

Leia [arquitetura](docs/architecture.md), [protocolo](docs/protocol.md),
[ameaças](docs/threat-model.md), [privacidade](docs/privacy.md),
[limitações](docs/security-limitations.md), [marca](docs/brand-guidelines.md),
[segurança](SECURITY.md) e [contribuição](CONTRIBUTING.md).
O nome e sua disponibilidade jurídica/registral não foram verificados para lançamento.

CI usa ações fixadas e privilégios mínimos. O workflow de pacote recusa lançamento
enquanto houver critérios obrigatórios pendentes em `docs/release-readiness.json`.
Não há instalador Windows validado, assinatura ou auditoria anunciada.
