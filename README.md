<p align="center"><img src="assets/brand/logo-dark.svg" alt="Veyra" width="240"></p>

# Veyra

**Mensagens privadas, diretas e efêmeras.** Privado por padrão. Efêmero por design.

Mensageiro desktop experimental, gratuito e Apache-2.0, com núcleo Rust e
interface Tauri 2/Vue 3 em português. Não há contas, histórico ou banco de mensagens.
Os contatos sincronizados ficam salvos localmente e aparecem desconectados ao
reabrir o app. Identidades, autorizações, ofertas de chaves e filas pertencem à
execução e ficam em RAM. Um novo convite e a verificação da sessão são necessários
para voltar a conversar. O protocolo composto ainda não teve revisão independente.

**Ainda não é uma release pronta.** Há código e provas reais de troca direta e
por Coturn, um pacote Debian instalado e testado com duas UIs e o fluxo real
do núcleo Windows com dois daemons e dois cofres. QA gráfico completo/instalação Windows,
background, reconexão/troca de rede e auditoria completa de persistência ainda
precisam de evidência.
Veja a [matriz completa](IMPLEMENTATION_STATUS.md).
O IPC Windows usa named pipes privados com ACL do logon e rejeição remota;
o [ensaio do produto](tests/integration/windows-product-report.json) passou
12 checks com binários MSVC debug, sem Node ou Rust no PATH dos clientes.

O [pacote Linux amd64 experimental](artifacts/Veyra_0.1.0_amd64.deb) foi validado
em Debian 12, sem Node ou Rust no PATH do cliente. Confira
[SHA256SUMS](artifacts/SHA256SUMS) e instale com
`sudo apt install ./Veyra_0.1.0_amd64.deb`. Não há endpoint público embutido;
pareamento exige um operador WSS/TURN configurado.

O código atual também oferece [pareamento direto sem servidores](docs/direct-connection.md)
como padrão. Compile novamente para usar esse fluxo; os artefatos anteriores
continuam com o comportamento da versão em que foram gerados.

## Desenvolvimento

Requer Rust 1.99.1, Node 24.22.2 e pnpm 12.18.0, fixados no repositório.
Node/pnpm são ferramentas de build; não participam do transporte distribuído.
Linux precisa das [dependências oficiais Tauri](https://v2.tauri.app/start/prerequisites/).
O backend nativo adicional precisa de CMake, libnice e OpenSSL compatíveis.
Compatibilidade Linux universal não é declarada.

```sh
pnpm install --frozen-lockfile
pnpm desktop:dev
```

`desktop:dev` prepara os binários internos e usa conexão direta sem servidores
por padrão. Troque um convite e uma resposta entre as instâncias e compare os
códigos de verificação antes de enviar. Esse modo atende redes locais e
endereços diretamente acessíveis; NATs e firewalls podem impedir acesso entre
redes da internet. Não exige VPS, domínio ou serviço externo.

Para ensaios opcionais com `VEYRA_DEV_LOCAL_OPERATOR=1`, o supervisor inicia
um operador WSS somente em loopback. A CA
temporária é confiada apenas pelos processos desta execução, preservando as
raízes existentes; nada é instalado no repositório de certificados do sistema.
Fechar o app encerra esse operador e remove seu material TLS temporário.
Esse modo conecta instâncias na mesma máquina e não oferece TURN nem serviço
público para outros computadores. Para usar um operador externo, defina
`VEYRA_SIGNALING_ENDPOINT` ou compile com `VEYRA_DEFAULT_SIGNALING_ENDPOINT`.
Sem essa opção, nenhum operador é iniciado. O comando `pnpm dev` inicia somente
o frontend.

### Windows

Instale os [pré-requisitos do Tauri para Windows](https://v2.tauri.app/start/prerequisites/#windows):
Visual Studio Build Tools com **Desenvolvimento para desktop com C++** e Windows
SDK, WebView2 e Rust via rustup. O desktop usa a toolchain **MSVC**; a variante
GNU não é suficiente. Python 3 prepara os binários internos; OpenSSL é necessário
para o operador local de desenvolvimento e pode vir com Git for Windows.
Após instalar as ferramentas, abra um novo terminal.

No PowerShell, use os atalhos `.cmd` para não depender da política de execução
de scripts `.ps1`:

```powershell
pnpm.cmd install --frozen-lockfile
& "$env:USERPROFILE\.cargo\bin\rustup.exe" toolchain install 1.99.1-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy
npm.cmd run desktop:dev
```

`desktop:dev` prepara o cofre e o daemon em debug e abre a janela Tauri com Vite.
O pareamento direto funciona sem operador. `pnpm.cmd dev` inicia somente o frontend.
Para gerar o instalador Windows experimental, sem assinatura:

```powershell
npm.cmd run desktop:build:windows
```

Esse comando prepara os binários internos em release e os inclui no instalador
NSIS em `target/release/bundle/nsis/`. Os dois comandos também podem ser
executados diretamente com `node scripts/desktop.mjs dev` e
`node scripts/desktop.mjs build:windows`. Eles localizam o Rust em
`CARGO_HOME/bin` ou no diretório padrão `.cargo/bin` e selecionam a toolchain
MSVC fixada pelo projeto, sem alterar a toolchain global.

O núcleo Windows já executou pareamento, mensagens nos dois sentidos, abertura
única e revogação com processos reais. Gerar o instalador não substitui testar
sua instalação ou auditar o WebView; o pacote continua experimental e sem
assinatura. O operador local de desenvolvimento não é incluído como serviço
público na distribuição.

Para compilar o frontend e o binário Linux incorporado sem servidor Vite:

```sh
pnpm build
cargo build --locked -p veyra-desktop -p veyra-daemon --features tauri/custom-protocol
./target/debug/veyra
```

O cofre e o daemon devem ficar junto ao executável desktop. A conexão padrão
troca manualmente oferta e resposta entre os aplicativos, sem servidores.
Se houver um operador opcional configurado na distribuição, a conexão usa esse
serviço. Ao entrar com um convite WSS válido sem uma sessão conectada, o
aplicativo usa o operador indicado nele;
não é necessário copiar o endereço para as configurações. Credenciais TURN
temporárias também são recebidas automaticamente pela sinalização.

O modo direto não depende de endpoint público. O modo opcional WSS/TURN ainda
não inclui um endpoint público operacional. Publicar
esse serviço e configurar seu endereço uma vez é responsabilidade de quem
distribui o aplicativo. Sem serviço, a identidade local continua disponível nas
plataformas suportadas, e é possível entrar com um convite de um operador real.
O campo manual fica em **Configuração avançada**. Veja como configurar o serviço
da distribuição em [operações](docs/relay-operations.md#configuração-automática-do-cliente).
O caminho é `/v1/rendezvous`, com certificado válido e confiável pelo sistema.
Convites ficam no corpo do protocolo, nunca em URL de navegador.

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

No Windows, o laboratório equivalente usa a toolchain MSVC fixada e OpenSSL:

```powershell
python tests/integration/run_product_windows.py --report target/windows-product-report.json
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

CI usa ações fixadas e privilégios mínimos. O workflow de release gera rascunhos
experimentais com instaladores, checksums e relatório das pendências. O canal
`production` continua bloqueado enquanto houver critérios obrigatórios pendentes
em `docs/release-readiness.json`. Consulte o [procedimento de release](docs/releasing.md).
Não há instalador Windows validado, assinatura ou auditoria anunciada.
