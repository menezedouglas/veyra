# Prompt de implementação — Veyra

## 1. Papel e resultado esperado

Atue como arquiteto de software e engenheiro sênior de Rust, aplicações desktop, redes P2P e segurança. Implemente um aplicativo funcional, não apenas uma proposta, protótipo visual ou coleção de interfaces sem integração.

O nome definitivo do produto é **Veyra**: mensageiro gratuito e open source para **Windows e Linux**, com interface gráfica, comunicação P2P prioritária, fallback automático por retransmissores e mensagens efêmeras sem persistência intencional.

Trabalhe no repositório disponível. Antes de alterar arquivos, examine sua estrutura e preserve trabalho existente. Se estiver vazio, crie o monorepositório. Consulte documentação oficial atual, escolha versões compatíveis, fixe dependências e registre decisões em ADRs. Não invente APIs, versões, resultados de testes, endpoints operacionais ou auditorias.

Este documento substitui decisões anteriores conflitantes: não usar SSH; a interface não é exclusivamente terminal; sinalização, STUN e relays são permitidos; o usuário não configura manualmente modem, firewall ou portas. A aplicação pode realizar ajustes pontuais, consentidos e reversíveis.

## 2. Requisitos obrigatórios e limites

O mesmo aplicativo deve iniciar e aceitar conexões. Esses papéis são de transporte, não versões diferentes do produto. Uma conexão estabelecida é bidirecional, independentemente de quem enviou a primeira mensagem.

Priorize o caminho direto e utilize relay automaticamente quando necessário. A E2EE termina exclusivamente nos dispositivos dos participantes. Nenhum operador recebe chaves privadas de identidade dos usuários, segredos das sessões ponta a ponta ou chaves de abertura das mensagens.

Não criar contas, cadastro por telefone/e-mail, lista pública de usuários, banco de mensagens, histórico, backup ou recuperação de sessão. Persistir os contatos sincronizados em catálogo local nativo para que fechar o aplicativo não perca a lista. Identidades e autorizações continuam temporárias por execução do núcleo, não por janela aberta.

Mensagens e credenciais do usuário ficam somente em RAM controlada pela aplicação. A primeira versão suporta texto e envio individual ou para múltiplos destinatários selecionados. Não implementar anexos, chamadas, grupos persistentes, blockchain, DHT, VPN, SSH, IA ou recursos sociais.

Prioridade decisória: **autenticidade e confidencialidade → ausência de persistência de mensagens e segredos → disponibilidade → conveniência**. O catálogo público de contatos é a exceção de persistência autorizada. Nunca resolver uma falha gravando mensagens, aceitando identidade não verificada ou transmitindo texto claro.

Não anunciar anonimato, conexão universal, irrecuperabilidade absoluta, resistência a malware no endpoint ou impossibilidade de o destinatário copiar a mensagem. “Leitura única” significa no máximo uma abertura permitida pelo cliente conforme. O protocolo da aplicação será experimental até revisão independente.

## 3. Stack definida

| Camada | Escolha |
|---|---|
| Desktop | Tauri 2 |
| Interface | Vue 3, TypeScript estrito, Vite |
| Visual | Tailwind CSS e shadcn-vue; componentes locais |
| Estado visual | Pinia exclusivamente para estado não sensível |
| Ferramentas frontend | Node.js LTS e pnpm, com versões e lockfile fixados |
| Núcleo | Rust estável e Cargo workspace |
| Concorrência | Tokio; canais e filas com limites explícitos |
| Transporte confiável | WebRTC DataChannel nativo, confiável e ordenado |
| Conectividade | ICE, STUN e TURN |
| Backend WebRTC inicial | Implementação Rust mantida da família webrtc-rs, após prova de compatibilidade |
| Canal autenticado de aplicação | Noise por `snow` |
| Envelopes | HPKE por implementação existente da RFC 9180 |
| Identidade | Ed25519, com implementação consolidada como `ed25519-dalek` |
| Memória | `zeroize` e integração com APIs nativas |
| Formato de protocolo | CBOR limitado, com codificação determinística definida |
| Sinalização | Rust, Tokio, Axum e WebSocket seguro |
| STUN/TURN | Coturn, com configuração de referência |
| Testes | Cargo test, testes de propriedades, fuzzing, Vitest e testes desktop reais |
| Distribuição | GitHub Actions e Releases; pacotes Windows/Linux |

Node.js é ferramenta de desenvolvimento/build, não servidor de mensagens nem runtime obrigatório do produto distribuído. Rede, identidade e criptografia ficam no Rust, não na WebView. [R1]

Antes de desenvolver a interface completa, valide o backend WebRTC com dois peers e Coturn: caminho direto, relay UDP e cliente TURN por TCP/TLS. Confirme suporte nas plataformas-alvo. Se a biblioteca escolhida não atender, documente o bloqueio e avalie um backend existente, como libdatachannel por binding Rust. Não substitua por transporte caseiro nem finja suporte. [R2][R3][R4]

## 3A. Identidade visual e design system obrigatório — Veyra

A identidade visual **Veyra** é parte do escopo de implementação, não uma sugestão opcional. O nome público, título das janelas, ícones, instaladores, README, screenshots, documentação e pacotes devem utilizar **Veyra** de forma consistente. Não utilizar o antigo nome Privmsg como marca.

**Posicionamento:** mensageiro P2P open source, privado, direto e efêmero, com fallback criptografado por relay e sem persistência intencional de mensagens. A comunicação deve ser apresentada com clareza, sem prometer anonimato absoluto, ausência universal de rastros ou entrega em todas as redes.

**Slogan principal:** “Mensagens privadas, diretas e efêmeras.”

**Frase de apoio:** “Privado por padrão. Efêmero por design.”

**Tom de voz:** técnico, sóbrio, confiável, acessível e sem exageros publicitários. Evitar slogans como “impossível rastrear”, “segurança inviolável” ou “converse sem deixar rastros”. Não apresentar uma tela de chat com histórico permanente, mensagens antigas ou estados fictícios, pois isso contraria o produto.

### Identidade gráfica

- **Símbolo:** monograma geométrico em forma de “V”, com duas formas convergentes que sugerem ligação entre dois peers; contornos limpos, sem cadeados genéricos como marca principal.
- **Wordmark:** `Veyra`, em tipografia sans-serif moderna, preferencialmente Inter SemiBold/Bold, com proporções e espaçamento refinados.
- **Aplicações:** logotipo horizontal, símbolo isolado, ícone do aplicativo, versão monocromática, versões para fundo claro e escuro e marca reduzida para barra de título e bandeja.
- **Cores principais:** verde-aurora `#00E6A8`, azul-profundo `#0B2A34`, grafite `#1E293B`, cinza-claro `#E5E7EB`; verde e azul são a direção visual central. Usar gradiente verde-aurora → azul-petróleo apenas em logo e destaques pontuais; evitar excesso de efeitos neon.
- **Cores semânticas:** sucesso, atenção, erro e estados de conectividade devem ter tokens próprios, contraste adequado e nunca depender apenas de cor.
- **Tipografia:** Inter para interface, com fallbacks locais de sistema; nenhuma fonte carregada de CDN. Para fingerprints e dados técnicos, usar fonte monoespaçada local.
- **Geometria:** cantos moderadamente arredondados, espaçamento consistente em múltiplos de 4/8 px, ícones de traço fino e layout com hierarquia clara.
- **Temas:** dark como apresentação principal e light completo; ambos acessíveis, com contraste WCAG AA para texto e controles.
- **Motion:** transições discretas (150–250 ms), respeitando `prefers-reduced-motion`; não animar conteúdo sensível de forma que prolongue sua retenção visual.

### Componentes visuais e telas

Desenvolver um design system baseado em tokens CSS, Tailwind e componentes locais shadcn-vue. Criar componentes reutilizáveis `VeyraLogo`, `PeerIdentityCard`, `FingerprintVerification`, `ConnectionRouteBadge`, `EphemeralMessageCard`, `OneTimeViewer`, `ExpiryIndicator`, `PrivacyStatus` e `ConnectivityDiagnostics`.

A navegação inicial deve conter **Mensagens pendentes**, **Contatos da sessão**, **Convites**, **Conexões** e **Configurações**. A tela principal mostra apenas mensagens não consumidas; após leitura, o cartão desaparece sem criar histórico. A rota exibida deve ser **P2P direto** ou **via relay**, determinada por telemetria local real do transporte, não por estado simulado. A composição de mensagens deve ser simples e sem auto-save. Exibir prazos de expiração, verificação de identidade e falhas de entrega de forma inequívoca.

O ícone de leitura única deve comunicar que a mensagem será consumida, com confirmação antes da abertura e visualizador isolado que oculta o conteúdo ao perder foco. Não apresentar botões de exportar, salvar, copiar ou recuperar mensagens. Não armazenar conteúdo sensível em stores reativas globais.

### Nomenclatura oficial

- **Produto e executável desktop:** `Veyra` / `veyra`.
- **Aplicativo Tauri:** `veyra-desktop`.
- **Núcleo Rust:** `veyra-core`.
- **Protocolo:** `veyra-protocol`.
- **Criptografia:** `veyra-crypto`.
- **Identidade:** `veyra-identity`.
- **Transporte:** `veyra-transport`.
- **Conectividade:** `veyra-connectivity`.
- **Estado volátil:** `veyra-state`.
- **Cofre local:** `veyra-vault`.
- **Daemon local:** `veyra-daemon`.
- **IPC:** `veyra-ipc`.
- **Sinalização WSS:** `veyra-signaling`.
- **Infraestrutura TURN:** `veyra-relay` (configuração Coturn; não inventar um servidor de relay proprietário).
- **Testes e fuzzing:** `veyra-tests`.
- **Documentação:** `veyra-docs`.
- **Infraestrutura de desenvolvimento:** `veyra-infra`.

Utilizar monorepositório público `veyra` e padrão `veyra-{componente}` para crates, executáveis, pacotes e serviços. Se o nome não estiver disponível em registros públicos, **não escolher outra marca automaticamente**: reportar conflito e solicitar decisão. Verificar marca e disponibilidade antes do lançamento, sem afirmar que estão livres.

### Entregáveis de marca

Produzir assets **vetoriais editáveis em SVG** (símbolo e wordmark), ícones PNG de tamanhos necessários, `.ico` para Windows, ícones para pacotes Linux, tokens CSS, exemplos claro/escuro e guia de uso em `docs/brand-guidelines.md`. Não usar uma imagem raster como única fonte do logotipo: redesenhar a marca em SVG limpo e verificável. Se forem fornecidas imagens de referência, usá-las como orientação de direção de arte, corrigindo eventuais textos e inconsistências, sem copiar layouts com histórico de mensagens incompatíveis com o produto.

**Critérios de aceitação visuais:** identidade consistente em app/instalador/docs; contraste e navegação por teclado; nenhuma mensagem antiga exibida após consumo; nenhum dado sensível em notificações, título da janela, capturas automáticas ou caches da interface; componentes de status baseados em eventos reais do núcleo.

## 4. Organização do código

```text
veyra/
├── apps/desktop/
│   ├── src/                 # Vue, TypeScript e componentes
│   └── src-tauri/           # Orquestração desktop e IPC
├── crates/
│   ├── domain/              # Estados, regras e políticas
│   ├── protocol/            # Frames e validação
│   ├── identity/            # Identidade e autorização voláteis
│   ├── crypto/              # Noise, HPKE e assinaturas
│   ├── secure-memory/       # Buffers e integrações nativas
│   ├── messaging/           # Entrega, expiração e consumo
│   ├── volatile-state/      # Filas e deduplicação em RAM
│   ├── connectivity/        # ICE, rotas e reconexão
│   ├── transport-webrtc/    # DataChannel nativo
│   ├── ipc/                 # Comunicação local restrita
│   ├── daemon/              # Rede e execução em background
│   └── vault/               # Custódia volátil de chaves
├── services/signaling/
├── infrastructure/coturn/
├── infrastructure/development/
├── tests/{integration,security,connectivity,persistence}/
├── fuzz/
├── docs/brand-guidelines.md
├── docs/adr/
├── .github/workflows/
├── SECURITY.md
├── CONTRIBUTING.md
└── LICENSE
```

Separe domínio, transporte, criptografia e apresentação. Use traits apenas onde houver fronteiras reais: relógio, transporte, fonte de aleatoriedade, custódia e autorização. Não criar ORM, Repository Layer de persistência ou abstrações sem necessidade. Teste regras com TDD, relógio controlável e falhas injetadas.

## 5. Fronteiras de processos e confiança

Entregue um único produto e instalador, com componentes internos:

**Interface/Tauri:** composição, pareamento, visualização e aprovação local. Nunca recebe chaves privadas. Texto claro aparece apenas durante composição ou leitura e durante as transferências locais indispensáveis.

**Daemon de rede:** conectividade, canal Noise, envelopes cifrados, confirmações e filas. Possui os segredos necessários ao transporte, mas não as chaves privadas HPKE de abertura.

**Cofre volátil:** identidade de assinatura e chaves descartáveis, sem listener de rede externa e sem acesso direto à internet. Só atende operações tipadas e autorizadas. Não expor uma API genérica de assinatura ou descriptografia.

No Linux, usar IPC local com permissões e validação de credenciais. No Windows, named pipes com ACL explícita e rejeição de clientes remotos. Autentique a instância local com material efêmero transmitido por canal herdado ou mecanismo equivalente; não colocar segredos em argumentos, variáveis de ambiente ou arquivos.

O daemon não pode produzir sozinho uma autorização de abertura. O fluxo interface–cofre deve exigir uma ação local válida e vinculada à mensagem. Documente que separação em processos sob a mesma conta não protege integralmente contra malware com privilégios equivalentes ou administrativos.

## 6. Identidade, pareamento e confiança

Ao iniciar uma execução nova do núcleo, gere com CSPRNG do sistema: identidade Ed25519, par estático X25519 para Noise e `runtime_id` aleatório. “Estático” significa estável durante aquela execução, não salvo em disco. Não converter ou reutilizar a chave Ed25519 como chave X25519.

Associe a identidade Ed25519 à chave pública Noise por documento assinado, versionado e com separação de domínio. Após o handshake, compare a chave estática remota observada com a chave vinculada ao documento. Um documento autossinado demonstra consistência, não a identidade humana da outra pessoa.

O pareamento ocorre por convite temporário contendo endereço do serviço de encontro, identificador aleatório, expiração e impressão digital da identidade convidante. Convites têm entropia criptográfica, expiram e são consumidos após pareamento bem-sucedido. Evite links abertos em navegador, histórico do shell e registro de tokens em URLs.

Mostre impressão digital e código de verificação derivado do transcript e das identidades dos dois lados, por método documentado. Exija aprovação explícita e recomende comparação por canal independente. Nunca trate um nome exibido ou uma afirmação do serviço de sinalização como identidade verificada.

Troca de identidade ou reinício do núcleo exige novo pareamento e verificação. Fechar e reabrir a janela não muda a identidade se daemon e cofre continuarem ativos. O catálogo público de contatos sobrevive ao encerramento e aparece offline ao reabrir; não restaura a confiança da sessão anterior, não reconecta automaticamente nem associa identidades novas pelo nome. Revogar um contato remove sua entrada persistida, impede novas operações e descarta suas pendências locais, sem prometer apagar cópias remotas.

## 7. Camadas de transporte e criptografia

Adote esta pilha, sem confundir suas responsabilidades:

```text
Mensagem → envelope HPKE → canal Noise → DataChannel confiável
→ SCTP/DTLS/ICE → caminho direto ou relay TURN
```

DataChannels fornecem o transporte de mensagens; ICE seleciona conectividade e TURN retransmite. Não executar `snow::TransportState` diretamente sobre UDP sujeito a perda e reordenação. Use um DataChannel binário, confiável e ordenado para o fluxo Noise da sessão, com escritor serializado e backpressure. [R2][R5]

Configure `Noise_XX_25519_ChaChaPoly_SHA256`, sem handshake criptográfico inventado, sem mensagens de usuário antes da verificação e sem 0-RTT. Defina prologue, binding das identidades, papéis, versão e execução em `protocol.md`. Noise usa chaves DH; Ed25519 é a identidade adicional de assinatura, não uma substituição do DH. [R5][R6]

Não desabilite DTLS do DataChannel. Noise fornece autenticação de aplicação independente da infraestrutura; HPKE mantém pendências cifradas fora do componente de rede. Essas camadas aumentam complexidade e não justificam promessas de segurança multiplicada.

Se uma conexão for recriada, estabeleça um novo handshake Noise. Não reutilize estado criptográfico com contadores reiniciados. Preserve apenas o estado lógico ainda válido da execução. Migrações de caminho precisam ser testadas; não prometer migração transparente não suportada pela biblioteca.

## 8. Envelopes e chaves descartáveis

Use HPKE com `DHKEM(X25519, HKDF-SHA256)`, `HKDF-SHA256` e `ChaCha20-Poly1305`. Não implementar primitivas, nonce ou KDF manualmente. Para HPKE Base, autentique o envelope com assinatura Ed25519 sobre uma representação canônica de seu cabeçalho, chave encapsulada e ciphertext. Valide assinatura e autorização antes de consumir a chave receptora. [R7]

O cofre receptor gera pares HPKE independentes de uso único e disponibiliza ofertas públicas assinadas apenas a peers verificados. Vincule cada oferta a remetente, destinatário, execuções, suíte, identificador de chave e validade. A chave privada permanece no cofre. Nenhum serviço externo hospeda pré-chaves privadas ou material de recuperação.

Uma oferta só pode corresponder a um envelope. Retransmissões desse mesmo envelope são idempotentes; um envelope diferente reutilizando a oferta é rejeitado. Não reutilizar a chave entre mensagens ou destinatários.

Ao enviar, cifre no núcleo, elimine referências desnecessárias ao texto e destrua contexto HPKE e segredos de encapsulamento. Conserve apenas o envelope enquanto aguarda confirmação. No recebimento, só abrir após autorização local; destruir chave e contexto de abertura quando consumidos, expirados ou descartados.

Não afirmar que HPKE fornece automaticamente proteção contra replay ou sigilo diante de comprometimento da chave privada receptora. Não anunciar Double Ratchet, recuperação pós-comprometimento, deniabilidade ou segurança pós-quântica sem implementar e validar essas propriedades. [R7]

Para vários destinatários, faça fan-out: uma oferta, um envelope e um estado de entrega independentes por participante. Não compartilhar chave de conteúdo nem revelar a lista completa aos demais destinatários.

## 9. Protocolo de aplicação

Defina esquema CBOR fechado e determinístico para assinaturas/AAD. Limite bytes, profundidade, coleções e campos; rejeite duplicidades e versões incompatíveis. Não desserializar entrada arbitrária em estruturas sem limites.

Um envelope deve vincular ao menos:

```text
version, suite, sender_identity, recipient_identity,
sender_runtime_id, recipient_runtime_id, message_id,
one_time_key_id, expiration_policy, encapsulated_key,
ciphertext, sender_signature
```

Defina explicitamente quais bytes entram no AAD, nas assinaturas e na separação de domínio. Não assinar um formato e verificar outro.

Implemente operações `HELLO`, `IDENTITY_BINDING`, `KEY_OFFER`, `MESSAGE`, `RECEIVED`, `REJECTED` e `CLOSE`. Separe handshake, frames de controle e mensagens. Não adicionar presença pública, digitação ou recibo de leitura por padrão.

A confirmação `RECEIVED` deve ser autenticada e vinculada ao identificador, digest do envelope e execução correta. Ela significa somente aceitação em RAM pelo destinatário, nunca armazenamento durável nem leitura.

Replays são bloqueados por execução, identificador, chave descartável e cache limitado de consumo. Retenha apenas metadados mínimos de deduplicação enquanto necessários à validade daquela execução. Nunca reduza esse controle silenciosamente para economizar memória: aplique backpressure ou recuse novas mensagens.

## 10. Leitura única, validade e entrega

Implemente máquinas de estados separadas de envio e recebimento, evitando tratar entrega, abertura e expiração como um único evento.

```text
Envio: COMPOSING → SEALED → WAITING_FOR_PEER → SENT → ACKNOWLEDGED
Recepção: RECEIVED_IN_RAM → CLAIMED → DISPLAYING → DESTROYED
Descarte: EXPIRED | CANCELLED | REJECTED
Incerteza de envio: DELIVERY_UNCONFIRMED
```

`CLAIMED` deve ser atômico. Consuma a autorização antes de liberar plaintext. Uma segunda janela ou chamada concorrente não abre novamente. Se houver falha após o consumo, a mensagem poderá ser perdida. Não criar mecanismo de recuperação que contradiga leitura única.

Após ACK válido, o remetente descarta o envelope. Sem ACK, retransmite o mesmo envelope dentro do prazo, sem reutilizar nonces Noise. Se o receptor já o tiver recebido, responda idempotentemente sem revelar que ele foi lido. Perda de ACK resulta em “entrega não confirmada”, não em certeza de falha.

Valores iniciais: texto de até 4 KiB UTF-8, frame de aplicação até 16 KiB, até 16 pendências por contato e 128 globais, validade máxima de cinco minutos e exibição de até 30 segundos. Limite também conexões, convites, ofertas de chave e tentativas de handshake.

O prazo nunca é renovado por retry, reconnect ou reopen. Mantenha deadlines locais monotônicos. A validade receptora fica limitada pelo prazo local original da oferta descartável, sem confiar no relógio civil do remetente. A mensagem pode expirar antes de cinco minutos se a oferta vencer antes.

Trate suspensão, alteração do relógio, hibernação e retomada explicitamente. Se não puder demonstrar que o prazo segue válido, descarte; não prolongue. A limpeza em falha abrupta ou encerramento forçado não deve ser apresentada como execução garantida de destrutores.

## 11. Conectividade automática

Use ICE com candidatos IPv4/IPv6 e STUN; prefira caminhos diretos validados. Implemente fallback TURN por UDP e conexão cliente–TURN por TCP/TLS, incluindo infraestrutura na porta 443 quando disponível. TURN/TLS não é HTTP e não atravessa necessariamente proxies HTTP ou redes com inspeção de tráfego. [R3][R8]

Priorize conexão utilizável sem longas esperas: faça gathering e checks com prazos limitados, use relay se necessário e tente retornar a direto quando suportado. Mantenha a mesma política E2EE em todos os caminhos. Não retransmitir plaintext pela sinalização para “resolver” uma falha.

Mudança de rede deve disparar reavaliação e ICE restart quando suportado. Sem caminho válido, apresentar estado objetivo e manter apenas pendências cifradas ainda válidas.

PCP, NAT-PMP e UPnP são otimizações opcionais e desabilitáveis, não condição para o primeiro fluxo funcional. Se implementados, usar leases curtos, apenas portas próprias e remoção best-effort. Não habilitar UPnP no roteador nem alterar configurações administrativas usando credenciais.

Não desabilitar firewall, antivírus, UAC, swap ou hibernação globalmente. Regras pontuais de firewall exigem APIs oficiais e consentimento do sistema. Se a permissão for negada, tentar caminhos de saída/relay; não repetir solicitações coercitivamente.

Não iniciar descoberta mDNS que anuncie identidade por padrão. Nenhuma API externa de IP, analytics ou atualização deve ser consultada fora da lista documentada de serviços necessários. Não prometer conexão em toda rede.

## 12. Sinalização e infraestrutura real

Implemente serviço de encontro em Rust, com WSS, salas temporárias em memória, convites de alta entropia, expiração, limites e encaminhamento de offer/answer e candidatos ICE. Não criar cadastro nem caixa postal.

Assine e vincule a sinalização às identidades, convites, execuções, sequência e papéis. O serviço não pode substituir a identidade esperada sem detecção. Sinalização sensível não deve aparecer em URLs, access logs, erros ou ferramentas de observabilidade.

Documente que o servidor pode observar IPs e, conforme o formato escolhido, candidatos e documentos públicos de identidade. Assinatura não é criptografia; não chamar a sinalização de confidencial ponta a ponta sem uma camada real que implemente isso.

Forneça configuração Coturn para STUN/TURN, credenciais temporárias e cotas. O segredo usado para emitir credenciais TURN permanece no operador, nunca embutido no cliente. Use identificadores aleatórios, não nomes de contatos, e autenticação que não transforme o serviço em relay aberto.

Mantenha estado de alocação e autenticação transitório em RAM. Não introduzir SQLite, Redis, filas duráveis ou tabelas de usuários para facilitar a implementação. Serviços podem precisar de certificados TLS, configuração operacional e segredos de autenticação: esses são distintos das chaves ponta a ponta dos usuários.

Entregue Docker Compose de desenvolvimento, configuração de implantação e testes com um relay real. Docker é ferramenta de desenvolvimento/operação, não requisito para o usuário desktop. Separe documentação do operador da experiência de instalação do cliente.

O binário público precisa receber endpoints reais, certificados válidos e política de emissão de credenciais antes de oferecer conectividade externa automática. Na ausência deles, informe “infraestrutura não configurada”; não use domínios fictícios como se fossem operacionais nem dependa de TURN público gratuito sem autorização.

Permita múltiplos operadores, seleção de fallback e configuração avançada. Não anunciar descentralização completa se a distribuição ainda depender de um único serviço de sinalização. Documente custo de banda e disponibilidade; software gratuito não torna a infraestrutura sem custo.

## 13. Política dos retransmissores

Relays encaminham dados cifrados de sessões ativas. Não armazenam mensagens para usuários offline. Buffers de transporte devem ser limitados e descartados após encaminhamento, timeout ou encerramento; não são caixas de entrada.

Coturn e componentes de infraestrutura podem gerar logs por padrão. Configure e verifique logs de aplicação, stdout/stderr, containers, proxy reverso, journal e arquivos temporários. Não confundir desativar stdout com desativar todos os logs. Não habilitar captura de pacotes em produção. [R9]

Aplique quotas, rate limits, antiamplificação e restrições de destinos inadequados, com estado transitório. Não aceitar relay irrestrito. Falha de um operador não permite downgrade criptográfico.

Um relay possui suas próprias chaves TLS e credenciais operacionais; o requisito é não possuir chaves privadas dos usuários nem chaves E2EE. Mesmo encerrando TLS externo, ele só obtém tráfego protegido pelos endpoints.

Não é possível impor apagamento a um operador malicioso. Ele pode copiar ciphertext e metadados; a confidencialidade não deve depender da honestidade do relay. A política sem persistência de mensagens é uma obrigação da implementação e operação de referência, não uma prova sobre terceiros.

## 14. Persistência e exposição residual

| Informação | Política |
|---|---|
| Texto, rascunhos, envelopes, chaves privadas, convites, códigos, autorizações e estado de sessão | Somente RAM, com validade e limites |
| Contatos sincronizados: identificador público e rótulo local | Catálogo nativo versionado `contacts.json` em `app_data_dir`; impressão digital recalculada; preservado ao encerrar |
| Histórico, backup, exportação e recuperação | Não existem |
| Logs com conteúdo, IPs de peers, tokens, SDP ou chaves | Proibidos na configuração distribuída |
| Binários, assets e configuração pública da distribuição | Podem existir em disco |
| Preferências visuais | Em RAM nesta versão |
| TLS e configuração do operador | Fora da proibição de persistir mensagens; protegidos e documentados |
| Estado transitório de sinalização/TURN | RAM limitada; sem histórico por usuário |

Não usar localStorage, sessionStorage, IndexedDB, service worker, cache de conteúdo, plugins de persistência, arquivos temporários de mensagens nem stores globais contendo conversas.

O catálogo de contatos pertence ao coordenador Rust e deve ser separado do estado de sessão. Grave alterações sem depender do fechamento do aplicativo e remova a entrada persistida na revogação, inclusive sem sessão ativa. O esquema deve rejeitar campos desconhecidos; não serializar `SessionStatus`, mensagens, bindings, códigos ou confiança anterior. Erros de leitura ou gravação devem ser visíveis, sem simular salvamento bem-sucedido ou substituir silenciosamente um arquivo inválido.

Audite os diretórios e perfis criados pelo Tauri/WebView, WebView2/WebKit, bibliotecas e SO. Modo privado ou efêmero, quando disponível, não comprova ausência de qualquer arquivo. Nunca apresentar “gravar e apagar ao sair” como ausência de persistência. [R10]

Use proteção de buffers e exclusão de dumps onde suportadas. `mlock`/`VirtualLock` podem falhar, e `zeroize` não limpa cópias que outras camadas fizeram. Verifique retornos e defina quais falhas bloqueiam operações sensíveis. Não substituir essa verificação por um booleano de configuração. [R11][R12]

JavaScript, DOM, IPC, WebView, GPU, teclado, acessibilidade, swap, hibernação, snapshots e processos privilegiados podem manter ou capturar informações. Documente limites e minimize exposição sem prometer controlar tudo. Não utilizar dados reais em testes de captura, dump ou inspeção.

## 15. Interface e proteção da WebView

Crie interface minimalista em português do Brasil, com suporte claro/escuro, navegação por teclado e estados acessíveis. Sem painel de métricas decorativo, textos promocionais ou simulação de recursos não implementados.

Telas essenciais: início de sessão; criação/entrada de convite; verificação de identidade; contatos salvos acessíveis também sem sessão; compositor; pendências cifradas; visualizador de leitura única; diagnóstico de conexão e privacidade.

Exibir rota “direta” ou “retransmitida” baseada em dados reais, fingerprint, tempo restante e distinção entre enviado, recebido e entrega não confirmada. Não manter bolhas de histórico após consumo. A interface não deve mostrar corpo antes de autorização.

Destrua referências do compositor após selar e do visualizador após leitura, expiração, bloqueio ou fechamento. Ao perder foco, oculte imediatamente o conteúdo; não reinicie o prazo quando o foco retornar. Notificações, títulos, bandeja e seletor de janelas não exibem remetente ou mensagem.

Renderize mensagens apenas como texto escapado. Proibir HTML, `v-html`, scripts, previews automáticos, fontes externas e links carregados sem ação explícita. Validar UTF-8, tamanho e caracteres de controle.

Configure CSP restritiva e capabilities mínimas, por janela e comando. Restrinja também comandos próprios Tauri; não presumir que as permissões de plugins cubram toda a API. Não disponibilizar shell, filesystem amplo, HTTP arbitrário ou criação livre de janelas. Não carregar conteúdo remoto com acesso ao IPC. [R13][R14]

Sem DevTools e logs sensíveis por padrão nas builds distribuídas. Clipboard de conteúdo não é oferecido; convites só são copiados após ação explícita, com aviso sobre histórico/sincronização externos. Limpeza posterior do clipboard não equivale a apagar todas as cópias.

## 16. API local e execução em segundo plano

Exponha comandos estreitos como `start_session`, `create_invite`, `join_invite`, `verify_peer`, `send_message`, `list_pending`, `open_once`, `discard_message`, `revoke_peer`, `connection_status` e `end_session`.

Valide autorização, origem local, tipos, limites e estado no Rust. Nenhum método retorna chave privada. `list_pending` não inclui plaintext. `open_once` entrega conteúdo apenas ao solicitante autorizado, nunca por broadcast global de eventos.

Fechar a janela mantém daemon e cofre apenas quando o usuário habilitou recepção em segundo plano. Ofereça “Sair e descartar sessão” claramente. Não reiniciar processos perdidos fingindo que a sessão sobreviveu.

Use processo por usuário/bandeja no Windows e integração por usuário no Linux; autostart é opcional e desligado por padrão. Evite privilégios administrativos permanentes. Se o cofre for encerrado, todas as pendências que dependiam dele devem ser invalidadas.

## 17. Destinatário indisponível

Com a interface fechada e núcleo ativo, receber normalmente em RAM. Sem conexão, o remetente só conserva envelope cifrado até o prazo. Isso exige oferta válida previamente obtida para a mesma execução receptora.

Sem oferta válida, bloquear envio e não criar fila invisível de plaintext. Rascunho permanece apenas no compositor aberto e é descartado ao encerrar esse fluxo.

Reinício do destinatário invalida ofertas antigas; como o remetente apagou o original, pode ser necessário redigir de novo. Reinício do remetente perde a fila de saída. Ambos offline significa ausência de entrega. Relay não altera essas regras.

Explique esses resultados na interface antes de depender deles, inclusive perda possível após ACK e antes da abertura. Essa perda é consequência deliberada da política volátil.

## 18. Testes obrigatórios e evidências

Implemente testes unitários, propriedades de estado, integração real e fuzzing. Mocks servem a testes isolados, não para substituir o fluxo distribuído.

| Área | Evidência exigida |
|---|---|
| Criptografia | Vetores oficiais; identidade errada, assinatura adulterada, AAD modificado e ciphertext inválido rejeitados |
| Autorização | Peer não verificado não envia; troca de chave exige nova verificação |
| Consumo | Duas aberturas concorrentes não liberam duas respostas; falha após claim não restaura a chave |
| Replay | Duplicata idêntica é idempotente; envelope diferente com mesma chave e execução antiga são recusados |
| Tempo | Retry, reconnect, relógio civil e suspensão não prorrogam mensagens |
| Conectividade | Duas instâncias reais em direto e relay, UDP bloqueado com TURN/TCP/TLS e falha de operador |
| Ciclo de vida | Janela fechada com background, encerramento total, crash do cofre e troca de rede |
| Privacidade | Nenhum plaintext/chave sensível em sinais, relays, logs, arquivos ou stores controlados pela aplicação |
| Frontend | XSS, comandos IPC indevidos, deep links malformados e conteúdo em notificações |
| Recursos | Filas, buffers, handshakes e salas não crescem ilimitadamente |
| Plataforma | Interoperabilidade Windows↔Linux e instalação nas versões declaradas |

Teste perda de ACK, indisponibilidade durante entrega, desconexão depois da aceitação, mensagens para múltiplos destinatários e expiração de ofertas ainda não utilizadas.

Use redes/NATs simulados em laboratório e testes reais quando disponíveis. Qualquer alteração de firewall para testes deve ficar isolada em máquinas de teste e não ser confundida com requisito para usuários.

Ferramentas: `cargo fmt`, `cargo clippy`, `cargo test`, `cargo-fuzz`, auditoria de dependências Rust/Node, Vitest e automação desktop compatível com Tauri. Relate separadamente o que foi executado, passou, falhou e não pôde ser testado. Inspeção sem achados não prova ausência universal de vestígios.

## 19. Entrega e ordem de implementação

Implemente em incrementos verificáveis: contrato e modelo de ameaças; prova de conectividade; protocolo/criptografia e máquinas de estado; daemon/cofre/IPC; integração gráfica; infraestrutura; testes de privacidade; empacotamento.

O primeiro fluxo vertical deve permitir que dois processos reais façam pareamento, verifiquem identidades, troquem uma mensagem cifrada e a consumam uma vez. Depois prove o mesmo fluxo passando por Coturn. Não desenvolver toda a interface antes de validar esses dois caminhos.

Mantenha todos os recursos obrigatórios na matriz de entrega, mesmo quando ainda pendentes. Funcionalidades opcionais não devem bloquear uma base funcional nem esconder lacunas de autenticação, E2EE, expiração, relay ou ausência de persistência de mensagens.

Entregue código compilável, lockfiles, testes, scripts de desenvolvimento, configuração de infraestrutura, instalador Windows, pacote Linux e CI por plataforma. Usuários finais não precisam instalar Node, Rust, Docker ou configurar rede manualmente. Dependências de runtime devem ser tratadas pelo empacotamento e documentadas, sem afirmar compatibilidade Linux universal.

Configure CI com privilégios mínimos, ações fixadas, controle de dependências e artefatos verificáveis. Não incluir segredos ou chaves de assinatura no repositório. Não afirmar que binários foram assinados, reproduzidos ou auditados sem evidência.

Adote licença Apache-2.0 para o projeto, preservando avisos e compatibilidade das dependências. Não apresentar licença como garantia de segurança ou conclusão jurídica sobre a operação em todos os países.

## 20. Documentação e definição de pronto

Produza `docs/brand-guidelines.md`, os assets da marca, `README.md`, `architecture.md`, `protocol.md`, `threat-model.md`, `privacy.md`, `security-limitations.md`, `relay-operations.md`, `testing.md`, `IMPLEMENTATION_STATUS.md`, ADRs e `SECURITY.md`.

O modelo de ameaças deve incluir rede hostil, relay/sinalização maliciosos, MITM, substituição de chaves, replay, exaustão de memória, frontend comprometido e exposição residual no endpoint. Identifique o que a arquitetura protege e o que não protege.

Em `privacy.md`, diferencie confidencialidade de anonimato; buffers transitórios de histórico; identidade persistente de identidade da execução; recebimento de leitura; operador honesto de operador malicioso; gravação intencional da aplicação de vestígios do SO.

A entrega só é funcional quando duas instâncias trocam mensagens reais em caminho direto e por relay, as identidades são verificadas, a abertura é at-most-once, os prazos funcionam, as filas não persistem e os pacotes podem ser instalados nas plataformas declaradas. Não chamar de pronto se houver mocks no caminho principal.

Ao encerrar o trabalho, informe o que foi implementado, como executar, testes e resultados observados, bloqueios reais, endpoints ainda necessários e riscos sem validação. Não chamar o software de auditado nem adequado a alta sensibilidade sem revisão independente.

**Regra final: se uma garantia obrigatória não puder ser mantida, recuse a operação ou descarte a mensagem. Não persista mensagens ou segredos, não rebaixe a criptografia e não simule sucesso. A persistência autorizada limita-se ao catálogo público de contatos.**

---

## Referências primárias para consulta e implementação

As referências fundamentam os componentes; não constituem auditoria da composição proposta. Verifique a versão efetivamente selecionada antes de usar qualquer API.

- [R1] Tauri — modelo de processos: `https://v2.tauri.app/concept/process-model/`
- [R2] RFC 8831 — WebRTC Data Channels: `https://www.rfc-editor.org/rfc/rfc8831.html`
- [R3] RFC 8835 — Transports for WebRTC: `https://datatracker.ietf.org/doc/rfc8835/`
- [R4] WebRTC.rs — implementação: `https://github.com/webrtc-rs/webrtc`
- [R5] Snow — modos de transporte Noise: `https://docs.rs/snow/latest/snow/`
- [R6] Noise Protocol Framework: `https://noiseprotocol.org/noise.html`
- [R7] RFC 9180 — HPKE e seus limites: `https://www.rfc-editor.org/rfc/rfc9180.html`
- [R8] RFC 8656 — TURN: `https://www.rfc-editor.org/rfc/rfc8656.html`
- [R9] Coturn — configuração: `https://github.com/coturn/coturn/blob/master/examples/etc/turnserver.conf`
- [R10] Tauri — configuração: `https://v2.tauri.app/reference/config/`
- [R11] Zeroize — garantias e limitações: `https://docs.rs/zeroize/latest/zeroize/`
- [R12] Libsodium — documentação de memória segura: `https://doc.libsodium.org/memory_management`
- [R13] Tauri — capabilities: `https://tauri.app/security/capabilities/`
- [R14] Tauri — Content Security Policy: `https://v2.tauri.app/security/csp/`
- [R15] RFC 8445 — ICE: `https://www.rfc-editor.org/rfc/rfc8445.html`
- [R16] HPKE — biblioteca Rust: `https://docs.rs/hpke/latest/hpke/`
- [R17] Tauri — instalador Windows: `https://v2.tauri.app/distribute/windows-installer/`
- [R18] libdatachannel — backend alternativo: `https://github.com/paullouisageneau/libdatachannel`
