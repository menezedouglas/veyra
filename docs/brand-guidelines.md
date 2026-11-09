# Veyra — guia de marca

**Mensagens privadas, diretas e efêmeras.**

Privado por padrão. Efêmero por design.

Veyra é um mensageiro desktop experimental. A marca comunica ligação entre dois peers e o caráter transitório da sessão. Não implica anonimato, conexão universal, apagamento absoluto ou auditoria de segurança.

## Arquivos e construção

As fontes editáveis estão em `assets/brand/`: `symbol.svg`, `symbol-mono.svg`, `logo-dark.svg`, `logo-light.svg` e `icon.svg`. O símbolo contém dois polígonos convergentes, sem bitmap, filtro ou recurso externo. Os wordmarks mantêm texto editável em Inter SemiBold com fallback local. Ao exportar material final, use a fonte distribuída localmente; o texto não foi convertido em curvas para manter edição simples.

O campo do símbolo é 96 × 96 unidades. Preserve proporções e área livre mínima de 12 unidades. Tamanho recomendado do símbolo: 24 px ou mais; a variante de 16 px se destina à bandeja e ao instalador. Não esticar, girar, adicionar cadeado, sombra ou brilho. Use `currentColor` na variante monocromática em fundos com contraste suficiente.

`python3 scripts/generate-icons.py` gera, de forma determinística e sem dependências externas, PNGs RGBA de 16, 24, 32, 48, 64, 128, 256 e 512 px e um ICO multirresolução em `apps/desktop/src-tauri/icons/`. O script usa as mesmas coordenadas de polígonos do SVG; alterações de geometria exigem atualizar ambos. A fonte vetorial permanece o arquivo de referência. PNGs são derivados para empacotamento Windows e Linux, não a única fonte do logotipo.

## Cor, tipografia e temas

| Token de marca | Valor | Uso |
| --- | --- | --- |
| Aurora | `#00E6A8` | Símbolo, foco e ação principal no tema escuro |
| Azul profundo | `#0B2A34` | Ícone, fundos e wordmark claro |
| Grafite | `#1E293B` | Referência neutra |
| Cinza claro | `#E5E7EB` | Texto no tema escuro |

A aurora sobre branco não tem contraste para texto: o tema claro usa verde `#00795B` para ações e foco. Semântica possui tokens próprios de sucesso, atenção, erro e rota. Estados sempre têm rótulos, além de cor.

`apps/desktop/src/styles/tokens.css` é a referência do design system. A UI usa Inter, empacotada por `@fontsource/inter`, sem CDN. Fingerprints e dados técnicos usam monospace do sistema. Cantos de 6–10 px, espaçamento em múltiplos de 4/8 px, ícones finos e transições de 150–250 ms. Não animar texto sensível; `prefers-reduced-motion` reduz movimento.

Contraste calculado sobre as cores sólidas, sem transparência:

| Par | Escuro | Claro |
| --- | ---: | ---: |
| Texto / fundo | 14,22:1 | 13,27:1 |
| Texto secundário / cartão | 8,30:1 | 6,26:1 |
| Texto do botão / primária | 9,59:1 | 5,40:1 |
| Atenção / cartão | 10,52:1 | 6,50:1 |
| Erro / cartão | 8,18:1 | 6,88:1 |
| Sucesso / cartão | 9,92:1 | 6,38:1 |

Esses números não substituem avaliação da interface renderizada ou navegação assistiva. `assets/brand/examples.html` reúne aplicações estáticas clara e escura sem mensagens ou estados fictícios.

## Linguagem e experiência

Usar “Veyra” em janelas, pacotes, README e instaladores. Executável: `veyra`; pacote Tauri: `veyra-desktop`. Não usar a marca anterior. O nome e eventuais marcas de terceiros não tiveram disponibilidade jurídica verificada; essa validação antecede lançamento público.

A interface é em português do Brasil. Rota é “P2P direto”, “via relay” ou estado objetivo não conectado, sempre a partir do núcleo. “Recebida” significa aceitação em RAM, não leitura. “Entrega não confirmada” preserva a incerteza quando falta ACK.

Leitura única exige confirmação e consome a autorização antes da resposta. Ao perder foco, o visualizador oculta e descarta sua referência local; não oferece copiar, salvar ou recuperar. Não apresentar histórico, métricas decorativas, nomes ou texto em títulos/notificações. O tema vive apenas em RAM. Nenhum modo visual deve simular criptografia, peers, mensagens ou disponibilidade de infraestrutura.

## Componentes locais e referências

`components.json` configura o padrão shadcn-vue. O botão local usa variantes tipadas, Tailwind e `Primitive` de Reka UI; componentes passam a fazer parte do repositório. Pinia é restrita à aparência, sem sessão ou conteúdo sensível.

Documentação primária consultada em 9 de novembro de 2026: [shadcn-vue, instalação manual](https://next.shadcn-vue.com/docs/installation/manual), [Tailwind v4](https://next.shadcn-vue.com/docs/tailwind-v4), [Tauri, chamadas Rust](https://v2.tauri.app/develop/calling-rust/) e [Node.js, versões LTS](https://nodejs.org/en/about/previous-releases). Dependências exatas e respectivas integridades estão no manifesto e lockfile, sem afirmar auditoria.

## Interface implementada

A navegação local contém Mensagens pendentes, Contatos da sessão, Convites, Conexões e Configurações. Os componentes `VeyraLogo`, `PeerIdentityCard`, `FingerprintVerification`, `ConnectionRouteBadge`, `EphemeralMessageCard`, `OneTimeViewer`, `ExpiryIndicator`, `PrivacyStatus` e `ConnectivityDiagnostics` estão em `apps/desktop/src/components/`. `MessageComposer` e `InvitePanel` integram os comandos nativos, sem peers, rotas ou mensagens de demonstração no aplicativo.

O endpoint do operador é opcional e vive em RAM. Deve ser WSS com caminho `/v1/rendezvous`, sem credenciais ou tokens na URL. O navegador recusa operações nativas. Configurar o endpoint não comprova disponibilidade; o núcleo informa rotas e falhas. Não há campo para inserir o segredo operacional do TURN.

O visualizador exige confirmação antes de `open_once`. O cartão é ocultado já na tentativa de consumo; falhas podem perder a mensagem. A resposta é mantida apenas no componente local e renderizada como texto. Perder foco, ocultar o documento, expirar, encerrar a sessão ou fechar o diálogo descarta a referência. Uma resposta que chega após perda de foco ou desmontagem não é mostrada. O prazo é limitado pelo menor orçamento original, nunca renovado pelo polling. Esses cuidados minimizam exposição; JavaScript, WebView, GPU, acessibilidade e SO podem manter outras cópias.

O compositor limpa sua referência imediatamente após a transferência IPC indispensável, inclusive quando o núcleo recusa o envio. Não oferece recuperação do rascunho. Resultados de fan-out são metadados por destinatário; “enviada” não é apresentado como recebimento. O clipboard é oferecido somente para convites, por ação explícita e com aviso prévio sobre histórico externo. Não há cópia programática de mensagens.

Diálogos locais usam Reka UI, gerenciamento de foco e Escape. O conteúdo sensível não recebe transições. O handler de erros não encaminha estado de componentes ao console ou a telemetria; em erro não tratado, desmonta a interface e exibe aviso fixo.


A validação frontend final passou 38 testes em sete arquivos Vitest, além de build e TypeScript estrito. Entre eles: texto escapado/XSS, confirmação antes da chamada nativa, dupla chamada, resposta tardia após blur, deadline original, destruição do rascunho em recusa, destinatário não verificado e tamanho UTF-8. Atualizações de entrega são associadas somente por `messageId` **e** `peerId`, com cache de metadados limitado e em RAM; ACK de uma mensagem anterior não confirma outra mensagem para a mesma pessoa. A prova desktop real e seus limites estão em `docs/desktop-qa.md`.

Os budgets de pendências e de exibição são ancorados antes da chamada IPC; uma resposta atrasada não acrescenta tempo. Dois testes adicionais simulam atraso de 2,5 segundos diante de orçamento de 1 segundo e confirmam que não há cartão nem corpo liberado após esse prazo.

O visualizador compara os deltas de relógio civil e performance a partir da abertura. Divergência superior a aproximadamente um segundo somente invalida a visualização; não acrescenta tempo. Testes cobrem avanço civil de 20 minutos com performance parado e retrocesso civil de cinco segundos. A limpeza ocorre quando o JavaScript volta a executar e não implica controle de GPU, SO ou outras cópias.
