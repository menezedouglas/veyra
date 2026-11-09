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
anônimos herdados. Não há listener local ou externo. Um desafio aleatório de
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
Os named pipes com ACL e rejeição remota exigidos no Windows ainda estão pendentes.
O worker recusa o início nessa plataforma antes do bootstrap; não usa pipes
anônimos Windows como substituto silencioso dessa garantia.
Processos sob a mesma conta não oferecem isolamento integral contra malware.

## Tempo e falhas

Linux usa `CLOCK_BOOTTIME`, que inclui tempo suspenso. Falha do relógio encerra
o cofre; tempo regressivo invalida ofertas e pendências. O prazo receptor está
ancorado na oferta original. Retry, reconexão e reabertura não criam deadline.
Windows implementa VirtualLock com exclusão WER das páginas, WER NOHEAP e
GetTickCount64; esse crate passou por compilação cruzada. Falhas nativas recusam
a operação. Execução, suspensão, IPC e instalação Windows seguem sem validação.

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

Fontes primárias: [processos Tauri](https://v2.tauri.app/concept/process-model/),
[capabilities](https://v2.tauri.app/security/capabilities/),
[DataChannels RFC 8831](https://www.rfc-editor.org/rfc/rfc8831.html),
[Noise](https://noiseprotocol.org/noise.html),
[HPKE RFC 9180](https://www.rfc-editor.org/rfc/rfc9180.html).
