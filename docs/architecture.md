# Arquitetura Veyra

Estado: implementação experimental. Consulte `IMPLEMENTATION_STATUS.md` antes de
interpretar uma decisão como funcionalidade entregue. A especificação completa
está em [requirements.md](requirements.md).

## Fronteiras

```text
Vue / WebView ── comandos locais Tauri ── coordenador Rust
                                             │
                          pipes herdados ────┼──── cofre volátil
                                             └──── daemon de rede separado

Mensagem → HPKE + assinatura → Noise XX → DataChannel SCTP/DTLS/ICE → direto / Coturn
```

A WebView e o coordenador Tauri não recebem chaves privadas. `list_pending`
contém apenas IDs, origem criptográfica e prazo restante. O cofre gera a
identidade Ed25519 e o `runtime_id` por execução e mantém o seed de assinatura e
as chaves de abertura em páginas dedicadas com lock nativo verificado.
As bibliotecas criptográficas criam cópias temporárias; essa proteção não cobre
automaticamente todas as cópias, pilhas, registradores ou a WebView.

`veyra-protocol` define CBOR de arrays com esquema fechado; `veyra-identity` faz
assinaturas Ed25519 e binding da chave Noise independente; `veyra-crypto` usa
Snow e HPKE. `veyra-domain`, `veyra-state`, `veyra-core` e `veyra-vault` mantêm
políticas, deduplicação, saída cifrada e custódia em RAM. `veyra-transport`
encapsula o backend WebRTC; `veyra-connectivity` valida configurações e rotas.
`veyra-ipc` restringe operações locais e tamanhos. Não existe camada de banco.

`veyra-session-runtime` roda no processo `veyra-daemon`, conecta WSS, faz ICE e
Noise e recebe comandos de envio já selados. Seus buffers de mensagens guardam
envelopes cifrados e metadados; o processo também mantém os segredos de transporte
necessários, incluindo sua chave privada Noise.
O coordenador desktop é o único dono dos handles do cofre. A ponte do daemon
aceita somente BindTransport (chave pública X25519), SignSignal com
domínio fechado, CreateOffer e AcceptEnvelope. O enum não possui OpenOnce,
VerifyPeer ou Seal. A aprovação de identidade e o plaintext de composição passam
do coordenador ao cofre; a abertura volta somente ao comando local solicitante.

O daemon gera seu par Noise com CSPRNG e mantém a chave privada em um
`LockedBuffer` local. Somente a chave pública atravessa a ponte em `BindTransport`;
o cofre a vincula à identidade Ed25519 e ao runtime por assinatura. A vinculação
é permitida uma única vez, antes de qualquer autorização de peer, sem mudar
Ed25519 ou runtime. Revogação não libera uma nova vinculação. O cofre zera a
metade privada Noise inicial que se tornou desnecessária. O helper
`prepare_transport` é local ao daemon; nenhuma resposta IPC contém a chave
privada Noise.

## Cofre separado

O executável `veyra-vault` é iniciado pelo coordenador com stdin/stdout em pipes
privados herdados: anônimos no Linux e named pipes pré-conectados no Windows.
Não há listener do cofre disponível para novos clientes. Um desafio aleatório de
32 bytes é enviado somente pelo pipe e respondido com SHA-256 com domínio
`Veyra/child-bootstrap/v1`. Não é uma autenticação de rede: a propriedade de
acesso vem dos handles privados herdados do processo pai. A resposta detecta
erros de bootstrap; não prova integridade de um executável substituído.

O protocolo local tem prefixo de tamanho de 32 bits big endian e JSON tipado,
no máximo 32 KiB e campos desconhecidos rejeitados. As operações do cofre têm
timeout de cinco segundos no coordenador. CBOR determinístico é usado no
protocolo entre peers.
Não há operação genérica de assinar/descriptografar. `OpenOnce` vincula ID e
confirmação local; o daemon não recebe os pipes nem `LocalAuthority`.

No Linux, antes do bootstrap, `fstat` verifica que stdin/stdout são FIFOs,
pertencem ao UID efetivo da instância e não permitem acesso por grupo/outros.
A identidade local vem do handle herdado privado, de seu proprietário no kernel
e do material efêmero; não há socket descobrível ou autenticação por pathname.
Testes reais rejeitam tipo e permissões incorretos. Isso não impede processos
privilegiados nem malware com controle equivalente do endpoint.
No Windows, o pai cria dois named pipes unidirecionais com nomes CSPRNG,
`FILE_FLAG_FIRST_PIPE_INSTANCE`, uma única instância e
`PIPE_REJECT_REMOTE_CLIENTS`. A DACL protegida concede direitos individuais de
dados somente ao SID do logon atual, sem `Everyone`, direitos genéricos de
escrita ou permissão de criar outra instância. O pai abre e pré-conecta os
clientes antes do spawn e verifica que o PID do cliente é o seu próprio PID.
Os handles originais do pai não são herdáveis; o spawn fornece ao worker as
duplicatas de stdin/stdout necessárias.

Antes de ler o desafio, o worker verifica tipo e extremidade do pipe, instância
única, proprietário/DACL, PID do servidor correspondente ao pai observado pelo
SO e sessão Windows correspondente. Arquivos redirecionados, pipes anônimos,
ACL ampla, outro servidor e outra sessão são recusados. Não há ciclo de
reconexão nem nomes de pipes ou desafios em argumentos, ambiente ou arquivos.
O desafio continua sendo a autenticação efêmera da instância; PID e ACL são
verificações adicionais do canal nativo. Testes de API Windows, de processos e
o [fluxo real do produto](../tests/integration/windows-product-report.json)
passaram. O daemon recebe apenas seu canal com o coordenador, que encaminha o
enum estreito de custódia; ele não recebe o handle do cofre nem `OpenOnce`.
Processos sob a mesma conta não oferecem isolamento integral contra malware.

## Tempo e falhas

Linux usa `CLOCK_BOOTTIME`, que inclui tempo suspenso. Falha do relógio encerra
o cofre; tempo regressivo invalida ofertas e pendências. O prazo receptor está
ancorado na oferta original. Retry, reconexão e reabertura não criam deadline.
Windows implementa VirtualLock com exclusão WER das páginas, WER NOHEAP e
GetTickCount64. Esses mecanismos e o IPC foram exercitados pelo fluxo real do
núcleo em MSVC debug; falhas nativas recusam a operação. Suspensão, hibernação,
viewer gráfico, instalação e auditoria abrangente de resíduos Windows ainda
precisam de validação.

Antes das alocações seguras, o processo Windows prepara e verifica um mínimo
de working set de 2 MiB. A quota de
[VirtualLock](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtuallock)
depende desse mínimo; a quota inicial limitava os testes paralelos do cofre.
O ajuste usa
[Get/SetProcessWorkingSetSizeEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsizeex),
preserva flags de limite, respeita um máximo rígido existente e relê o resultado.
Isso configura somente o próprio processo e não substitui verificar cada
VirtualLock e registro WER. Se a quota ou qualquer chamada falhar, o buffer não
é aceito como protegido.

O claim remove a pendência e toma a chave antes de descriptografar. Uma falha
posterior perde a mensagem. ACK significa aceitação em RAM; o remetente elimina
o envelope ao validar assinatura, digest e execução. Duplicatas não informam
leitura. Crash do cofre invalida a sessão; ele não é reiniciado como se as
chaves anteriores ainda existissem. O encerramento do daemon perde seu par
Noise; registrar outro par exige uma nova execução do núcleo.

## Desktop

Tauri 2 com capabilities dos comandos próprios, janela local `main`, validação
de origem no Rust, CSP restritiva, sem shell/filesystem/rede arbitrária, sem
conteúdo remoto, DevTools e título sem dados de peers. A UI usa Vue/TypeScript,
tokens locais e Pinia apenas para preferência visual em RAM. O modo incognito
foi configurado; a auditoria de escrita do WebView permanece necessária.

No modo `desktop:dev`, o supervisor inicia um operador WSS real em loopback
quando não existe endpoint de distribuição/execução e o modo local não foi
desativado. O helper cria TLS temporário, informa endpoint e bundle público de
CA ao supervisor e os fornece somente aos subprocessos do app. O bundle mantém
as raízes já confiadas; nenhuma CA é instalada no SO. EOF/`stop` encerram o
operador e removem sua configuração temporária. Esse serviço é apenas para a
mesma máquina; o aplicativo distribuído continua dependendo de um operador
externo real para conectividade entre computadores.

Fontes primárias: [processos Tauri](https://v2.tauri.app/concept/process-model/),
[capabilities](https://v2.tauri.app/security/capabilities/),
[DataChannels RFC 8831](https://www.rfc-editor.org/rfc/rfc8831.html),
[Noise](https://noiseprotocol.org/noise.html),
[HPKE RFC 9180](https://www.rfc-editor.org/rfc/rfc9180.html).
