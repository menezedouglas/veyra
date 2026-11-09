# Matriz de entrega — Veyra

Atualização: 2026-11-09. **Não pronto para lançamento.** Código presente não é
sinônimo de evidência de instalação, interoperabilidade ou ausência de vestígios.
Os resultados observados e comandos estão em `docs/testing.md`.

| Requisito obrigatório | Implementação / evidência | Lacuna restante |
|---|---|---|
| Monorepositório, marca, Apache-2.0 | Cargo workspace, Vue/Tauri, locks, SVG/PNG/ICO, tokens e guia | Verificação registral da marca antes de lançamento |
| Windows e Linux | Código e CI por plataforma; pacote Debian instalado/testado; início Windows recusa IPC ainda não implementado | Windows não executado; não há instalador Windows validado |
| Uma aplicação bidirecional | Mesmo runtime inicia/aceita; duas UIs Linux instaladas enviam/recebem | Interoperabilidade Windows↔Linux e desktop por relay |
| Direct e relay | Dois peers reais; Coturn UDP e TCP/TLS; com UDP bloqueado no cliente, TCP/TLS passaram e UDP falhou | GUI por relay, redes/NATs externos, falha de operador e troca de rede |
| Escolha backend comprovada antes UI completa | webrtc-rs UDP provado, TCP/TLS recusado; backend libdatachannel/libnice/TLS e pacote Linux testados | Backend nativo e instalador Windows |
| E2EE exclusiva endpoints | Noise XX + binding Ed25519 + HPKE Base assinado | Revisão independente da composição |
| Sem plaintext em relay/sinalização | Fluxo de mensagem só envelope→Noise→DataChannel | Inspeção dinâmica abrangente de logs/arquivos/infra |
| Identidade por núcleo | Ed25519 no cofre, Noise privado gerado no daemon, binding público assinado e runtime aleatório | Background/reabertura de janela ainda desabilitados |
| Cofre separado sem rede | Binário com pipes herdados; daemon recebe somente operações estreitas | Sandbox de syscall/OS e defesa contra malware equivalente não garantidos |
| Daemon não autoriza abertura | Enum fechado NetworkVaultRequest omite Open/Verify/Seal; teste de rejeição e fluxo real em quatro processos | Revisão independente da fronteira IPC |
| IPC Linux/Windows exigido | Bootstrap efêmero, limite/timer/EOF; Linux fstat valida FIFO/UID/permissões | Windows named pipes com ACL explícita e rejeição remota pendentes |
| Convite temporário | 256 bits, expiração, fingerprint/binding, WSS assinado | Experiência multioperador/fallback e recuperação de conectividade |
| Comparação explícita | Fingerprint e código do transcript, aprovação local no cofre/runtime; fluxo exercitado nas duas UIs | Avaliação humana de usabilidade e Windows |
| Ofertas descartáveis | Uma chave independente por envelope/destinatário, assinatura e prazo | Teste adversarial distribuído de exaustão prolongada |
| Leitura no máximo uma vez | Claim antes de decriptar, concorrência e falha pós-claim; abertura/consumo nas duas UIs Linux | Validação Windows e crash durante visualização |
| Replay e ACK | Digest, execução, ID/chave, cache limitado e ACK autenticado/idempotente | Perda de ACK e quedas de rede distribuídas mais amplas |
| Prazos | Deadline da oferta, CLOCK_BOOTTIME Linux, limpeza periódica do cofre | Suspensão/hibernação reais e Windows ainda não validados |
| Texto e fan-out | Limite UTF-8/controles, envelopes/estados independentes, filas limitadas | Teste gráfico multiusuário de carga e UX de recibos |
| Interface PT-BR | Cinco abas, tema, convites, verificação, compositor, pendências e viewer; 16+19 checks nativos instalados | Windows, contraste em todos os estados e acessibilidade manual |
| Viewer isolado | Resposta local, confirmação, prazo sem renovação; ocultação por perda de foco e ausência de histórico comprovadas na UI Linux | Windows, GPU e auditoria abrangente de resíduos |
| WebView restrita | CSP, comandos próprios mínimos, origem/janela, incognito, sem plugins amplos | Auditoria dinâmica de perfis; incompatibilidade WebDriver/incognito registrada |
| Estado sem persistência intencional | RAM apenas, sem storage/browser DB/filas duráveis, locks verificados | SO, WebView, swap, hibernação e snapshot continuam fora da garantia |
| Endpoints e certificados reais | WSS implementado; configuração fornecida pelo operador | Nenhum serviço público configurado ou implantado |
| Credenciais TURN temporárias | Emissão REST no WSS autenticado e relay automático real sem ICE configurado nos clientes | Renovação periódica, política pública de antiabuso e múltiplos operadores |
| Infra sem mailbox | Coturn/Compose com quotas, sem DB de mensagem e logs restritos | Verificação operacional do host/proxy/journal de produção |
| Background e bandeja | Desabilitados; fechar encerra a sessão | Integração por usuário Windows/Linux, reopen e crash |
| Rede alterada / ICE restart | Erro objetivo e nenhuma redução de criptografia | Reconexão/ICE restart/migração ainda pendentes |
| PCP/NAT-PMP/UPnP opcionais | Não implementados; não necessários ao laboratório | Não são bloqueadores de base |
| Testes cripto/estado/propriedades | Vetores oficiais, adulteração, prazo, replay, concorrência, fan-out | Cobertura não constitui auditoria |
| Fuzzing e auditorias | libFuzzer executado; RustSec e pnpm audit executados | Campanhas longas e revisão de dependências transitivas |
| CI e pacotes | Workflows configurados, staging sidecars, gate de release; Debian release instalado sem runtime Node/Rust | CI remoto, assinatura e instalador Windows não observados |
| Segurança pública | Modelo de ameaças, limitações, SECURITY e ADRs | Canal privado de reporte antes de lançamento |

Pendências não são substituídas por mocks nem por estado simulado. Ausência de
uma garantia necessária recusa a operação ou descarta o material. A entrega
integral do prompt permanece incompleta enquanto a matriz obrigatória não fechar.
